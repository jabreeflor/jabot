//! Gemini CLI readiness (#219).
//!
//! Gemini is the product *and* the ACP adapter: `gemini --acp` (or the older
//! `--experimental-acp` flag). There is no separate npm wrapper to blame, so
//! the Doctor has to answer four different questions from the same binary:
//!
//! 1. Is `gemini` installed?
//! 2. Does this build speak ACP (`--acp` or `--experimental-acp`)?
//! 3. Is anyone signed in (env, `~/.gemini` profile, Vertex ADC)?
//! 4. Is the selected auth/model actually usable (Vertex needs a project;
//!    an empty `model.name` is a config hole)?
//!
//! Auth lives in Gemini CLI's own user profile (`~/.gemini`, env, OS
//! keychain). JaBot's Google MCP grant (Gmail / Calendar / Drive) is a
//! different OAuth client and is **not** reused. Every Gemini thread on this
//! machine shares that one profile — upstream does not isolate per bot.

use super::super::protocol::methods::HarnessStatus;
use super::catalog::Launch;
use super::doctor::{ProbeHost, ProbeRun};

/// Official ACP flag, verified against
/// <https://geminicli.com/docs/cli/acp-mode/> (docs last updated 2026-04-10).
pub const ACP_FLAG: &str = "--acp";
/// Older CLI builds advertised the same mode as experimental.
pub const EXPERIMENTAL_ACP_FLAG: &str = "--experimental-acp";

const AUTH_REMEDY: &str =
    "Run `gemini` once and sign in, or export GEMINI_API_KEY (AI Studio) / Vertex ADC.";
const UPDATE_REMEDY: &str = "Update Gemini CLI (`gemini update` or `npm i -g @google/gemini-cli`) to a build that supports `--acp`.";
const MODEL_REMEDY: &str = "Set `model.name` in `~/.gemini/settings.json`, or export GEMINI_MODEL.";
const VERTEX_REMEDY: &str = "Set GOOGLE_CLOUD_PROJECT (or GOOGLE_CLOUD_PROJECT_ID) for Vertex AI, or switch auth in `gemini`.";

/// What the inspect decided, including which ACP flag this build actually has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspect {
    pub status: HarnessStatus,
    pub detail: String,
    pub remedy: Option<String>,
    pub launch: Launch,
}

/// Pick `--acp` when the installed CLI advertises it, else the experimental
/// flag. Used both by the Doctor and by `resolved_runtime_spec`, so a thread
/// opened on an older CLI does not snapshot a flag that will fail at spawn.
pub fn select_launch(launches: &[Launch], probe: &dyn ProbeHost) -> Option<Launch> {
    let primary = launches.first()?;
    match probe.stdout(&primary.command, &["--help".into()]) {
        Ok(help) => acp_args_from_help(&help).map(|args| with_args(primary, args)),
        Err(_) => Some(primary.clone()),
    }
}

/// Which ACP argv this `--help` text supports, current flag first.
pub fn acp_args_from_help(help: &str) -> Option<Vec<String>> {
    if help_has_flag(help, ACP_FLAG) {
        Some(vec![ACP_FLAG.to_string()])
    } else if help_has_flag(help, EXPERIMENTAL_ACP_FLAG) {
        Some(vec![EXPERIMENTAL_ACP_FLAG.to_string()])
    } else {
        None
    }
}

fn help_has_flag(help: &str, flag: &str) -> bool {
    help.split_whitespace()
        .any(|token| token == flag || token.split(',').any(|part| part.trim() == flag))
}

fn with_args(launch: &Launch, args: Vec<String>) -> Launch {
    Launch {
        args,
        ..launch.clone()
    }
}

/// Classify an installed `gemini` binary.
pub fn inspect(probe: &dyn ProbeHost, launch: &Launch) -> Inspect {
    let help = match probe.stdout(&launch.command, &["--help".into()]) {
        Ok(text) => text,
        Err(ProbeRun::TimedOut) => {
            return Inspect {
                status: HarnessStatus::Unknown,
                detail: "`gemini --help` did not answer in time.".into(),
                remedy: Some(UPDATE_REMEDY.into()),
                launch: launch.clone(),
            };
        }
        Err(ProbeRun::Failed(err)) => {
            return Inspect {
                status: HarnessStatus::Unknown,
                detail: format!("could not run `gemini --help`: {err}"),
                remedy: Some(UPDATE_REMEDY.into()),
                launch: launch.clone(),
            };
        }
        Err(ProbeRun::Exit(code)) => {
            return Inspect {
                status: HarnessStatus::Unknown,
                detail: format!("`gemini --help` exited {code}."),
                remedy: Some(UPDATE_REMEDY.into()),
                launch: launch.clone(),
            };
        }
    };

    let Some(args) = acp_args_from_help(&help) else {
        let version = probe
            .stdout(&launch.command, &["--version".into()])
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        let detail = match version {
            Some(version) => {
                format!("Gemini CLI {version} does not advertise `--acp` or `--experimental-acp`.")
            }
            None => {
                "this Gemini CLI build does not advertise `--acp` or `--experimental-acp`.".into()
            }
        };
        return Inspect {
            status: HarnessStatus::AdapterOutdated,
            detail,
            remedy: Some(UPDATE_REMEDY.into()),
            launch: launch.clone(),
        };
    };
    let launch = with_args(launch, args);

    let profile = GeminiProfile::read(probe);
    if !profile.has_auth() {
        return Inspect {
            status: HarnessStatus::LoggedOut,
            detail: "Gemini CLI is installed but not signed in (no GEMINI_API_KEY, Vertex ADC, or `~/.gemini` account profile).".into(),
            remedy: Some(AUTH_REMEDY.into()),
            launch,
        };
    }
    if profile.vertex_missing_project() {
        return Inspect {
            status: HarnessStatus::InvalidConfig,
            detail: "Vertex AI is selected but no Google Cloud project is set.".into(),
            remedy: Some(VERTEX_REMEDY.into()),
            launch,
        };
    }
    if profile.model_explicitly_empty() {
        return Inspect {
            status: HarnessStatus::InvalidConfig,
            detail: "`model.name` in the Gemini account profile is empty.".into(),
            remedy: Some(MODEL_REMEDY.into()),
            launch,
        };
    }

    let flag = launch.args.first().map(String::as_str).unwrap_or(ACP_FLAG);
    Inspect {
        status: HarnessStatus::Ready,
        detail: format!("Ready via `gemini {flag}`."),
        remedy: None,
        launch,
    }
}

#[derive(Debug, Default)]
struct GeminiProfile {
    api_key: bool,
    google_api_key: bool,
    application_credentials: bool,
    oauth_file: bool,
    accounts_file: bool,
    env_file_auth: bool,
    selected_type: Option<String>,
    model_name: Option<String>,
    cloud_project: bool,
}

impl GeminiProfile {
    fn read(probe: &dyn ProbeHost) -> Self {
        let settings = probe
            .home_file(".gemini/settings.json")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let env_file = probe.home_file(".gemini/.env").unwrap_or_default();
        let selected = settings
            .as_ref()
            .and_then(|value| value.pointer("/security/auth/selectedType"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let model = settings
            .as_ref()
            .and_then(|value| value.pointer("/model/name"))
            .and_then(|value| {
                if value.is_null() {
                    Some(String::new())
                } else {
                    value.as_str().map(str::to_string)
                }
            });
        Self {
            api_key: env_set(probe, "GEMINI_API_KEY") || env_file_has(&env_file, "GEMINI_API_KEY"),
            google_api_key: env_set(probe, "GOOGLE_API_KEY")
                || env_file_has(&env_file, "GOOGLE_API_KEY"),
            application_credentials: env_set(probe, "GOOGLE_APPLICATION_CREDENTIALS"),
            oauth_file: probe
                .home_file(".gemini/oauth_creds.json")
                .is_some_and(|raw| !raw.trim().is_empty()),
            accounts_file: probe
                .home_file(".gemini/google_accounts.json")
                .is_some_and(|raw| !raw.trim().is_empty()),
            env_file_auth: env_file_has(&env_file, "GEMINI_API_KEY")
                || env_file_has(&env_file, "GOOGLE_API_KEY"),
            selected_type: selected,
            model_name: model,
            cloud_project: env_set(probe, "GOOGLE_CLOUD_PROJECT")
                || env_set(probe, "GOOGLE_CLOUD_PROJECT_ID")
                || env_file_has(&env_file, "GOOGLE_CLOUD_PROJECT")
                || env_file_has(&env_file, "GOOGLE_CLOUD_PROJECT_ID"),
        }
    }

    fn has_auth(&self) -> bool {
        self.api_key
            || self.google_api_key
            || self.application_credentials
            || self.oauth_file
            || self.accounts_file
            || self.env_file_auth
            || self
                .selected_type
                .as_deref()
                .is_some_and(|selected| !selected.is_empty())
    }

    fn vertex_missing_project(&self) -> bool {
        self.uses_vertex() && !self.cloud_project && !self.application_credentials
    }

    fn uses_vertex(&self) -> bool {
        matches!(
            self.selected_type.as_deref(),
            Some("vertex-ai" | "USE_VERTEX_AI" | "oauth-vertex")
        )
    }

    fn model_explicitly_empty(&self) -> bool {
        matches!(self.model_name.as_deref(), Some(""))
    }
}

fn env_set(probe: &dyn ProbeHost, key: &str) -> bool {
    probe.env(key).is_some_and(|value| !value.trim().is_empty())
}

fn env_file_has(contents: &str, key: &str) -> bool {
    contents.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return false;
        }
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
            .is_some_and(|value| !value.trim().trim_matches('"').is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_flag_wins_over_the_experimental_one() {
        let help = "Options:\n  --acp\n  --experimental-acp\n  --debug";
        assert_eq!(acp_args_from_help(help), Some(vec![ACP_FLAG.into()]));
    }

    #[test]
    fn older_builds_still_count() {
        assert_eq!(
            acp_args_from_help("  --experimental-acp  Start ACP mode"),
            Some(vec![EXPERIMENTAL_ACP_FLAG.into()])
        );
    }

    #[test]
    fn a_build_without_either_flag_is_not_usable() {
        assert_eq!(acp_args_from_help("gemini --prompt --yolo"), None);
    }
}
