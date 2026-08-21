//! Turning an allowlist into the `mcpServers` array on ACP `session/new`.
//!
//! This is where per-bot enforcement actually happens. The bot's `tools[]` is
//! not a filter applied to results — a schema the model can see is a schema
//! the model will try, and a refusal after the call is a refusal after the
//! side effect. A tool a bot is not allowed to use is simply not in the array,
//! so the agent never learns it exists.
//!
//! The other rule this file keeps: **short-lived bearer, never the refresh
//! token**. ACP params can be logged by the adapter, so what crosses that wire
//! is an access token that expires, and the refresh token stays in the vault
//! (`docs/research/bot-crew/mcp-and-tools.md`).

use std::path::PathBuf;

use serde_json::{json, Value};

use super::catalog::{ToolEntry, Transport};

/// Why an allowlisted tool did not become a server. Surfaced rather than
/// swallowed: "Gmail did nothing" and "Gmail is not connected" are different
/// problems, and only one of them is the model's fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub tool_id: String,
    pub reason: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct McpPlan {
    pub servers: Vec<Value>,
    pub skipped: Vec<Skipped>,
}

impl McpPlan {
    pub fn as_params(&self) -> Value {
        Value::Array(self.servers.clone())
    }
}

/// What the host can offer for one entry right now.
pub enum Credential<'a> {
    /// A live `Authorization` header value.
    Bearer(&'a str),
    /// The entry needs a grant and there is none usable.
    Missing(String),
    /// The entry needs no credential at all.
    None,
}

/// Build the array, in catalog order, from entries the bot is allowed to use.
///
/// `resolve` turns a local MCP command into an absolute path on the augmented
/// PATH; a `None` means the tool is not installed on this machine, which is a
/// skip with a reason rather than a spawn that fails inside the adapter.
///
/// `profile` answers where an entry's JaBot-owned state goes, and is allowed
/// to refuse. A `--user-data-dir` is a lock rather than a setting, so who may
/// hold it depends on which other threads are live — something only the host
/// knows, which is why the answer is passed in instead of derived here.
pub fn plan<'a>(
    entries: impl IntoIterator<Item = &'a ToolEntry>,
    credential: impl Fn(&ToolEntry) -> Credential<'a>,
    profile: impl Fn(&ToolEntry) -> Result<PathBuf, String>,
    resolve: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> McpPlan {
    let mut plan = McpPlan::default();
    for entry in entries {
        match build(entry, &credential(entry), &profile, &resolve) {
            Ok(Some(server)) => plan.servers.push(server),
            Ok(None) => {}
            Err(reason) => plan.skipped.push(Skipped {
                tool_id: entry.id.to_string(),
                reason,
            }),
        }
    }
    plan
}

/// `Ok(None)` for an entry that is not a server at all (Terminal).
fn build(
    entry: &ToolEntry,
    credential: &Credential<'_>,
    profile: &impl Fn(&ToolEntry) -> Result<PathBuf, String>,
    resolve: &impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Result<Option<Value>, String> {
    match &entry.transport {
        Transport::HarnessExecute => Ok(None),
        Transport::Http { url } => match credential {
            Credential::Bearer(header) => Ok(Some(json!({
                "type": "http",
                "name": entry.id,
                "url": url,
                "headers": [{ "name": "Authorization", "value": header }],
            }))),
            Credential::Missing(reason) => Err(reason.clone()),
            Credential::None => Err(format!("{} needs a connection", entry.label)),
        },
        Transport::Stdio {
            command,
            args,
            profile_flag,
        } => {
            let resolved = resolve(command).ok_or_else(|| {
                format!(
                    "{command} is not installed, so {} cannot start",
                    entry.label
                )
            })?;
            let mut args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
            if let Some(flag) = profile_flag {
                let dir = profile(entry)?;
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
                args.push((*flag).to_string());
                args.push(dir.to_string_lossy().into_owned());
            }
            Ok(Some(json!({
                "name": entry.id,
                "command": resolved.to_string_lossy(),
                "args": args,
                "env": [],
            })))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::catalog::{self, CATALOG};
    use super::*;

    fn entries(ids: &[&str]) -> Vec<&'static ToolEntry> {
        CATALOG.iter().filter(|e| ids.contains(&e.id)).collect()
    }

    fn names(plan: &McpPlan) -> Vec<String> {
        plan.servers
            .iter()
            .filter_map(|server| server.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn always_resolve(command: &str) -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from("/usr/local/bin").join(command))
    }

    /// A host with a profile directory to give, in a directory the test owns.
    fn profiles_in(root: &Path) -> impl Fn(&ToolEntry) -> Result<PathBuf, String> + '_ {
        move |entry| Ok(root.join(entry.id))
    }

    fn no_profile(entry: &ToolEntry) -> Result<PathBuf, String> {
        Err(format!("{} has nowhere to keep a profile", entry.label))
    }

    /// The enforcement claim, stated as a test: an allowlist of one produces
    /// an array of one, and the tools left out are absent — not present and
    /// filtered later.
    #[test]
    fn only_allowlisted_tools_reach_the_agent() {
        let plan = plan(
            entries(&["gmail"]),
            |_| Credential::Bearer("Bearer live-access-token"),
            no_profile,
            always_resolve,
        );
        assert_eq!(names(&plan), vec!["gmail".to_string()]);

        let rendered = plan.as_params().to_string();
        for other in CATALOG.iter().filter(|e| e.id != "gmail") {
            assert!(
                !rendered.contains(&format!("\"name\":\"{}\"", other.id)),
                "{} leaked into a Gmail-only session",
                other.id
            );
        }
    }

    /// Terminal is the harness's `execute`. Allowlisting it must not put a
    /// server in the array — there is no server, and inventing one would put a
    /// shell behind a tool schema.
    #[test]
    fn terminal_never_becomes_a_server() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan(
            entries(&["terminal", "browser"]),
            |_| Credential::None,
            profiles_in(dir.path()),
            always_resolve,
        );
        assert_eq!(names(&plan), vec!["browser".to_string()]);
        assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    }

    /// An unconnected remote tool is left out with a reason, not passed with
    /// an empty Authorization header.
    #[test]
    fn a_tool_without_a_grant_is_left_out_with_a_reason() {
        let plan = plan(
            entries(&["gmail", "notion"]),
            |entry| {
                if entry.id == "gmail" {
                    Credential::Bearer("Bearer live")
                } else {
                    Credential::Missing("Notion is not connected".into())
                }
            },
            no_profile,
            always_resolve,
        );
        assert_eq!(names(&plan), vec!["gmail".to_string()]);
        assert_eq!(
            plan.skipped,
            vec![Skipped {
                tool_id: "notion".into(),
                reason: "Notion is not connected".into()
            }]
        );
    }

    /// The bearer crosses the ACP wire; the refresh token must not. This is
    /// the shape assertion that keeps it that way.
    #[test]
    fn only_the_access_token_crosses_the_wire() {
        let plan = plan(
            entries(&["gmail"]),
            |_| Credential::Bearer("Bearer ya29.access-only"),
            no_profile,
            always_resolve,
        );
        let rendered = plan.as_params().to_string();
        assert!(rendered.contains("ya29.access-only"));
        assert!(!rendered.to_lowercase().contains("refresh"));
        assert_eq!(plan.servers[0]["type"], "http");
        assert_eq!(plan.servers[0]["headers"][0]["name"], "Authorization");
    }

    #[test]
    fn a_local_server_gets_a_jabot_owned_profile_directory() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan(
            entries(&["browser"]),
            |_| Credential::None,
            profiles_in(dir.path()),
            always_resolve,
        );
        let server = &plan.servers[0];
        assert_eq!(server["name"], "browser");
        assert!(server["command"].as_str().unwrap().ends_with("npx"));
        let args: Vec<String> = serde_json::from_value(server["args"].clone()).unwrap();
        let flag = args
            .iter()
            .position(|a| a == "--user-data-dir")
            .expect("profile flag");
        assert_eq!(args[flag + 1], dir.path().join("browser").to_string_lossy());
        assert!(
            dir.path().join("browser").is_dir(),
            "profile dir is created up front"
        );
    }

    /// A profile the host will not hand out is the same class of answer as a
    /// command that is not installed: no server, and a reason for the log.
    #[test]
    fn a_profile_the_host_refuses_is_a_skip_with_its_reason() {
        let plan = plan(
            entries(&["browser"]),
            |_| Credential::None,
            |_| Err("Browser is in use by another thread".to_string()),
            always_resolve,
        );
        assert!(plan.servers.is_empty());
        assert_eq!(
            plan.skipped,
            vec![Skipped {
                tool_id: "browser".into(),
                reason: "Browser is in use by another thread".into()
            }]
        );
    }

    #[test]
    fn a_missing_local_command_is_a_skip_not_a_broken_session() {
        let plan = plan(
            entries(&["browser"]),
            |_| Credential::None,
            no_profile,
            |_| None,
        );
        assert!(plan.servers.is_empty());
        assert!(
            plan.skipped[0].reason.contains("not installed"),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn an_unknown_allowlist_entry_is_simply_not_a_tool() {
        assert!(catalog::find("everything").is_none());
        let plan = plan(
            entries(&["nope"]),
            |_| Credential::None,
            no_profile,
            always_resolve,
        );
        assert!(plan.servers.is_empty());
    }
}
