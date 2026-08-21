//! How the host learns that a session opened a pull request.
//!
//! ACP has no "pull request opened" event, so linkage is an overlay — the same
//! shape as the Inbox (`docs/research/git-and-prs/pr-linkage.md`). The evidence
//! ladder in that file, high confidence to low:
//!
//! 1. a `/pull/<n>` URL in the **stdout of an execute tool call** — which is
//!    exactly what `gh pr create` prints;
//! 2. `gh pr view` in the session's own worktree at turn end, which also
//!    catches a PR opened in the browser or by an MCP server;
//! 3. matching the thread's head branch against open PRs.
//!
//! This module owns (1) and the trigger for (2). Two rules it will not bend.
//!
//! **Only execute output counts.** An agent that *reads a file* mentioning a
//! pull request has not opened one, and a link written from that is a PR
//! attributed to the wrong conversation forever — the link is written once and
//! never re-derived. So a tool call is scanned only if the adapter called it
//! `execute`, and because a `tool_call_update` need not repeat the kind, the
//! ids of the execute calls in flight are remembered per thread.
//!
//! **What the agent *says* is not evidence.** "I opened PR #23" in a chat
//! bubble is a sentence, and agents invent numbers. It raises the flag that
//! makes the host go and *ask* `gh`; it never writes a row (`pr-linkage.md` §4).

use std::collections::HashSet;

use serde_json::Value;

/// One pull request, as a link found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrLink {
    pub host: String,
    pub owner: String,
    pub name: String,
    pub number: i64,
    pub url: String,
}

impl PrLink {
    /// `owner/name` — the one spelling `gh --repo` and `thread_prs.repo` use.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// What one turn's ACP traffic told us, per thread. RAM, like the prompt queue:
/// a turn that a restart interrupted has no half-parsed state worth keeping,
/// and the post-turn `gh` fallback is what covers that case anyway.
#[derive(Debug, Default)]
pub struct PrWatch {
    /// Tool call ids the adapter declared as `execute`. Only their output is
    /// trusted as evidence.
    execute_calls: HashSet<String>,
    /// Something in this turn suggested a PR without proving one: a
    /// `gh pr create` command, or the agent saying it opened one.
    pub suspected: bool,
}

impl PrWatch {
    /// Consume one `session/update` payload; return every PR link it proves.
    pub fn observe(&mut self, acp: &Value) -> Vec<PrLink> {
        let kind = acp.get("sessionUpdate").and_then(Value::as_str);
        match kind {
            Some("tool_call") | Some("tool_call_update") => self.observe_tool_call(acp),
            Some("agent_message_chunk") => {
                // Display-only evidence: it decides whether we bother asking
                // `gh` at turn end, and writes nothing by itself.
                if mentions_a_pull_request(&text_of(acp.get("content"))) {
                    self.suspected = true;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn observe_tool_call(&mut self, acp: &Value) -> Vec<PrLink> {
        let id = acp
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let declared_execute = acp.get("kind").and_then(Value::as_str) == Some("execute");
        if declared_execute && !id.is_empty() {
            self.execute_calls.insert(id.to_string());
        }
        let is_execute = declared_execute || (!id.is_empty() && self.execute_calls.contains(id));
        if !is_execute {
            return Vec::new();
        }
        let mut text = String::new();
        // The command itself, wherever the adapter put it. `title` is what the
        // prototype's toolblock shows; `rawInput.command` is what ACP carries
        // for a shell call.
        for field in [acp.get("title"), acp.get("rawInput")] {
            let part = text_of(field);
            if !part.is_empty() {
                text.push_str(&part);
                text.push('\n');
            }
        }
        text.push_str(&text_of(acp.get("content")));
        if opens_a_pull_request(&text) {
            self.suspected = true;
        }
        scan(&text)
    }

    /// Start of a new turn: the ids of the last turn's tool calls are no use,
    /// and a suspicion that was already acted on must not re-trigger.
    pub fn reset(&mut self) {
        self.execute_calls.clear();
        self.suspected = false;
    }
}

/// Every `/pull/<n>` link in a blob of text, in order, without duplicates.
///
/// A GitHub *compare* URL is not a pull request — it is the page you are shown
/// **before** you open one, and `git push` prints it every single time. Treating
/// it as linkage would link a PR that does not exist yet to a number it does
/// not have, so only `/pull/<n>` matches.
pub fn scan(text: &str) -> Vec<PrLink> {
    let mut found: Vec<PrLink> = Vec::new();
    for (index, _) in text.match_indices("/pull/") {
        let Some(link) = parse_around(text, index) else {
            continue;
        };
        if !found.contains(&link) {
            found.push(link);
        }
    }
    found
}

/// Read backwards from `/pull/` for `host/owner/name`, forwards for the number.
///
/// Hand-rolled because the crate has no regex dependency and this is the only
/// pattern the host has ever needed to match. The shapes accepted are the ones
/// `gh` and GitHub actually print: `https://host/o/r/pull/1`, the `http://`
/// form a GHES instance behind a proxy may emit, and the bare `host/o/r/pull/1`
/// that turns up in prose.
fn parse_around(text: &str, at: usize) -> Option<PrLink> {
    let bytes = text.as_bytes();
    let number_start = at + "/pull/".len();
    let mut end = number_start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == number_start {
        return None;
    }
    let number: i64 = text.get(number_start..end)?.parse().ok()?;
    if number <= 0 {
        return None;
    }

    // Walk left over the three path segments that precede `/pull/`.
    let head = text.get(..at)?;
    let start = head
        .rfind(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '(' || c == '<')
        .map(|i| i + head[i..].chars().next().map(char::len_utf8).unwrap_or(1))
        .unwrap_or(0);
    let candidate = head.get(start..)?;
    let candidate = candidate
        .split_once("://")
        .map(|(_scheme, rest)| rest)
        .unwrap_or(candidate);
    let mut segments = candidate.split('/');
    let host = segments.next()?;
    let owner = segments.next()?;
    let name = segments.next()?;
    // Exactly three segments before `/pull/`. A deeper path is some other
    // document that merely contains the word.
    if segments.next().is_some() {
        return None;
    }
    if host.is_empty() || owner.is_empty() || name.is_empty() || !host.contains('.') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let name = name.strip_suffix(".git").unwrap_or(name);
    Some(PrLink {
        url: format!("https://{host}/{owner}/{name}/pull/{number}"),
        host,
        owner: owner.to_string(),
        name: name.to_string(),
        number,
    })
}

/// A command that is trying to open a pull request. Used only to decide whether
/// the post-turn `gh pr view` is worth a subprocess — never to write a row.
fn opens_a_pull_request(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("gh pr create") || lower.contains("hub pull-request")
}

fn mentions_a_pull_request(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("pull request") || lower.contains("opened pr")
}

/// Every string in an arbitrary ACP content value, flattened.
///
/// ACP tool-call content is a union that has grown over versions — plain text,
/// `{ type: "content", content: { text } }`, `{ type: "terminal" }` with the
/// output somewhere inside — and an adapter may nest any of them. Walking the
/// JSON for strings is stable against all of that, and the only thing being
/// looked for is a URL, so a stray field name in the haystack costs nothing.
/// Diff hunks are skipped: a patch that *adds* a link to a README is not a
/// pull request being opened.
fn text_of(value: Option<&Value>) -> String {
    let mut out = String::new();
    if let Some(value) = value {
        collect_text(value, &mut out);
    }
    out
}

fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("diff") {
                return;
            }
            for (key, item) in map {
                // Field names are not content, and a key called `oldText`
                // would otherwise drag a whole file in behind it.
                if key == "oldText" || key == "newText" {
                    continue;
                }
                collect_text(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_url_gh_pr_create_prints() {
        let links = scan("https://github.com/jabreeflor/jabot/pull/23\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].slug(), "jabreeflor/jabot");
        assert_eq!(links[0].number, 23);
        assert_eq!(links[0].host, "github.com");
    }

    #[test]
    fn accepts_the_shapes_that_actually_turn_up() {
        // Bare, http, an enterprise host, a trailing `.git`, and a URL sitting
        // inside prose or quotes.
        for text in [
            "github.com/o/r/pull/7",
            "http://github.com/o/r/pull/7",
            "https://git.corp.example.com/o/r/pull/7",
            "see https://github.com/o/r.git/pull/7 for details",
            "\"https://github.com/o/r/pull/7\"",
        ] {
            let links = scan(text);
            assert_eq!(links.len(), 1, "{text}");
            assert_eq!(links[0].number, 7, "{text}");
            assert_eq!(links[0].owner, "o", "{text}");
            assert_eq!(links[0].name, "r", "{text}");
        }
        assert_eq!(
            scan("https://git.corp.example.com/o/r/pull/7")[0].host,
            "git.corp.example.com"
        );
    }

    /// The failure this costs the most: a compare URL is what `git push`
    /// prints on *every* push, and it names no pull request.
    #[test]
    fn a_compare_url_is_not_a_pull_request() {
        let pushed = "remote: Create a pull request for 'jabot/t1' on GitHub by visiting:\n\
                      remote:      https://github.com/o/r/compare/jabot/t1?expand=1\n";
        assert!(scan(pushed).is_empty());
        // Nor is a path with more segments in front of it, nor a missing number.
        assert!(scan("https://github.com/o/r/tree/main/pull/3").is_empty());
        assert!(scan("https://github.com/o/r/pull/").is_empty());
        assert!(scan("/pull/12").is_empty());
    }

    #[test]
    fn the_same_url_twice_is_one_link() {
        let links = scan(
            "https://github.com/o/r/pull/9\nhttps://github.com/o/r/pull/9\n\
             https://github.com/o/r/pull/10\n",
        );
        assert_eq!(links.len(), 2);
        assert_eq!(links[1].number, 10);
    }

    fn execute_call(id: &str, command: &str) -> Value {
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "kind": "execute",
            "title": command,
            "status": "in_progress"
        })
    }

    #[test]
    fn only_execute_output_is_evidence() {
        let mut watch = PrWatch::default();
        // A read tool whose output happens to contain a PR link proves nothing.
        let read = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-read",
            "kind": "read",
            "title": "CHANGELOG.md",
            "content": [{ "type": "content", "content": {
                "type": "text", "text": "see https://github.com/o/r/pull/4"
            }}]
        });
        assert!(watch.observe(&read).is_empty());

        // The same text out of a shell call is.
        watch.observe(&execute_call("call-1", "gh pr create --fill"));
        let out = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "completed",
            "content": [{ "type": "content", "content": {
                "type": "text", "text": "https://github.com/o/r/pull/4\n"
            }}]
        });
        let links = watch.observe(&out);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].number, 4);
    }

    /// The update that carries the output need not repeat `kind`, which is why
    /// the ids are remembered — and why an id from a *previous* turn must not
    /// keep counting once the turn is over.
    #[test]
    fn an_execute_id_is_remembered_within_the_turn_and_not_across_it() {
        let mut watch = PrWatch::default();
        watch.observe(&execute_call("call-1", "git push"));
        let update = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "content": "https://github.com/o/r/pull/5"
        });
        assert_eq!(watch.observe(&update).len(), 1);

        watch.reset();
        assert!(watch.observe(&update).is_empty());
    }

    #[test]
    fn a_gh_pr_create_command_raises_the_flag_even_with_no_url() {
        let mut watch = PrWatch::default();
        // Truncated stdout: the command ran, we never saw what it printed.
        assert!(watch
            .observe(&execute_call("call-1", "gh pr create --draft"))
            .is_empty());
        assert!(watch.suspected, "the fallback should be armed");
    }

    #[test]
    fn agent_prose_arms_the_fallback_and_writes_nothing() {
        let mut watch = PrWatch::default();
        let said = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "I opened pull request #99 for you." }
        });
        assert!(
            watch.observe(&said).is_empty(),
            "an agent's sentence is not a row"
        );
        assert!(watch.suspected);
    }

    /// A diff that adds a link to a file is a file change, not a pull request.
    #[test]
    fn a_diff_hunk_is_not_scanned() {
        let mut watch = PrWatch::default();
        let edit = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call-edit",
            "kind": "execute",
            "title": "apply patch",
            "content": [{
                "type": "diff",
                "path": "/repo/README.md",
                "oldText": "",
                "newText": "See https://github.com/o/r/pull/1\n"
            }]
        });
        assert!(watch.observe(&edit).is_empty());
    }
}
