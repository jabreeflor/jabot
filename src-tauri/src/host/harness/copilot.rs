//! GitHub Copilot CLI readiness (#221).
//!
//! Copilot is unusual in this catalog: the vendor CLI *is* the ACP adapter
//! (`copilot --acp`). There is no second package to install, and a binary
//! being on PATH is not enough — older builds predate ACP, a missing login is
//! a different fix from an org policy that blocks the CLI, and an empty
//! `model` in settings is a different fix from "use the default".
//!
//! Classification is a pure function of facts the Doctor gathered, so the
//! rules can be tested without installing Copilot.

use super::super::protocol::methods::HarnessStatus;
use super::doctor::{ProbeHost, ProbeOutput, ProbeRun};

/// What the Doctor learned about this machine's Copilot install.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopilotFacts {
    /// Combined stdout+stderr of `copilot --help`.
    pub help_text: String,
    /// Combined stdout+stderr of `copilot --version`.
    pub version_text: String,
    /// Token env vars that are set and non-empty, in Copilot's own precedence
    /// (`COPILOT_GITHUB_TOKEN`, then `GH_TOKEN`, then `GITHUB_TOKEN`).
    pub token_vars: Vec<String>,
    /// `COPILOT_MODEL` when exported (including an empty string, which is a
    /// misconfiguration rather than "use the default").
    pub model_env: Option<String>,
    /// Directory Copilot will actually use: `$COPILOT_HOME` or `~/.copilot`.
    pub home: Option<String>,
    /// Contents of `config.json` in that directory, when the file exists.
    pub config_json: Option<String>,
    /// Contents of `settings.json` in that directory, when the file exists.
    pub settings_json: Option<String>,
}

/// One sentence the Doctor can show, plus the status and the fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotDiagnosis {
    pub status: HarnessStatus,
    pub detail: String,
    pub remedy: Option<String>,
}

/// Gather the facts [`classify`] needs from this machine.
pub fn gather(probe: &dyn ProbeHost) -> CopilotFacts {
    let help = probe.run_text("copilot", &["--help".into()]);
    let version = probe.run_text("copilot", &["--version".into()]);
    let token_vars = ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .filter(|key| probe.env_value(key).is_some())
        .map(str::to_string)
        .collect();
    let model_env = probe.env_raw("COPILOT_MODEL");
    let home = probe.env_value("COPILOT_HOME").or_else(|| {
        probe
            .env_value("HOME")
            .map(|home| format!("{home}/.copilot"))
    });
    let (config_json, settings_json) = match &home {
        Some(dir) => (
            probe.read_file(&format!("{dir}/config.json")),
            probe.read_file(&format!("{dir}/settings.json")),
        ),
        None => (None, None),
    };
    CopilotFacts {
        help_text: output_text(&help),
        version_text: output_text(&version),
        token_vars,
        model_env,
        home,
        config_json,
        settings_json,
    }
}

fn output_text(output: &ProbeOutput) -> String {
    match output.run {
        ProbeRun::Failed(ref err) => format!("{err}\n{}", output.text),
        _ => output.text.clone(),
    }
}

/// Turn gathered facts into a Doctor status. Order is the user's next step:
/// install a build that speaks ACP, then sign in, then fix model/policy.
pub fn classify(facts: &CopilotFacts) -> CopilotDiagnosis {
    if !supports_acp(&facts.help_text) {
        let version = facts
            .version_text
            .lines()
            .next()
            .unwrap_or("unknown version");
        return CopilotDiagnosis {
            status: HarnessStatus::AdapterOutdated,
            detail: format!(
                "this Copilot CLI ({}) does not advertise `copilot --acp`.",
                version.trim()
            ),
            remedy: Some(
                "Update GitHub Copilot CLI (`npm i -g @github/copilot` or `copilot update`) to a build that supports Agent Client Protocol (`copilot --acp`)."
                    .into(),
            ),
        };
    }

    if let Some(policy) = policy_problem(facts) {
        return policy;
    }

    if !is_authenticated(facts) {
        return CopilotDiagnosis {
            status: HarnessStatus::LoggedOut,
            detail: "GitHub Copilot CLI is installed but no GitHub credentials were found."
                .into(),
            remedy: Some(
                "Run `copilot login`, or export COPILOT_GITHUB_TOKEN / GH_TOKEN / GITHUB_TOKEN. `gh auth login` also works when JaBot can inherit that token."
                    .into(),
            ),
        };
    }

    if let Some(model) = model_problem(facts) {
        return model;
    }

    let via = auth_detail(facts);
    CopilotDiagnosis {
        status: HarnessStatus::Ready,
        detail: format!("Ready — {via}"),
        remedy: None,
    }
}

/// Official ACP docs: `copilot --acp` (stdio by default). A build whose help
/// text does not mention `--acp` cannot be the adapter.
pub fn supports_acp(help_text: &str) -> bool {
    help_text.split_whitespace().any(|token| {
        token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-') == "--acp"
    }) || help_text.contains(" --acp")
        || help_text.contains("\n--acp")
        || help_text.contains("\t--acp")
}

fn is_authenticated(facts: &CopilotFacts) -> bool {
    if !facts.token_vars.is_empty() {
        return true;
    }
    logged_in_users(facts.config_json.as_deref()).is_some()
}

fn logged_in_users(config_json: Option<&str>) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(config_json?).ok()?;
    let users = value.get("loggedInUsers")?.as_array()?;
    let names: Vec<String> = users
        .iter()
        .filter_map(|user| {
            user.as_str()
                .map(str::to_string)
                .or_else(|| {
                    user.get("login")
                        .and_then(|login| login.as_str())
                        .map(str::to_string)
                })
                .or_else(|| {
                    user.get("name")
                        .and_then(|name| name.as_str())
                        .map(str::to_string)
                })
        })
        .filter(|name| !name.is_empty())
        .collect();
    (!names.is_empty()).then_some(names)
}

fn policy_problem(facts: &CopilotFacts) -> Option<CopilotDiagnosis> {
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        facts.help_text,
        facts.version_text,
        facts.config_json.as_deref().unwrap_or(""),
        facts.settings_json.as_deref().unwrap_or("")
    );
    let lower = haystack.to_ascii_lowercase();
    let hit = [
        "access denied by policy",
        "copilot cli policy",
        "organization has restricted",
        "enterprise policy",
        "organizations have not enabled",
        "not enabled use of this feature",
    ]
    .into_iter()
    .find(|needle| lower.contains(needle))?;
    Some(CopilotDiagnosis {
        status: HarnessStatus::InvalidConfig,
        detail: format!("GitHub Copilot CLI is blocked by subscription or organization policy ({hit})."),
        remedy: Some(
            "Check github.com/settings/copilot for an active license, and ask an org admin to enable Copilot CLI in organization policy."
                .into(),
        ),
    })
}

fn model_problem(facts: &CopilotFacts) -> Option<CopilotDiagnosis> {
    if let Some(model) = &facts.model_env {
        if model.trim().is_empty() {
            return Some(CopilotDiagnosis {
                status: HarnessStatus::InvalidConfig,
                detail: "COPILOT_MODEL is set but empty, so Copilot has no model to use.".into(),
                remedy: Some(
                    "Unset COPILOT_MODEL to use Copilot's default, or set it to a model your plan allows (see `copilot` `/model`)."
                        .into(),
                ),
            });
        }
    }
    let settings: serde_json::Value = serde_json::from_str(facts.settings_json.as_deref()?).ok()?;
    let model = settings.get("model")?;
    let blank = match model {
        serde_json::Value::Null => true,
        serde_json::Value::String(text) => text.trim().is_empty(),
        _ => false,
    };
    blank.then(|| CopilotDiagnosis {
        status: HarnessStatus::InvalidConfig,
        detail: "Copilot settings pin an empty model.".into(),
        remedy: Some(
            "Set `model` in ~/.copilot/settings.json (or $COPILOT_HOME/settings.json), or choose one with `/model` in `copilot`."
                .into(),
        ),
    })
}

fn auth_detail(facts: &CopilotFacts) -> String {
    if let Some(var) = facts.token_vars.first() {
        return format!("authenticated via `{var}`");
    }
    if let Some(users) = logged_in_users(facts.config_json.as_deref()) {
        let home = facts.home.as_deref().unwrap_or("~/.copilot");
        return format!("signed in as {} ({home})", users.join(", "));
    }
    "authenticated".into()
}

/// Re-read a failed ACP handshake in Copilot's own words. The shallow probe
/// cannot hit the network; a deep Doctor can, and must not report a policy
/// denial as "unknown".
pub fn classify_handshake_error(err: &str) -> Option<CopilotDiagnosis> {
    let lower = err.to_ascii_lowercase();
    if [
        "access denied by policy",
        "copilot cli policy",
        "organization has restricted",
        "enterprise policy",
        "403 forbidden",
        "not enabled use of this feature",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Some(CopilotDiagnosis {
            status: HarnessStatus::InvalidConfig,
            detail: format!("Copilot ACP handshake was refused by subscription or organization policy: {err}"),
            remedy: Some(
                "Check github.com/settings/copilot for an active license, and ask an org admin to enable Copilot CLI in organization policy."
                    .into(),
            ),
        });
    }
    if [
        "no authentication",
        "not logged in",
        "not authenticated",
        "401 unauthorized",
        "authentication information found",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Some(CopilotDiagnosis {
            status: HarnessStatus::LoggedOut,
            detail: format!("Copilot ACP handshake failed to authenticate: {err}"),
            remedy: Some(
                "Run `copilot login`, or export COPILOT_GITHUB_TOKEN / GH_TOKEN / GITHUB_TOKEN."
                    .into(),
            ),
        });
    }
    if lower.contains("model")
        && (lower.contains("not found")
            || lower.contains("invalid")
            || lower.contains("unavailable")
            || lower.contains("not configured"))
    {
        return Some(CopilotDiagnosis {
            status: HarnessStatus::InvalidConfig,
            detail: format!("Copilot ACP handshake failed on the configured model: {err}"),
            remedy: Some(
                "Set a model your plan allows (`COPILOT_MODEL` or `/model` in `copilot`), or unset an empty model pin."
                    .into(),
            ),
        });
    }
    None
}

/// What #218 should set per account profile, and nothing else.
///
/// Copilot's own `activeProfile` and deprecated `--config-dir` do **not**
/// isolate plugins or MCP servers. Only `COPILOT_HOME` does. When the
/// account-profile system lands, JaBot must point this harness at a distinct
/// directory via `COPILOT_HOME` rather than asking Copilot to switch users
/// in a shared home.
#[allow(dead_code)] // #218 account profiles are not wired yet; this is the hook.
pub fn profile_env(profile_home: &str) -> Vec<(String, String)> {
    vec![("COPILOT_HOME".into(), profile_home.into())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_facts() -> CopilotFacts {
        CopilotFacts {
            help_text:
                "Usage: copilot [options]\n  --acp    Start as Agent Client Protocol server\n"
                    .into(),
            version_text: "0.0.400\n".into(),
            token_vars: vec!["GH_TOKEN".into()],
            model_env: None,
            home: Some("/tmp/copilot-home".into()),
            config_json: None,
            settings_json: None,
        }
    }

    #[test]
    fn a_build_without_acp_is_outdated_not_ready() {
        let facts = CopilotFacts {
            help_text: "Usage: copilot\n  --login\n".into(),
            version_text: "0.0.100\n".into(),
            ..CopilotFacts::default()
        };
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::AdapterOutdated);
        assert!(report.detail.contains("0.0.100"), "{}", report.detail);
        assert!(report.remedy.unwrap().contains("copilot --acp"));
    }

    #[test]
    fn help_that_names_acp_counts_as_supported() {
        assert!(supports_acp("  --acp  Start ACP"));
        assert!(supports_acp("Options:\n--acp\n--stdio"));
        assert!(!supports_acp("copilot login"));
        assert!(!supports_acp("the acronym ACP appears in prose"));
    }

    #[test]
    fn missing_credentials_are_logged_out() {
        let mut facts = ready_facts();
        facts.token_vars.clear();
        facts.config_json = Some(r#"{"loggedInUsers":[]}"#.into());
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("copilot login"));
    }

    #[test]
    fn a_stored_login_counts_without_a_token_env() {
        let mut facts = ready_facts();
        facts.token_vars.clear();
        facts.config_json = Some(r#"{"loggedInUsers":[{"login":"octocat"}]}"#.into());
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert!(report.detail.contains("octocat"), "{}", report.detail);
    }

    #[test]
    fn empty_model_env_is_invalid_config() {
        let mut facts = ready_facts();
        facts.model_env = Some("   ".into());
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.detail.contains("COPILOT_MODEL"), "{}", report.detail);
    }

    #[test]
    fn empty_settings_model_is_invalid_config() {
        let mut facts = ready_facts();
        facts.settings_json = Some(r#"{"model":""}"#.into());
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("settings.json"));
    }

    #[test]
    fn an_unset_model_uses_the_default_and_is_ready() {
        let facts = ready_facts();
        assert_eq!(classify(&facts).status, HarnessStatus::Ready);
    }

    #[test]
    fn org_policy_text_is_not_a_login_problem() {
        let mut facts = ready_facts();
        facts
            .help_text
            .push_str("\nAccess denied by policy settings\n");
        let report = classify(&facts);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.detail.contains("policy"), "{}", report.detail);
        assert!(report.remedy.unwrap().contains("organization"));
    }

    #[test]
    fn handshake_errors_are_not_unknown() {
        let policy = classify_handshake_error("Access denied by policy settings").unwrap();
        assert_eq!(policy.status, HarnessStatus::InvalidConfig);
        let auth = classify_handshake_error("initialize failed: not authenticated (401)").unwrap();
        assert_eq!(auth.status, HarnessStatus::LoggedOut);
        let model = classify_handshake_error("model gpt-nope is unavailable").unwrap();
        assert_eq!(model.status, HarnessStatus::InvalidConfig);
        assert!(classify_handshake_error("connection reset").is_none());
    }

    #[test]
    fn profile_isolation_is_copilot_home_only() {
        assert_eq!(
            profile_env("/jabot/profiles/work"),
            vec![("COPILOT_HOME".into(), "/jabot/profiles/work".into())]
        );
    }
}
