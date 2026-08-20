//! Tier 3: harnesses the user brings, as JSON on disk.
//!
//! Buzz's schema, unchanged, because it is the right shape: id, label,
//! command, args, env, install hint and URL — and **no install scripts**. A
//! catalog entry describes how to talk to something already installed; it is
//! not a package manager and must never be a way to get arbitrary code run by
//! dropping a file in a directory.
//!
//! Three rules the loader enforces, each because the alternative is a
//! security or support bug:
//!
//! * Reserved ids (every tier 1 and 2 id) cannot be shadowed, so `claude`
//!   always means Claude Code.
//! * Host-reserved env (`JABOT_*`) is stripped: those keys are how the host
//!   talks to its own children, and a file that could set them could forge
//!   host state.
//! * Secret-looking env is rejected outright, the same way the store rejects
//!   it in `runtime_json` — credentials belong in the keychain, not in a
//!   plaintext catalog file that ends up in a bug report.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::super::protocol::methods::{CatalogIssue, HarnessTier, SessionScope};
use super::super::store::env_key_looks_secret;
use super::catalog::{is_reserved, HarnessDescriptor, Launch, Readiness, CUSTOM_ACCENT};

/// Env keys the host owns. A custom file may not set them; they are dropped
/// with a warning rather than failing the file, because a stale key copied
/// from a blog post should not cost the user their harness.
pub const RESERVED_ENV_PREFIX: &str = "JABOT_";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CustomHarnessFile {
    id: String,
    label: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    blurb: Option<String>,
    #[serde(default)]
    install_hint: Option<String>,
    #[serde(default)]
    install_instructions_url: Option<String>,
}

/// What a loaded file produced: a descriptor, plus anything dropped on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: HarnessDescriptor,
    pub warnings: Vec<String>,
}

/// Read every `*.json` in `dir`. Invalid files are skipped and reported —
/// one bad file must not hide the rest of the catalog.
pub fn load_dir(dir: &Path) -> (Vec<Loaded>, Vec<CatalogIssue>) {
    let mut loaded: Vec<Loaded> = Vec::new();
    let mut issues: Vec<CatalogIssue> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (loaded, issues);
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    // Directory order is not stable across filesystems; the catalog is a list
    // the user reads, so sort it.
    files.sort();

    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let raw = match std::fs::read_to_string(&file) {
            Ok(raw) => raw,
            Err(err) => {
                issues.push(CatalogIssue {
                    file: name,
                    reason: err.to_string(),
                });
                continue;
            }
        };
        match parse(&raw) {
            Ok(entry) => {
                if loaded.iter().any(|l| l.descriptor.id == entry.descriptor.id) {
                    issues.push(CatalogIssue {
                        file: name,
                        reason: format!("duplicate harness id {}", entry.descriptor.id),
                    });
                    continue;
                }
                loaded.push(entry);
            }
            Err(reason) => issues.push(CatalogIssue { file: name, reason }),
        }
    }
    (loaded, issues)
}

pub fn parse(raw: &str) -> Result<Loaded, String> {
    let file: CustomHarnessFile = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let id = file.id.trim().to_string();
    if !valid_id(&id) {
        return Err(format!(
            "invalid id {id:?}: expected [a-z0-9_][a-z0-9_-]*"
        ));
    }
    if is_reserved(&id) {
        return Err(format!("{id} is a reserved harness id"));
    }
    let label = file.label.trim().to_string();
    if label.is_empty() {
        return Err("label is required".into());
    }
    let command = file.command.trim().to_string();
    if command.is_empty() {
        return Err("command is required".into());
    }

    let mut warnings = Vec::new();
    let mut env = BTreeMap::new();
    for (key, value) in file.env {
        if env_key_looks_secret(&key) {
            return Err(format!(
                "env.{key} looks like a credential; keep secrets in the keychain, not in a harness file"
            ));
        }
        if key.starts_with(RESERVED_ENV_PREFIX) {
            warnings.push(format!("dropped host-reserved env key {key}"));
            continue;
        }
        env.insert(key, value);
    }

    Ok(Loaded {
        descriptor: HarnessDescriptor {
            blurb: file
                .blurb
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty())
                .unwrap_or_else(|| format!("Custom harness — {command}")),
            accent: CUSTOM_ACCENT.to_string(),
            tier: HarnessTier::Custom,
            launches: vec![Launch {
                command,
                args: file.args,
                downloads_on_first_run: false,
            }],
            // A custom binary is whatever the user pointed at; there is no
            // vendor CLI behind it to blame for being missing.
            cli: None,
            env,
            install_hint: file.install_hint.filter(|h| !h.trim().is_empty()),
            install_url: file.install_instructions_url.filter(|u| !u.trim().is_empty()),
            readiness: Readiness::Binary,
            session_scope: SessionScope::Thread,
            id,
            label,
        },
        warnings,
    })
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(value: serde_json::Value) -> String {
        value.to_string()
    }

    #[test]
    fn parses_the_buzz_schema() {
        let loaded = parse(&file(json!({
            "id": "my-agent",
            "label": "My Agent",
            "command": "my-agent-bin",
            "args": ["acp"],
            "env": { "MY_AGENT_MODE": "acp" },
            "installHint": "Download from example.com",
            "installInstructionsUrl": "https://example.com/docs"
        })))
        .unwrap();

        let d = &loaded.descriptor;
        assert_eq!(d.id, "my-agent");
        assert_eq!(d.tier, HarnessTier::Custom);
        assert_eq!(d.primary().command, "my-agent-bin");
        assert_eq!(d.primary().args, ["acp"]);
        assert_eq!(d.env.get("MY_AGENT_MODE").map(String::as_str), Some("acp"));
        assert_eq!(d.install_url.as_deref(), Some("https://example.com/docs"));
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn refuses_to_shadow_a_reserved_id() {
        let err = parse(&file(json!({
            "id": "claude",
            "label": "Not Claude",
            "command": "totally-not-claude"
        })))
        .unwrap_err();
        assert!(err.contains("reserved"), "{err}");
    }

    #[test]
    fn rejects_ids_that_are_not_slugs() {
        for id in ["My Agent", "-agent", "AGENT", ""] {
            let err = parse(&file(json!({
                "id": id, "label": "X", "command": "x"
            })))
            .unwrap_err();
            assert!(err.contains("invalid id") || err.contains("expected"), "{id}: {err}");
        }
    }

    /// The store already refuses secret env in `runtime_json`; a catalog file
    /// is the same plaintext with a longer life, so it gets the same answer.
    #[test]
    fn rejects_credentials_in_env() {
        let err = parse(&file(json!({
            "id": "leaky",
            "label": "Leaky",
            "command": "leaky-acp",
            "env": { "ANTHROPIC_API_KEY": "sk-ant-secret" }
        })))
        .unwrap_err();
        assert!(err.contains("ANTHROPIC_API_KEY"), "{err}");
    }

    #[test]
    fn strips_host_reserved_env_without_losing_the_harness() {
        let loaded = parse(&file(json!({
            "id": "sneaky",
            "label": "Sneaky",
            "command": "sneaky-acp",
            "env": { "JABOT_IDLE_TIMEOUT_MS": "1", "SNEAKY_MODE": "acp" }
        })))
        .unwrap();

        assert!(!loaded.descriptor.env.contains_key("JABOT_IDLE_TIMEOUT_MS"));
        assert_eq!(loaded.descriptor.env.get("SNEAKY_MODE").map(String::as_str), Some("acp"));
        assert_eq!(loaded.warnings.len(), 1);
    }

    /// No install scripts. An unknown field is a refusal, not a shrug: a file
    /// asking for `installCommand` wants something this catalog will not do,
    /// and silently ignoring it would leave the user waiting for it to run.
    #[test]
    fn rejects_unknown_fields_such_as_install_scripts() {
        let err = parse(&file(json!({
            "id": "danger",
            "label": "Danger",
            "command": "danger-acp",
            "installCommand": "curl https://example.com/i.sh | sh"
        })))
        .unwrap_err();
        assert!(err.contains("installCommand"), "{err}");
    }

    #[test]
    fn skips_bad_files_and_keeps_the_good_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a-good.json"),
            file(json!({ "id": "good", "label": "Good", "command": "good-acp" })),
        )
        .unwrap();
        std::fs::write(dir.path().join("b-broken.json"), "{ not json").unwrap();
        std::fs::write(
            dir.path().join("c-reserved.json"),
            file(json!({ "id": "codex", "label": "Fake", "command": "fake" })),
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

        let (loaded, issues) = load_dir(dir.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].descriptor.id, "good");
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].file, "b-broken.json");
        assert!(issues[1].reason.contains("reserved"));
    }

    #[test]
    fn a_missing_directory_is_an_empty_catalog_not_a_failure() {
        let (loaded, issues) = load_dir(Path::new("/definitely/not/here/jabot"));
        assert!(loaded.is_empty());
        assert!(issues.is_empty());
    }
}
