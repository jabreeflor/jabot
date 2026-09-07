//! Aider as a JaBot harness (#223).
//!
//! Aider does **not** speak ACP. Upstream ACP support is an unmerged
//! experiment (`Aider-AI/aider#4936`); this module is honest about that.
//! JaBot drives Aider's documented scripting CLI (`--message` / `--yes` /
//! `--no-auto-commits`, see https://aider.chat/docs/scripting.html) through
//! a JaBot-owned ACP adapter (`aider_acp.rs`).
//!
//! Doctor classification is richer than a single readiness command: missing
//! binary, unsupported version, missing API key, and missing model each have
//! a different fix.

use std::path::PathBuf;

use super::super::protocol::methods::HarnessStatus;
use super::catalog::{HarnessDescriptor, Launch};
use super::doctor::{Diagnosis, ProbeHost, ProbeRun};

/// Floor under which the scripting flags this adapter relies on are not
/// treated as present. `--message`, `--yes`, and `--no-auto-commits` landed
/// early; 0.35 is the documented scripting surface we verified against.
pub const MIN_VERSION: (u32, u32, u32) = (0, 35, 0);

/// Env floor snapshotted onto every Aider thread. The adapter *also* passes
/// the matching CLI flags, so a user who exported `AIDER_AUTO_COMMITS=true`
/// still cannot make Aider commit inside a JaBot-managed worktree.
pub const ENV_FLOOR: &[(&str, &str)] = &[
    ("AIDER_AUTO_COMMITS", "false"),
    ("AIDER_DIRTY_COMMITS", "false"),
    ("AIDER_YES", "true"),
];

/// Provider keys Aider will accept from the process environment. JaBot does
/// not store these itself today; it inherits the host environment and any
/// account-profile export that lands in these names.
pub const AUTH_ENV_KEYS: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "OPENROUTER_API_KEY",
    "DEEPSEEK_API_KEY",
    "GROQ_API_KEY",
    "COHERE_API_KEY",
    "TOGETHER_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "MISTRAL_API_KEY",
    "AIDER_API_KEY",
];

const MODEL_ENV_KEYS: &[&str] = &["AIDER_MODEL"];

const CONFIG_AUTH_KEYS: &[&str] = &[
    "openai-api-key",
    "anthropic-api-key",
    "gemini-api-key",
    "openrouter-api-key",
    "api-key",
    "openai_api_key",
    "anthropic_api_key",
];

const CONFIG_MODEL_KEYS: &[&str] = &["model", "weak-model", "editor-model"];

/// What the Doctor needs from the machine beyond "is this binary here?".
pub trait AiderFacts: Sync {
    fn env(&self, key: &str) -> Option<String>;
    fn home_config(&self) -> Option<String>;
}

/// The real process: inherited env plus `~/.aider.conf.yml` / `~/.env`.
pub struct SystemFacts;

impl AiderFacts for SystemFacts {
    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }

    fn home_config(&self) -> Option<String> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        for name in [".aider.conf.yml", ".aider.conf.yaml", ".aider.conf.json"] {
            let path = home.join(name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                return Some(text);
            }
        }
        let env_path = home.join(".env");
        std::fs::read_to_string(env_path).ok()
    }
}

/// After the generic Doctor has found `aider` on PATH, classify version,
/// auth, and model. `None` means those are fine and the generic adapter /
/// readiness path should continue.
pub fn classify(
    descriptor: &HarnessDescriptor,
    probe: &dyn ProbeHost,
    facts: &dyn AiderFacts,
    launch: Option<&Launch>,
    resolved: Option<&PathBuf>,
    started: std::time::Instant,
) -> Option<Diagnosis> {
    let finish = |status: HarnessStatus, detail: String, remedy: Option<String>| Diagnosis {
        id: descriptor.id.clone(),
        status,
        detail,
        remedy,
        launch: launch.cloned(),
        resolved_path: resolved.cloned(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    };

    match probe.output("aider", &["--version".to_string()]) {
        Ok(text) => match parse_version(&text) {
            Some(version) if version_less(version, MIN_VERSION) => {
                return Some(finish(
                    HarnessStatus::InvalidConfig,
                    format!(
                        "Aider {} is too old; JaBot's scripting adapter needs {}.{}.{}+ (`--message`, `--yes`, `--no-auto-commits`).",
                        format_version(version),
                        MIN_VERSION.0,
                        MIN_VERSION.1,
                        MIN_VERSION.2
                    ),
                    Some("Upgrade Aider: `python -m pip install -U aider-chat`.".into()),
                ));
            }
            Some(_) => {}
            None => {
                return Some(finish(
                    HarnessStatus::Unknown,
                    format!("`aider --version` did not print a version ({text:?})."),
                    Some("Reinstall Aider and run `aider --version`.".into()),
                ));
            }
        },
        Err(ProbeRun::TimedOut) => {
            return Some(finish(
                HarnessStatus::Unknown,
                "`aider --version` did not answer in time.".into(),
                Some("Check that `aider` is not blocked on a network or TTY prompt.".into()),
            ));
        }
        Err(ProbeRun::Failed(err)) => {
            return Some(finish(
                HarnessStatus::Unknown,
                format!("could not run `aider --version`: {err}"),
                descriptor.install_hint.clone(),
            ));
        }
        Err(ProbeRun::Exit(code)) => {
            return Some(finish(
                HarnessStatus::Unknown,
                format!("`aider --version` exited {code}."),
                Some("Reinstall Aider: `python -m pip install -U aider-chat`.".into()),
            ));
        }
    }

    let config = facts.home_config().unwrap_or_default();
    let has_key = AUTH_ENV_KEYS.iter().any(|key| facts.env(key).is_some())
        || config_has_any(&config, CONFIG_AUTH_KEYS)
        || env_file_has_any(&config, AUTH_ENV_KEYS);
    let has_model = MODEL_ENV_KEYS.iter().any(|key| facts.env(key).is_some())
        || config_has_any(&config, CONFIG_MODEL_KEYS)
        || env_file_has_any(&config, MODEL_ENV_KEYS);

    if !has_key {
        return Some(finish(
            HarnessStatus::LoggedOut,
            "Aider has no API key in the environment or ~/.aider.conf.yml.".into(),
            Some(
                "Export a provider key (OPENAI_API_KEY, ANTHROPIC_API_KEY, OPENROUTER_API_KEY, …) or add it to ~/.aider.conf.yml. JaBot inherits the host environment; Aider's own config is user-global and is not isolated per bot."
                    .into(),
            ),
        ));
    }

    if !has_model && !has_defaultable_provider(facts, &config) {
        return Some(finish(
            HarnessStatus::InvalidConfig,
            "Aider has credentials but no model is configured.".into(),
            Some("Set AIDER_MODEL or add `model: <name>` to ~/.aider.conf.yml.".into()),
        ));
    }

    None
}

fn has_defaultable_provider(facts: &dyn AiderFacts, config: &str) -> bool {
    // Aider picks a default model when a well-known provider key is present.
    [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENROUTER_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ]
    .iter()
    .any(|key| facts.env(key).is_some())
        || config_has_any(
            config,
            &[
                "openai-api-key",
                "anthropic-api-key",
                "openrouter-api-key",
                "gemini-api-key",
            ],
        )
}

fn config_has_any(text: &str, keys: &[&str]) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        keys.iter().any(|key| {
            line.starts_with(key)
                && line
                    .get(key.len()..)
                    .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('='))
                && value_present(line)
        })
    })
}

fn env_file_has_any(text: &str, keys: &[&str]) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        keys.iter().any(|key| {
            line.starts_with(key)
                && line
                    .get(key.len()..)
                    .is_some_and(|rest| rest.starts_with('='))
                && value_present(line)
        })
    })
}

fn value_present(line: &str) -> bool {
    line.split_once([':', '='])
        .map(|(_, value)| {
            let value = value.trim().trim_matches(['"', '\'']);
            !value.is_empty() && value != "null" && value != "~"
        })
        .unwrap_or(false)
}

pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let rest = &text[i..];
            let mut parts = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty());
            if let (Some(major), Some(minor)) = (parts.next(), parts.next()) {
                let major = major.parse().ok()?;
                let minor = minor.parse().ok()?;
                let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                return Some((major, minor, patch));
            }
        }
        i += 1;
    }
    None
}

fn version_less(left: (u32, u32, u32), right: (u32, u32, u32)) -> bool {
    left < right
}

fn format_version((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

/// CLI flags the adapter always passes. Kept next to the Doctor so a test
/// can see the worktree policy in one place.
pub fn scripting_args(message_file: &str, history_file: &str, files: &[String]) -> Vec<String> {
    let mut args = vec![
        "--message-file".into(),
        message_file.into(),
        "--yes".into(),
        "--no-auto-commits".into(),
        "--no-dirty-commits".into(),
        "--no-pretty".into(),
        "--no-fancy-input".into(),
        "--no-check-update".into(),
        "--no-show-model-warnings".into(),
        "--chat-history-file".into(),
        history_file.into(),
        "--restore-chat-history".into(),
        "--stream".into(),
    ];
    args.extend(files.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::harness::catalog::compiled_in;
    use std::collections::HashMap;
    use std::time::Instant;

    struct Fake {
        installed: HashMap<String, PathBuf>,
        output: Result<String, ProbeRun>,
        env: HashMap<String, String>,
        config: Option<String>,
    }

    impl Fake {
        fn ready() -> Self {
            Self {
                installed: HashMap::from([
                    ("aider".into(), PathBuf::from("/opt/bin/aider")),
                    (
                        "jabot-aider-acp".into(),
                        PathBuf::from("/opt/bin/jabot-aider-acp"),
                    ),
                ]),
                output: Ok("aider 0.82.2".into()),
                env: HashMap::from([("OPENAI_API_KEY".into(), "sk-test".into())]),
                config: None,
            }
        }
    }

    impl ProbeHost for Fake {
        fn resolve(&self, command: &str) -> Option<PathBuf> {
            self.installed.get(command).cloned()
        }
        fn run(&self, command: &str, args: &[String]) -> ProbeRun {
            match self.output(command, args) {
                Ok(_) => ProbeRun::Exit(0),
                Err(run) => run,
            }
        }
        fn listening(&self, _: &str) -> bool {
            false
        }
        fn output(&self, command: &str, _: &[String]) -> Result<String, ProbeRun> {
            if !self.installed.contains_key(command) {
                return Err(ProbeRun::Failed(format!("{command} is not on PATH")));
            }
            self.output.clone()
        }
    }

    impl AiderFacts for Fake {
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn home_config(&self) -> Option<String> {
            self.config.clone()
        }
    }

    fn card() -> HarnessDescriptor {
        compiled_in()
            .into_iter()
            .find(|d| d.id == "aider")
            .expect("aider is compiled in")
    }

    fn ask(fake: &Fake) -> Option<Diagnosis> {
        let card = card();
        classify(
            &card,
            fake,
            fake,
            Some(card.primary()),
            None,
            Instant::now(),
        )
    }

    #[test]
    fn a_current_aider_with_a_key_is_not_a_doctor_failure() {
        assert!(ask(&Fake::ready()).is_none());
    }

    #[test]
    fn an_old_aider_is_invalid_config() {
        let fake = Fake {
            output: Ok("aider 0.20.0".into()),
            ..Fake::ready()
        };
        let report = ask(&fake).expect("old");
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.detail.contains("0.20.0"), "{}", report.detail);
        assert!(report.remedy.unwrap().contains("pip"));
    }

    #[test]
    fn no_api_key_is_logged_out() {
        let fake = Fake {
            env: HashMap::new(),
            config: None,
            ..Fake::ready()
        };
        let report = ask(&fake).expect("logged out");
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn a_config_file_key_counts_as_signed_in() {
        let fake = Fake {
            env: HashMap::new(),
            config: Some("openai-api-key: sk-from-file\nmodel: gpt-4o\n".into()),
            ..Fake::ready()
        };
        assert!(ask(&fake).is_none());
    }

    #[test]
    fn a_key_without_a_defaultable_provider_and_no_model_is_invalid_config() {
        let fake = Fake {
            env: HashMap::from([("GROQ_API_KEY".into(), "gsk-test".into())]),
            config: None,
            ..Fake::ready()
        };
        let report = ask(&fake).expect("model");
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.detail.contains("model"), "{}", report.detail);
    }

    #[test]
    fn parse_version_reads_the_forms_aider_prints() {
        assert_eq!(parse_version("aider 0.82.2"), Some((0, 82, 2)));
        assert_eq!(parse_version("0.35.0"), Some((0, 35, 0)));
        assert_eq!(parse_version("aider-chat, version 1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("not a version"), None);
    }

    #[test]
    fn scripting_args_disable_commits_and_restore_history() {
        let args = scripting_args("/tmp/msg", "/tmp/hist", &["src/lib.rs".into()]);
        assert!(args.contains(&"--no-auto-commits".into()));
        assert!(args.contains(&"--no-dirty-commits".into()));
        assert!(args.contains(&"--yes".into()));
        assert!(args.contains(&"--restore-chat-history".into()));
        assert!(args.contains(&"src/lib.rs".into()));
        assert!(!args.iter().any(|a| a == "--auto-commits"));
    }

    #[test]
    fn the_catalog_floor_matches_the_adapter_policy() {
        let env: Vec<_> = ENV_FLOOR.iter().copied().collect();
        assert!(env.contains(&("AIDER_AUTO_COMMITS", "false")));
        assert!(env.contains(&("AIDER_DIRTY_COMMITS", "false")));
        assert!(env.contains(&("AIDER_YES", "true")));
    }
}
