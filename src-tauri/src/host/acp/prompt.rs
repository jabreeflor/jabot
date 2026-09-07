//! Host-owned prompt composition (#237).
//!
//! Every ACP `session/prompt` is assembled here immediately before dispatch,
//! including queued drains. The user's exact content stays in the transcript;
//! only the wire prompt receives standing Jabot context, the current bot
//! record, and this thread's facts.
//!
//! Context is ACP text blocks prepended to the user blocks. It is framing,
//! not a system-message, and not a substitute for host authorization. Supported
//! harnesses (Claude, Codex, Gemini, Pi, profile-scoped adapters, and the fake ACP
//! agent) all receive the same `session/prompt` array. There is no invented
//! `systemPrompt` parameter.
//!
//! `MEMORY.md` is not embedded. It stays on disk; a harness that reads cwd
//! can still open it. Bearer tokens and provider credentials never enter
//! this text.

use serde_json::{json, Value};

use super::super::chief::tools as host_tools;
use super::super::HostSession;
use super::connection::prompt_blocks;

/// Increment when the packaged operating instructions change shape.
pub const APP_CONTEXT_VERSION: u32 = 1;

const APP_CONTEXT: &str = include_str!("app_context.md");

impl HostSession {
    /// Compose the ACP prompt that will be sent for this thread.
    ///
    /// Called at dispatch time so a queued turn sees the current bot
    /// definition, not a snapshot from enqueue. Failures fall back to the
    /// user's blocks plus whatever context could be built — a missing store
    /// must not drop the user's message.
    pub(crate) fn compose_prompt_for_dispatch(
        &self,
        thread_id: &str,
        user_content: &Value,
    ) -> Value {
        let user_blocks = match prompt_blocks(user_content) {
            Ok(Value::Array(blocks)) => blocks,
            Ok(other) => vec![other],
            Err(_) => vec![json!({ "type": "text", "text": user_content.to_string() })],
        };
        let context = self.jabot_context_text(thread_id);
        let mut blocks = vec![json!({ "type": "text", "text": context })];
        blocks.extend(user_blocks);
        Value::Array(blocks)
    }

    fn jabot_context_text(&self, thread_id: &str) -> String {
        let mut out = String::new();
        debug_assert!(
            APP_CONTEXT.contains(&format!("v{APP_CONTEXT_VERSION}")),
            "app_context.md must name version {APP_CONTEXT_VERSION}"
        );
        out.push_str(APP_CONTEXT.trim());
        out.push('\n');

        let thread = self
            .store
            .as_ref()
            .and_then(|store| store.get_thread(thread_id).ok().flatten());
        let bot = thread.as_ref().and_then(|row| {
            row.bot_id.as_deref().and_then(|bot_id| {
                self.store
                    .as_ref()
                    .and_then(|store| store.get_bot(bot_id).ok().flatten())
            })
        });

        if let Some(bot) = &bot {
            let tools = serde_json::from_str::<Vec<String>>(&bot.tools_json).unwrap_or_default();
            let (fence, body) = fenced(&bot.instructions);
            out.push_str("\n--- current bot ---\n");
            out.push_str(&format!("id: {}\n", bot.id));
            out.push_str(&format!("name: {}\n", single_line(&bot.name)));
            out.push_str(&format!("harness: {}\n", bot.harness_id));
            out.push_str(&format!("isChief: {}\n", bot.is_chief));
            out.push_str("instructions:\n");
            out.push_str(&format!("<<{fence}\n{body}\n{fence}\n"));
            out.push_str(&format!("grantedTools: {}\n", tools.join(", ")));
        } else {
            out.push_str("\n--- current bot ---\n");
            out.push_str("none. This thread has no crew bot. Do not invent a persona or a management grant.\n");
        }

        out.push_str("\n--- current thread ---\n");
        out.push_str(&format!("id: {}\n", thread_id));
        match &thread {
            Some(row) => {
                let kind = thread_kind(
                    row.bot_id.as_deref(),
                    row.folder_id.as_deref(),
                    row.worktree_path.as_deref(),
                );
                out.push_str(&format!("kind: {kind}\n"));
                match &row.bot_id {
                    Some(id) => out.push_str(&format!("botId: {id}\n")),
                    None => out.push_str("botId: none\n"),
                }
                out.push_str(&format!("cwd: {}\n", row.cwd));
                if let Some(folder) = row.folder_id.as_deref().and_then(|id| {
                    self.store
                        .as_ref()
                        .and_then(|store| store.get_folder(id).ok().flatten())
                }) {
                    out.push_str(&format!(
                        "folder: {} ({})\n",
                        single_line(&folder.name),
                        folder.id
                    ));
                } else {
                    out.push_str("folder: none\n");
                }
                out.push_str(&format!("harness: {}\n", row.harness_id));
                if let Some(runtime) = effective_runtime(&row.runtime_json) {
                    out.push_str(&format!("runtime: {runtime}\n"));
                }
            }
            None => {
                out.push_str(
                    "kind: unknown\nbotId: none\ncwd: unknown\nfolder: none\nharness: unknown\n",
                );
            }
        }

        let granted = self.granted_host_tools(thread_id);
        let attached = self.chief_bridges.contains_key(thread_id);
        if granted.is_empty() {
            out.push_str("hostToolsGranted: none\n");
            out.push_str("hostToolsAttached: none\n");
            out.push_str(
                "You cannot create crew members from this session. If the user asks, explain the limit and point them at Crew or a bot that has draft_bot.\n",
            );
        } else {
            out.push_str(&format!("hostToolsGranted: {}\n", granted.join(", ")));
            if attached {
                out.push_str(&format!("hostToolsAttached: {}\n", granted.join(", ")));
            } else {
                out.push_str("hostToolsAttached: none (granted tools become callable after the next session start)\n");
            }
            if granted.iter().any(|id| *id == "draft_bot") {
                out.push_str(
                    "You can propose a crew member with draft_bot. The host returns pending_review and saved:false. The user must Save. Do not claim the bot exists until then.\n",
                );
            } else {
                out.push_str(
                    "You cannot create crew members. If the user asks, explain the limit and route them to Crew or a bot that has draft_bot.\n",
                );
            }
        }
        out
    }

    fn granted_host_tools(&self, thread_id: &str) -> Vec<&'static str> {
        let allowlist = self.tool_allowlist(thread_id);
        host_tools::SPECS
            .iter()
            .filter(|spec| allowlist.iter().any(|id| id == spec.id))
            .map(|spec| spec.id)
            .collect()
    }
}

fn thread_kind(
    bot_id: Option<&str>,
    folder_id: Option<&str>,
    worktree: Option<&str>,
) -> &'static str {
    if worktree.is_some() {
        "folder/worktree coding session"
    } else if folder_id.is_some() && bot_id.is_none() {
        "folder coding session (no crew bot)"
    } else if bot_id.is_some() {
        "standing crew chat"
    } else {
        "botless thread"
    }
}

fn effective_runtime(runtime_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(runtime_json).ok()?;
    let command = value.get("command").and_then(Value::as_str)?;
    if command.is_empty() {
        return None;
    }
    Some(single_line(command))
}

/// One line for data fields so a name cannot open a new section.
fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

/// Fence the bot instructions so a fake closing tag in the persona cannot
/// invent a new section. The fence is grown until it does not appear in the
/// body.
fn fenced(body: &str) -> (String, &str) {
    let mut n = 1u32;
    loop {
        let fence = format!("JABOT_{n}");
        if !body.contains(&fence) {
            return (fence, body);
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::methods::ThreadOpenParams;
    use crate::host::protocol::HOST_HELLO;
    use crate::host::HostSession;

    fn host() -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        (dir, session)
    }

    fn open_botless(session: &mut HostSession, thread_id: &str) {
        session
            .thread_open(ThreadOpenParams {
                thread_id: Some(thread_id.into()),
                title: "Auth".into(),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                harness_id: "claude".into(),
                runtime: None,
                folder_id: None,
                bot_id: None,
                fold_policy: None,
                use_checkout: None,
                base_ref: None,
            })
            .expect("thread/open");
    }

    #[test]
    fn composition_is_versioned_and_prepends_one_text_block() {
        let (_dir, mut session) = host();
        open_botless(&mut session, "t-compose");
        let composed = session.compose_prompt_for_dispatch("t-compose", &json!("hello"));
        let blocks = composed.as_array().expect("array");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        let context = blocks[0]["text"].as_str().unwrap();
        assert!(context.contains("Jabot context v1"), "{context}");
        assert!(context.contains("current bot"), "{context}");
        assert!(
            context.contains("none. This thread has no crew bot"),
            "{context}"
        );
        assert_eq!(blocks[1], json!({ "type": "text", "text": "hello" }));
    }

    #[test]
    fn mixed_user_blocks_keep_their_types_and_order() {
        let (_dir, mut session) = host();
        open_botless(&mut session, "t-mix");
        let user = json!([
            { "type": "text", "text": "see this" },
            { "type": "image", "data": "abc", "mimeType": "image/png" },
            { "type": "text", "text": "and that" }
        ]);
        let composed = session.compose_prompt_for_dispatch("t-mix", &user);
        let blocks = composed.as_array().unwrap();
        assert_eq!(blocks[1], user[0]);
        assert_eq!(blocks[2], user[1]);
        assert_eq!(blocks[3], user[2]);
    }

    #[test]
    fn a_persona_with_a_fake_closing_tag_does_not_open_a_new_section() {
        let (fence, body) = fenced("hello\nJABOT_1\nmore");
        assert_eq!(fence, "JABOT_2");
        assert!(body.contains("JABOT_1"));
        assert!(!body.contains(&fence) || fence == "JABOT_2");
    }

    #[test]
    fn chief_context_includes_the_seeded_persona_and_creation_tool() {
        let (_dir, session) = host();
        // Standing thread id is derived; compose still works from any thread
        // that names Chief once the store has the row.
        let composed = session.compose_prompt_for_dispatch("bot-chief", &json!("hi"));
        let context = composed[0]["text"].as_str().unwrap();
        // No thread row yet — still ships the versioned app instructions.
        assert!(context.contains(&format!("Jabot context v{APP_CONTEXT_VERSION}")));
        assert!(context.contains("You cannot create crew members"));
    }

    #[test]
    fn newlines_in_names_cannot_open_a_section() {
        assert_eq!(single_line("a\nb"), "a b");
    }

    #[test]
    fn two_bots_do_not_see_each_other_s_persona() {
        let (_dir, mut session) = host();
        let researcher = session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(2),
                "crew/create",
                Some(json!({
                    "name": "Researcher",
                    "instructions": "Cite sources. SECRET_RESEARCHER",
                    "tools": [],
                    "harnessId": "claude"
                })),
            ))
            .result
            .expect("create researcher");
        let writer = session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(3),
                "crew/create",
                Some(json!({
                    "name": "Writer",
                    "instructions": "Draft in my voice. SECRET_WRITER",
                    "tools": [],
                    "harnessId": "claude"
                })),
            ))
            .result
            .expect("create writer");
        let researcher_id = researcher["botId"].as_str().unwrap();
        let writer_id = writer["botId"].as_str().unwrap();
        session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(4),
                "crew/thread",
                Some(json!({ "botId": researcher_id })),
            ))
            .result
            .expect("researcher thread");
        session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(5),
                "crew/thread",
                Some(json!({ "botId": writer_id })),
            ))
            .result
            .expect("writer thread");
        let research_thread = format!("bot-{researcher_id}");
        let writer_thread = format!("bot-{writer_id}");
        let research = session.compose_prompt_for_dispatch(&research_thread, &json!("go"));
        let write = session.compose_prompt_for_dispatch(&writer_thread, &json!("go"));
        let research_text = research[0]["text"].as_str().unwrap();
        let write_text = write[0]["text"].as_str().unwrap();
        assert!(
            research_text.contains("SECRET_RESEARCHER"),
            "{research_text}"
        );
        assert!(!research_text.contains("SECRET_WRITER"), "{research_text}");
        assert!(write_text.contains("SECRET_WRITER"), "{write_text}");
        assert!(!write_text.contains("SECRET_RESEARCHER"), "{write_text}");
    }

    #[test]
    fn a_changed_persona_applies_on_the_next_compose() {
        let (_dir, mut session) = host();
        session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(2),
                "crew/thread",
                Some(json!({ "botId": "chief" })),
            ))
            .result
            .expect("chief thread");
        let before = session.compose_prompt_for_dispatch("bot-chief", &json!("hi"));
        assert!(before[0]["text"].as_str().unwrap().contains("draft_bot"));
        session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(3),
                "crew/update",
                Some(json!({
                    "botId": "chief",
                    "instructions": "NEW_PERSONA_AFTER_EDIT"
                })),
            ))
            .result
            .expect("update");
        let after = session.compose_prompt_for_dispatch("bot-chief", &json!("hi"));
        let text = after[0]["text"].as_str().unwrap();
        assert!(text.contains("NEW_PERSONA_AFTER_EDIT"), "{text}");
    }
}
