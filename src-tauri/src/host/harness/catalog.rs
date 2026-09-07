//! The three-tier harness catalog (#13, decision #6).
//!
//! Tier 1 (shipped) and tier 2 (presets) are compiled in. A user cannot edit
//! them and their ids are reserved, so a JSON file dropped into
//! `custom_harnesses/` cannot quietly replace `claude` with something else.
//! Tier 3 is that JSON.
//!
//! Presets are deliberately not second-class in the Doctor. PATH-only
//! readiness is exactly what makes OpenClaw claim to work while its Gateway is
//! down (`docs/research/setup-porting/buzz.md` §4), so every tier carries the
//! same [`Readiness`] description and goes through the same probe.
//!
//! A card lists *candidates*, not one command: Claude's adapter was renamed
//! from `@zed-industries/claude-code-acp` to
//! `@agentclientprotocol/claude-agent-acp` and both are the same zero-arg
//! runtime, and Pi is reachable as `pi-acp`, as `omp acp`, or through `npx`.
//! Resolution picks the first candidate that exists on the augmented PATH, so
//! a machine with only the older binary still gets a working card.
//!
//! The last candidate on a card can be the copy this build ships inside
//! JaBot.app (`bundled.rs`). It goes last on purpose: an adapter the user
//! installed themselves is a deliberate choice, and ours is the floor under it
//! — there so that having Claude Code installed is the only thing a person has
//! to do before New Chat works.

use std::collections::BTreeMap;

use super::super::protocol::methods::{
    HarnessCardView, HarnessStatus, HarnessTier, RuntimeSpec, SessionScope,
};

/// One way to start a harness, tried in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
    /// `npx -y <pkg>` resolves because `npx` is installed, not because the
    /// harness is. Ready, but the first run pays for a download — the Doctor
    /// says so rather than pretending the package is already here.
    pub downloads_on_first_run: bool,
    /// This candidate is the copy shipped inside JaBot.app (`bundled.rs`), not
    /// something the user installed. It changes what the Doctor says — the
    /// resolved path is a Node, not an adapter, and "Ready — /opt/homebrew/bin/node"
    /// would tell a user nothing.
    pub bundled: bool,
    /// Env this candidate needs and the others do not. The bundled adapter
    /// carries `CLAUDE_CODE_EXECUTABLE`; a globally installed one ships its own
    /// Claude Code binary and must not be redirected at ours.
    pub env: BTreeMap<String, String>,
}

impl Launch {
    fn new(command: &str, args: &[&str], downloads_on_first_run: bool) -> Self {
        Self {
            command: command.to_string(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            downloads_on_first_run,
            bundled: false,
            env: BTreeMap::new(),
        }
    }

    /// A candidate resolved out of this build's own resources.
    pub fn bundled(command: &str, args: &[&str], env: BTreeMap<String, String>) -> Self {
        Self {
            bundled: true,
            env,
            ..Self::new(command, args, false)
        }
    }
}

/// What "installed" is not enough to prove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// The binary being there is the whole story.
    Binary,
    /// Run a command and read its exit code. Non-zero means `on_failure`.
    Command {
        command: String,
        args: Vec<String>,
        on_failure: HarnessStatus,
        remedy: String,
    },
    /// A daemon must be listening before the adapter can do anything.
    Daemon { addr: String, remedy: String },
    /// Vendor-specific inspect after the CLI is known to be on PATH.
    Inspect { kind: InspectKind },
}

/// Extra classification a binary-on-PATH answer cannot give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectKind {
    Gemini,
}

/// A catalog entry, whatever tier it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessDescriptor {
    pub id: String,
    pub label: String,
    pub blurb: String,
    pub accent: String,
    pub tier: HarnessTier,
    pub launches: Vec<Launch>,
    /// The vendor CLI behind the adapter. Its absence is `CliMissing` — a
    /// different sentence to the user than "the ACP adapter is missing".
    pub cli: Option<String>,
    /// Floor, not override: a value the user already exported wins. Resolved
    /// in [`HarnessDescriptor::runtime_spec`], so what a thread snapshots is
    /// what the supervisor will apply.
    pub env: BTreeMap<String, String>,
    pub install_hint: Option<String>,
    pub install_url: Option<String>,
    /// What this harness advertises — and what it does not. Absent means the
    /// host negotiates capabilities at `initialize` and has nothing extra to
    /// say up front.
    pub capability_notes: Option<String>,
    pub readiness: Readiness,
    pub session_scope: SessionScope,
}

impl HarnessDescriptor {
    /// What `thread/open` snapshots and what the supervisor spawns, for one
    /// resolved candidate.
    ///
    /// The env floor is resolved *here*, because this is the one place where
    /// the env is purely the catalog's: a key the user already exported is
    /// left out of the snapshot so their value is the one the child inherits.
    /// Downstream the spec is indistinguishable from env a client passed to
    /// `thread/open`, and that env is not a default the environment may
    /// outvote — it is the runtime the thread recorded.
    ///
    /// A candidate's own env (`Launch::env`) is merged in first and floored
    /// the same way: `CLAUDE_CODE_EXECUTABLE` is how the bundled adapter is
    /// told which Claude Code to drive, and someone who exported their own
    /// still means it.
    pub fn runtime_spec(&self, launch: &Launch) -> RuntimeSpec {
        let mut catalog_env = self.env.clone();
        catalog_env.extend(launch.env.clone());
        let env: BTreeMap<String, String> =
            super::floor_env(&catalog_env, |key| std::env::var_os(key).is_some())
                .into_iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect();
        RuntimeSpec {
            command: launch.command.clone(),
            args: Some(launch.args.clone()),
            env: (!env.is_empty()).then_some(env),
            install_hint: self.install_hint.clone(),
        }
    }

    pub fn primary(&self) -> &Launch {
        // Every descriptor is built with at least one launch: the compiled
        // tables below all have one, and `custom::parse` rejects a file with
        // an empty command.
        &self.launches[0]
    }

    pub fn card(&self) -> HarnessCardView {
        let launch = self.primary();
        HarnessCardView {
            id: self.id.clone(),
            label: self.label.clone(),
            blurb: self.blurb.clone(),
            accent: self.accent.clone(),
            tier: self.tier,
            command: launch.command.clone(),
            args: launch.args.clone(),
            install_hint: self.install_hint.clone(),
            install_url: self.install_url.clone(),
            capability_notes: self.capability_notes.clone(),
            session_scope: self.session_scope,
            reserved: is_reserved(&self.id),
        }
    }

    /// Which adapter processes may be shared.
    ///
    /// Hermes wants one long-lived process per profile with JaBot chats
    /// multiplexed as ACP sessions (`setup-porting/hermes.md` §5), so two
    /// threads on the same profile key belong on the same process and two
    /// profiles never do. Threads are the key for everything else.
    pub fn profile_key(&self, thread_id: &str) -> String {
        match self.session_scope {
            SessionScope::Profile => {
                let launch = self.primary();
                format!("{}:{}", self.id, launch.args.join(" "))
            }
            SessionScope::Thread => format!("{}:{thread_id}", self.id),
        }
    }
}

struct Compiled {
    id: &'static str,
    label: &'static str,
    blurb: &'static str,
    accent: &'static str,
    tier: HarnessTier,
    launches: &'static [(&'static str, &'static [&'static str], bool)],
    cli: Option<&'static str>,
    env: &'static [(&'static str, &'static str)],
    install_hint: &'static str,
    install_url: &'static str,
    capability_notes: Option<&'static str>,
    readiness: CompiledReadiness,
    session_scope: SessionScope,
}

enum CompiledReadiness {
    Binary,
    Command(
        &'static str,
        &'static [&'static str],
        HarnessStatus,
        &'static str,
    ),
    Daemon(&'static str, &'static str),
    Inspect(InspectKind),
}

/// Tier 1. Reserved ids, one New Chat card each.
const SHIPPED: &[Compiled] = &[
    Compiled {
        id: "claude",
        label: "Claude Code",
        blurb: "Anthropic's coding agent, wrapped in JaBot's UI",
        accent: "var(--h-claude)",
        tier: HarnessTier::Shipped,
        // `claude-code-acp` is the older name for the same zero-arg runtime.
        launches: &[
            ("claude-agent-acp", &[], false),
            ("claude-code-acp", &[], false),
        ],
        cli: Some("claude"),
        env: &[],
        // JaBot ships the adapter, so the only install left is Claude Code
        // itself — and Node, which the bundled adapter runs on. The npm line
        // stays for the machine with no Node and for anyone who would rather
        // run their own.
        install_hint: "Install Claude Code (and Node 20+, which JaBot's bundled adapter runs on), or install the adapter yourself with `npm i -g @agentclientprotocol/claude-agent-acp`.",
        install_url: "https://github.com/agentclientprotocol/claude-agent-acp",
        capability_notes: None,
        readiness: CompiledReadiness::Command(
            "claude",
            &["auth", "status"],
            HarnessStatus::LoggedOut,
            "Run `claude` once and sign in, or export ANTHROPIC_API_KEY.",
        ),
        session_scope: SessionScope::Thread,
    },
    Compiled {
        id: "codex",
        label: "Codex",
        blurb: "OpenAI's coding agent",
        accent: "var(--h-codex)",
        tier: HarnessTier::Shipped,
        launches: &[("codex-acp", &[], false)],
        cli: Some("codex"),
        env: &[],
        install_hint: "Install Codex, then `npm i -g @zed-industries/codex-acp`.",
        install_url: "https://github.com/agentclientprotocol/codex-acp",
        capability_notes: None,
        readiness: CompiledReadiness::Command(
            "codex",
            &["login", "status"],
            HarnessStatus::LoggedOut,
            "Run `codex login`.",
        ),
        session_scope: SessionScope::Thread,
    },
    Compiled {
        id: "pi",
        label: "Pi",
        blurb: "Mario Zechner's coding agent",
        accent: "var(--h-pi)",
        tier: HarnessTier::Shipped,
        launches: &[
            ("pi-acp", &[], false),
            ("omp", &["acp"], false),
            ("npx", &["-y", "pi-acp"], true),
        ],
        cli: Some("pi"),
        env: &[],
        install_hint: "Install Pi (pi.dev), then `npm i -g pi-acp`.",
        install_url: "https://pi.dev/",
        capability_notes: None,
        // Pi resolves credentials per provider at run time and has no
        // login-status command to ask; claiming otherwise would be a guess.
        readiness: CompiledReadiness::Binary,
        session_scope: SessionScope::Thread,
    },
    Compiled {
        id: "gemini",
        label: "Gemini CLI",
        blurb: "Google's Gemini CLI over its documented ACP mode",
        accent: "var(--h-gemini)",
        tier: HarnessTier::Shipped,
        // Current docs (`geminicli.com/docs/cli/acp-mode/`) start ACP with
        // `--acp`. Older builds only advertised `--experimental-acp`. Both
        // are the same stdio JSON-RPC agent; Doctor picks the flag `--help`
        // actually lists so a thread does not snapshot a name that will fail.
        launches: &[
            ("gemini", &["--acp"], false),
            ("gemini", &["--experimental-acp"], false),
        ],
        cli: Some("gemini"),
        env: &[],
        install_hint: "Install Gemini CLI (`npm i -g @google/gemini-cli`), then run `gemini` once to sign in or export GEMINI_API_KEY.",
        install_url: "https://geminicli.com/docs/cli/acp-mode/",
        capability_notes: Some(
            "Streams, tools, permissions, cancel, and session/load. Does not advertise session/resume or session/close — JaBot falls back to load, then a new session.",
        ),
        readiness: CompiledReadiness::Inspect(InspectKind::Gemini),
        session_scope: SessionScope::Thread,
    },
];

/// Tier 2. PATH-probed, not user-editable, ids still reserved.
const PRESETS: &[Compiled] = &[
    Compiled {
        id: "hermes",
        label: "Hermes",
        blurb: "Nous Research's agent runtime",
        accent: "var(--h-hermes)",
        tier: HarnessTier::Preset,
        launches: &[("hermes", &["acp"], false), ("hermes-acp", &[], false)],
        cli: Some("hermes"),
        // Host-selected MCP has to win over Hermes' own config.yaml servers
        // (decision #6: skip ambient harness MCP as a general rule).
        env: &[("HERMES_ACP_SKIP_CONFIGURED_MCP", "1")],
        install_hint: "Install Hermes Agent, then run `hermes setup`.",
        install_url: "https://hermes-agent.nousresearch.com/docs/user-guide/features/acp",
        capability_notes: None,
        // `hermes acp --check` is the vendor's own readiness answer: it fails
        // when no provider or model is configured, which is a different fix
        // from "log in".
        readiness: CompiledReadiness::Command(
            "hermes",
            &["acp", "--check"],
            HarnessStatus::InvalidConfig,
            "Run `hermes acp --setup` to choose a provider and model.",
        ),
        session_scope: SessionScope::Profile,
    },
    Compiled {
        id: "openclaw",
        label: "OpenClaw",
        blurb: "Your self-hosted OpenClaw Gateway",
        accent: "var(--h-openclaw)",
        tier: HarnessTier::Preset,
        launches: &[("openclaw", &["acp"], false)],
        cli: Some("openclaw"),
        env: &[],
        install_hint: "Install OpenClaw and run `openclaw onboard --install-daemon`.",
        install_url: "https://docs.openclaw.ai/tools/acp-agents",
        capability_notes: None,
        // `openclaw acp` is a bridge to a long-lived Gateway. On PATH and
        // Gateway down, it is a false ready: the binary answers and every
        // session fails. The daemon socket is the honest question.
        readiness: CompiledReadiness::Daemon(
            "127.0.0.1:18789",
            "Start the Gateway (`openclaw gateway status` shows whether it is up).",
        ),
        session_scope: SessionScope::Profile,
    },
];

fn build(compiled: &Compiled) -> HarnessDescriptor {
    HarnessDescriptor {
        id: compiled.id.to_string(),
        label: compiled.label.to_string(),
        blurb: compiled.blurb.to_string(),
        accent: compiled.accent.to_string(),
        tier: compiled.tier,
        // The user's own installs first, this build's copy after them
        // (module docs above). A card with nothing bundled is unchanged.
        launches: compiled
            .launches
            .iter()
            .map(|(command, args, downloads)| Launch::new(command, args, *downloads))
            .chain(super::bundled::launches_for(compiled.id))
            .collect(),
        cli: compiled.cli.map(str::to_string),
        env: compiled
            .env
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        install_hint: Some(compiled.install_hint.to_string()),
        install_url: Some(compiled.install_url.to_string()),
        capability_notes: compiled.capability_notes.map(str::to_string),
        readiness: match &compiled.readiness {
            CompiledReadiness::Binary => Readiness::Binary,
            CompiledReadiness::Command(command, args, on_failure, remedy) => Readiness::Command {
                command: (*command).to_string(),
                args: args.iter().map(|a| (*a).to_string()).collect(),
                on_failure: *on_failure,
                remedy: (*remedy).to_string(),
            },
            CompiledReadiness::Daemon(addr, remedy) => Readiness::Daemon {
                addr: (*addr).to_string(),
                remedy: (*remedy).to_string(),
            },
            CompiledReadiness::Inspect(kind) => Readiness::Inspect { kind: kind.clone() },
        },
        session_scope: compiled.session_scope,
    }
}

/// Tiers 1 and 2, in New Chat order: shipped cards first, then presets.
pub fn compiled_in() -> Vec<HarnessDescriptor> {
    SHIPPED.iter().chain(PRESETS).map(build).collect()
}

/// Ids tier 3 may not take. Shadowing `claude` with a user file would make the
/// crew's default harness mean whatever the file says.
pub fn is_reserved(id: &str) -> bool {
    SHIPPED
        .iter()
        .chain(PRESETS)
        .any(|compiled| compiled.id == id)
}

/// The accent every tier-3 card shares (`--h-custom` in the design tokens).
pub const CUSTOM_ACCENT: &str = "var(--h-custom)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_ids_are_the_reserved_cards() {
        let ids: Vec<_> = SHIPPED.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["claude", "codex", "pi", "gemini"]);
        for id in ids {
            assert!(is_reserved(id), "{id} must be reserved");
        }
        assert!(is_reserved("hermes"), "presets are reserved too");
        assert!(!is_reserved("my-agent"));
    }

    #[test]
    fn gemini_speaks_acp_on_the_vendor_cli_itself() {
        let gemini = compiled_in()
            .into_iter()
            .find(|d| d.id == "gemini")
            .unwrap();
        assert_eq!(gemini.tier, HarnessTier::Shipped);
        assert_eq!(gemini.session_scope, SessionScope::Thread);
        assert_eq!(gemini.cli.as_deref(), Some("gemini"));
        assert_eq!(gemini.primary().command, "gemini");
        assert_eq!(gemini.primary().args, ["--acp"]);
        assert!(gemini
            .launches
            .iter()
            .any(|l| l.args == ["--experimental-acp"]));
        assert!(gemini
            .capability_notes
            .as_deref()
            .unwrap()
            .contains("session/load"));
        assert!(matches!(
            gemini.readiness,
            Readiness::Inspect {
                kind: InspectKind::Gemini
            }
        ));
    }

    /// Both adapter names are tried, current one first, and whatever this
    /// build bundles comes after them — never instead of them. Filtered by
    /// `bundled` rather than asserted on the whole list because the bundled
    /// candidate only exists on a machine that has Node, `claude`, and a
    /// staged tree, and this test has to say the same thing on all of them.
    #[test]
    fn claude_tries_both_adapter_names_before_anything_bundled() {
        let claude = compiled_in()
            .into_iter()
            .find(|d| d.id == "claude")
            .unwrap();
        let installed: Vec<_> = claude
            .launches
            .iter()
            .take_while(|l| !l.bundled)
            .map(|l| l.command.clone())
            .collect();
        assert_eq!(installed, ["claude-agent-acp", "claude-code-acp"]);
        assert!(
            claude.launches[installed.len()..].iter().all(|l| l.bundled),
            "a bundled candidate may only follow the user's own installs"
        );
    }

    /// The bundled candidate is the app's, not the user's, so it carries the
    /// env that makes it work and the other candidates do not inherit it.
    #[test]
    fn a_bundled_launch_contributes_its_env_to_the_snapshot() {
        let claude = compiled_in()
            .into_iter()
            .find(|d| d.id == "claude")
            .unwrap();
        let bundled = Launch::bundled(
            "/usr/bin/node",
            &["/Applications/JaBot.app/adapter.js"],
            BTreeMap::from([("CLAUDE_CODE_EXECUTABLE".into(), "/opt/bin/claude".into())]),
        );
        let spec = claude.runtime_spec(&bundled);
        assert_eq!(spec.command, "/usr/bin/node");
        // Skipped for whoever is debugging with their own export: the floor
        // hands them theirs, which is the documented behaviour and not this
        // test's subject.
        if std::env::var_os("CLAUDE_CODE_EXECUTABLE").is_none() {
            assert_eq!(
                spec.env
                    .as_ref()
                    .and_then(|env| env.get("CLAUDE_CODE_EXECUTABLE"))
                    .map(String::as_str),
                Some("/opt/bin/claude")
            );
        }
        assert!(
            claude.runtime_spec(claude.primary()).env.is_none(),
            "a candidate the user installed is not redirected at our Claude Code"
        );
    }

    /// The floor is the snapshot's, not the spawner's: a key the environment
    /// already answers is omitted so the exported value survives, and
    /// everything else is recorded verbatim for the supervisor to apply.
    #[test]
    fn runtime_spec_drops_env_the_environment_already_answers() {
        let mut hermes = compiled_in()
            .into_iter()
            .find(|d| d.id == "hermes")
            .unwrap();
        let spec = hermes.runtime_spec(hermes.primary());
        assert_eq!(
            spec.env
                .as_ref()
                .and_then(|env| env.get("HERMES_ACP_SKIP_CONFIGURED_MCP"))
                .map(String::as_str),
            Some("1"),
            "an unexported switch belongs in the snapshot"
        );

        // `HOME` stands in for "the user exported this already" — it is the one
        // key every machine running this test has set.
        if std::env::var_os("HOME").is_some() {
            hermes.env.insert("HOME".into(), "/jabot/floor".into());
            let spec = hermes.runtime_spec(hermes.primary());
            assert!(!spec.env.expect("catalog env").contains_key("HOME"));
        }
    }

    #[test]
    fn hermes_carries_the_skip_ambient_mcp_floor() {
        let hermes = compiled_in()
            .into_iter()
            .find(|d| d.id == "hermes")
            .unwrap();
        assert_eq!(
            hermes
                .env
                .get("HERMES_ACP_SKIP_CONFIGURED_MCP")
                .map(String::as_str),
            Some("1")
        );
    }

    /// Hermes multiplexes chats onto one process per profile; two chats on the
    /// same profile must land on the same key and two profiles must not.
    #[test]
    fn profile_scope_shares_a_process_and_thread_scope_does_not() {
        let mut hermes = compiled_in()
            .into_iter()
            .find(|d| d.id == "hermes")
            .unwrap();
        assert_eq!(hermes.profile_key("t1"), hermes.profile_key("t2"));

        let researcher = Launch::new("hermes", &["-p", "researcher", "acp"], false);
        hermes.launches.insert(0, researcher);
        assert_ne!(
            hermes.profile_key("t1"),
            compiled_in()
                .into_iter()
                .find(|d| d.id == "hermes")
                .unwrap()
                .profile_key("t1")
        );

        let claude = compiled_in()
            .into_iter()
            .find(|d| d.id == "claude")
            .unwrap();
        assert_ne!(claude.profile_key("t1"), claude.profile_key("t2"));
    }

    #[test]
    fn openclaw_readiness_is_the_gateway_not_the_binary() {
        let openclaw = compiled_in()
            .into_iter()
            .find(|d| d.id == "openclaw")
            .unwrap();
        assert!(matches!(openclaw.readiness, Readiness::Daemon { .. }));
    }
}
