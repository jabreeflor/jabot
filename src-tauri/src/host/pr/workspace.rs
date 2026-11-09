//! In-app PR reads and explicit user actions through the existing gh credential.
use super::super::{protocol::error::RpcError, repo::exec};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    repo: String,
    number: u64,
    #[serde(default = "default_host")]
    host: String,
    action: Option<String>,
    #[serde(default)]
    body: String,
    sha: Option<String>,
    method: Option<String>,
    path: Option<String>,
    line: Option<u64>,
    side: Option<String>,
    title: Option<String>,
    reviewers: Option<Vec<String>>,
}
fn default_host() -> String {
    "github.com".into()
}
fn invalid(message: &str) -> RpcError {
    RpcError::InvalidParams(message.into())
}
fn validate(p: &Params) -> Result<(), RpcError> {
    let parts: Vec<_> = p.repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
        || p.number == 0
    {
        return Err(invalid(
            "Expected owner/repository and a positive PR number",
        ));
    }
    if p.host.is_empty()
        || p.host.starts_with('-')
        || !p
            .host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    {
        return Err(invalid("Invalid GitHub hostname"));
    }
    if p.body.len() > 60000 {
        return Err(invalid("Comment is too long"));
    }
    Ok(())
}
fn api(
    p: &Params,
    endpoint: &str,
    method: &str,
    body: Option<Value>,
    pages: bool,
) -> Result<Value, RpcError> {
    let mut args = vec!["api", "--hostname", &p.host, "--method", method, endpoint];
    if pages {
        args.extend(["--paginate", "--slurp"]);
    }
    let input = body.map(|v| v.to_string());
    if input.is_some() {
        args.extend(["--input", "-"]);
    }
    let mut spec = exec::Spawn::new("gh", &args, Duration::from_secs(30));
    if let Some(ref input) = input {
        spec = spec.with_stdin(input);
    }
    let result = exec::spawn(spec)
        .map_err(|e| RpcError::Internal(format!("GitHub request failed: {e:?}")))?;
    if !result.ok() {
        return Err(RpcError::Internal(result.stderr.trim().into()));
    }
    let value: Value =
        serde_json::from_str(&result.stdout).map_err(|e| RpcError::Internal(e.to_string()))?;
    if pages {
        Ok(Value::Array(
            value
                .as_array()
                .ok_or_else(|| invalid("Invalid page response"))?
                .iter()
                .flat_map(|page| page.as_array().into_iter().flatten().cloned())
                .collect(),
        ))
    } else {
        Ok(value)
    }
}
pub fn dispatch(method: &str, p: Params) -> Result<Value, RpcError> {
    dispatch_with(method, p, api)
}

fn dispatch_with(
    method: &str,
    p: Params,
    request: impl Fn(&Params, &str, &str, Option<Value>, bool) -> Result<Value, RpcError> + Sync,
) -> Result<Value, RpcError> {
    validate(&p)?;
    let root = format!("repos/{}", p.repo);
    let pull = format!("{root}/pulls/{}", p.number);
    let issue = format!("{root}/issues/{}", p.number);
    if method == "pr/detail" {
        return detail(&p, &root, &pull, &issue, &request);
    }
    let action = p
        .action
        .as_deref()
        .ok_or_else(|| invalid("Missing action"))?;
    if matches!(action, "comment" | "COMMENT" | "REQUEST_CHANGES" | "inline")
        && p.body.trim().is_empty()
    {
        return Err(invalid("Write a comment first"));
    }
    match action {
        "edit" => {
            let title = p
                .title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("Title is required"))?;
            request(
                &p,
                &pull,
                "PATCH",
                Some(json!({"title":title,"body":p.body})),
                false,
            )
        }
        "reviewers" => {
            let reviewers = p
                .reviewers
                .as_ref()
                .filter(|r| !r.is_empty())
                .ok_or_else(|| invalid("Enter at least one reviewer"))?;
            if reviewers
                .iter()
                .any(|r| r.is_empty() || !r.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
            {
                return Err(invalid("Invalid GitHub username"));
            }
            request(
                &p,
                &format!("{pull}/requested_reviewers"),
                "POST",
                Some(json!({"reviewers":reviewers})),
                false,
            )
        }
        "ready" | "draft" => {
            let current = request(&p, &pull, "GET", None, false)?;
            let id = current["node_id"]
                .as_str()
                .ok_or_else(|| invalid("Missing pull request ID"))?;
            let mutation = if action == "ready" {
                "markPullRequestReadyForReview"
            } else {
                "convertPullRequestToDraft"
            };
            let query = format!("mutation($id:ID!) {{ {mutation}(input:{{pullRequestId:$id}}) {{ pullRequest {{ isDraft }} }} }}");
            let result = request(
                &p,
                "graphql",
                "POST",
                Some(json!({"query":query,"variables":{"id":id}})),
                false,
            )?;
            if let Some(errors) = result.get("errors") {
                return Err(RpcError::Internal(errors.to_string()));
            }
            Ok(result)
        }
        "comment" => request(
            &p,
            &format!("{issue}/comments"),
            "POST",
            Some(json!({"body":p.body})),
            false,
        ),
        "APPROVE" | "REQUEST_CHANGES" | "COMMENT" | "inline" | "merge" => {
            let sha = p
                .sha
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("Refresh the PR before continuing"))?;
            let current = request(&p, &pull, "GET", None, false)?;
            if current["head"]["sha"].as_str() != Some(sha) {
                return Err(invalid(
                    "New commits were pushed. Refresh and review them before continuing.",
                ));
            }
            if action == "merge" {
                let strategy = p.method.as_deref().unwrap_or("squash");
                if !matches!(strategy, "merge" | "squash" | "rebase") {
                    return Err(invalid("Invalid merge method"));
                }
                let result = request(
                    &p,
                    &format!("{pull}/merge"),
                    "PUT",
                    Some(json!({"sha":sha,"merge_method":strategy})),
                    false,
                )?;
                if result["merged"] != true {
                    return Err(RpcError::Internal(
                        result["message"]
                            .as_str()
                            .unwrap_or("Merge was not completed")
                            .into(),
                    ));
                }
                Ok(result)
            } else if action == "inline" {
                let path = p
                    .path
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| invalid("Missing file path"))?;
                let line = p
                    .line
                    .filter(|n| *n > 0)
                    .ok_or_else(|| invalid("Missing diff line"))?;
                let side = p.side.as_deref().unwrap_or("RIGHT");
                if !matches!(side, "LEFT" | "RIGHT") {
                    return Err(invalid("Invalid diff side"));
                }
                request(
                    &p,
                    &format!("{pull}/comments"),
                    "POST",
                    Some(
                        json!({"body":p.body,"commit_id":sha,"path":path,"line":line,"side":side}),
                    ),
                    false,
                )
            } else {
                request(
                    &p,
                    &format!("{pull}/reviews"),
                    "POST",
                    Some(json!({"body":p.body,"event":action,"commit_id":sha})),
                    false,
                )
            }
        }
        "close" | "reopen" => request(
            &p,
            &pull,
            "PATCH",
            Some(json!({"state":if action == "close" {"closed"} else {"open"}})),
            false,
        ),
        _ => Err(invalid("Unsupported PR action")),
    }
}

/// Everything the PR page shows, in two network round trips instead of eight.
///
/// Each read is its own `gh api` process and its own HTTPS round trip, and
/// run one after another they made opening a PR take as long as all eight
/// added together (about six seconds against github.com). None of the lists
/// depends on another, so they all go at once; only the checks wait, because
/// they are addressed by the head commit the PR read returns. They start the
/// moment that read lands, while the slower paginated lists are still running.
fn detail(
    p: &Params,
    root: &str,
    pull: &str,
    issue: &str,
    request: &(impl Fn(&Params, &str, &str, Option<Value>, bool) -> Result<Value, RpcError> + Sync),
) -> Result<Value, RpcError> {
    let list = |path: String| move || request(p, &path, "GET", None, true);
    std::thread::scope(|scope| {
        let comments = scope.spawn(list(format!("{issue}/comments?per_page=100")));
        let reviews = scope.spawn(list(format!("{pull}/reviews?per_page=100")));
        let files = scope.spawn(list(format!("{pull}/files?per_page=100")));
        let inline = scope.spawn(list(format!("{pull}/comments?per_page=100")));
        let commits = scope.spawn(list(format!("{pull}/commits?per_page=100")));
        let pr = request(p, pull, "GET", None, false)?;
        let sha = pr["head"]["sha"]
            .as_str()
            .ok_or_else(|| invalid("Missing head commit"))?;
        // Optional checks may be inaccessible to tokens that can still review code.
        let check_path = format!("{root}/commits/{sha}/check-runs?per_page=100");
        let checks = scope.spawn(move || request(p, &check_path, "GET", None, false));
        let statuses = request(
            p,
            &format!("{root}/commits/{sha}/status"),
            "GET",
            None,
            false,
        );
        let join = |handle: std::thread::ScopedJoinHandle<'_, Result<Value, RpcError>>| {
            handle
                .join()
                .unwrap_or_else(|_| Err(RpcError::Internal("GitHub request panicked".into())))
        };
        let checks = join(checks);
        let (comments, reviews, files, inline, commits) = (
            join(comments)?,
            join(reviews)?,
            join(files)?,
            join(inline)?,
            join(commits)?,
        );
        Ok(json!({
            "pr": pr,
            "comments": comments,
            "reviews": reviews,
            "files": files,
            "inline": inline,
            "commits": commits,
            "checks": checks.as_ref().ok(),
            "statuses": statuses.as_ref().ok(),
            "checksError": checks.err().or_else(|| statuses.err()).map(|e| e.to_string()),
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_untrusted_routes_before_spawning() {
        for repo in ["../repo", "o/r/extra", "o/r?x=y", "-o/", "o/@file"] {
            let p: Params = serde_json::from_value(json!({"repo":repo,"number":1})).unwrap();
            assert!(validate(&p).is_err());
        }
    }
    #[test]
    fn stale_head_never_writes() {
        let p =
            serde_json::from_value(json!({"repo":"o/r","number":1,"action":"merge","sha":"old"}))
                .unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = dispatch_with("pr/action", p, |_, _, method, _, _| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(method, "GET");
            Ok(json!({"head":{"sha":"new"}}))
        });
        assert!(result.is_err());
        assert_eq!(calls.into_inner(), 1);
    }
    #[test]
    fn merge_is_pinned_and_does_not_force_protections() {
        let p = serde_json::from_value(
            json!({"repo":"o/r","number":1,"action":"merge","sha":"head","method":"rebase"}),
        )
        .unwrap();
        let result = dispatch_with("pr/action", p, |_, path, method, body, _| {
            if method == "GET" {
                return Ok(json!({"head":{"sha":"head"}}));
            }
            assert_eq!(path, "repos/o/r/pulls/1/merge");
            assert_eq!(method, "PUT");
            assert_eq!(body, Some(json!({"sha":"head","merge_method":"rebase"})));
            Ok(json!({"merged":true}))
        });
        assert!(result.is_ok());
    }
    #[test]
    fn review_keeps_body_as_json_and_uses_displayed_commit() {
        let p = serde_json::from_value(json!({"repo":"o/r","number":1,"action":"REQUEST_CHANGES","sha":"head","body":"@file $(ignored)"})).unwrap();
        let result = dispatch_with("pr/action", p, |_, path, method, body, _| {
            if method == "GET" {
                return Ok(json!({"head":{"sha":"head"}}));
            }
            assert_eq!(path, "repos/o/r/pulls/1/reviews");
            assert_eq!(
                body,
                Some(
                    json!({"event":"REQUEST_CHANGES","commit_id":"head","body":"@file $(ignored)"})
                )
            );
            Ok(json!({"id":1}))
        });
        assert!(result.is_ok());
    }
    #[test]
    fn blank_comment_never_reaches_github() {
        let p =
            serde_json::from_value(json!({"repo":"o/r","number":1,"action":"comment","body":"  "}))
                .unwrap();
        assert!(dispatch_with("pr/action", p, |_, _, _, _, _| panic!(
            "must not call GitHub"
        ))
        .is_err());
    }
    #[test]
    fn detail_reads_everything_at_once_and_pins_checks_to_the_head() {
        let p = serde_json::from_value(json!({"repo":"o/r","number":7})).unwrap();
        let seen = std::sync::Mutex::new(Vec::new());
        let result = dispatch_with("pr/detail", p, |_, path, method, body, pages| {
            assert_eq!(method, "GET");
            assert!(body.is_none());
            seen.lock().unwrap().push(path.to_string());
            Ok(match path {
                "repos/o/r/pulls/7" => json!({"head":{"sha":"abc"}}),
                "repos/o/r/commits/abc/check-runs?per_page=100" => json!({"check_runs":[]}),
                "repos/o/r/commits/abc/status" => {
                    return Err(RpcError::Internal("no access".into()))
                }
                _ => {
                    assert!(pages, "{path} is a list and must paginate");
                    json!([{"path": path}])
                }
            })
        })
        .unwrap();
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(seen.len(), 8);
        assert_eq!(result["pr"]["head"]["sha"], "abc");
        assert_eq!(
            result["files"][0]["path"],
            "repos/o/r/pulls/7/files?per_page=100"
        );
        assert_eq!(result["checks"], json!({"check_runs":[]}));
        assert!(result["statuses"].is_null());
        assert!(result["checksError"]
            .as_str()
            .unwrap()
            .contains("no access"));
    }
    #[test]
    fn detail_fails_when_a_list_fails() {
        let p = serde_json::from_value(json!({"repo":"o/r","number":7})).unwrap();
        let result = dispatch_with("pr/detail", p, |_, path, _, _, _| {
            if path.contains("/reviews") {
                return Err(RpcError::Internal("boom".into()));
            }
            Ok(json!({"head":{"sha":"abc"}}))
        });
        assert!(result.is_err());
    }
    #[test]
    fn accepts_repository_names() {
        let p = serde_json::from_value(json!({"repo":"my-org/my.repo_name","number":42})).unwrap();
        assert!(validate(&p).is_ok());
    }
}
