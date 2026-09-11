//! Why a harness is not ready.
//!
//! "Not installed" is the least useful thing a picker can say, because five
//! different problems produce it: the vendor CLI is absent, the ACP adapter is
//! absent, the adapter is too old to speak our protocol, the CLI is installed
//! but logged out, it is configured wrongly, or a daemon it depends on is not
//! running. Each has a different fix, so each is a different status with a
//! different remedy.
//!
//! Probes run concurrently. A serial Doctor takes as long as the slowest
//! vendor CLI multiplied by the size of the catalog, and every one of those
//! seconds is spent in front of a user who just opened New Chat.

use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::super::procgroup;
use super::super::protocol::methods::HarnessStatus;
use super::aider::AiderFacts;
use super::catalog::{HarnessDescriptor, InspectKind, Launch, Readiness};
use super::gemini;

/// A readiness command gets this long before it is killed. Long enough for a
/// CLI that checks a token over the network, short enough that five of them in
/// parallel still feel like opening a menu.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_TIMEOUT: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeRun {
    Exit(i32),
    /// The probe could not be started at all (missing binary, permissions).
    Failed(String),
    TimedOut,
}

/// A readiness command plus whatever it printed. OpenCode reads auth-list /
/// models; Copilot and Gemini read `--help` / `--version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub run: ProbeRun,
    pub stdout: String,
    pub text: String,
}

/// Everything the classifier needs from the machine, behind a seam so the
/// classification rules can be tested without installing five vendor CLIs.
pub trait ProbeHost: Sync {
    fn resolve(&self, command: &str) -> Option<PathBuf>;
    fn run(&self, command: &str, args: &[String]) -> ProbeRun;
    fn run_capture(&self, command: &str, args: &[String]) -> ProbeOutput {
        ProbeOutput {
            run: self.run(command, args),
            stdout: String::new(),
            text: String::new(),
        }
    }
    fn listening(&self, addr: &str) -> bool;
    /// Capture stdout of a short probe. Used when the exit code is not enough
    /// (Aider's `--version`, for example). Failed / timed-out runs stay
    /// [`ProbeRun`].
    fn output(&self, command: &str, args: &[String]) -> Result<String, ProbeRun>;

    /// Combined stdout+stderr of a short probe. Default is the exit only —
    /// most readiness commands are classified by status, not by what they
    /// printed. Copilot is the exception (`--help` must mention `--acp`).
    fn run_text(&self, command: &str, args: &[String]) -> ProbeOutput {
        self.run_capture(command, args)
    }

    /// A non-empty environment value. Empty and unset are the same here:
    /// Copilot treats a blank token as missing.
    fn env_value(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    /// An exported value including the empty string. `COPILOT_MODEL=` is a
    /// misconfiguration, not "use the default".
    fn env_raw(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn read_file(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    /// Captured stdout+stderr of a short probe. Used when the exit code is
    /// not the diagnosis (`gemini --help` lists flags; `gemini --version`
    /// names the build).
    fn stdout(&self, command: &str, args: &[String]) -> Result<String, ProbeRun> {
        let output = self.run_capture(command, args);
        match output.run {
            ProbeRun::Exit(_) => Ok(output.stdout),
            other => Err(other),
        }
    }
    fn env(&self, key: &str) -> Option<String>;
    /// A non-empty environment value the catalog treats as credentials.
    fn env_present(&self, key: &str) -> bool {
        self.env(key).is_some_and(|value| !value.is_empty())
    }
    /// A file under `$HOME`, using `/` separators. `None` if missing or empty
    /// is still `Some` — the caller decides whether blank counts.
    fn home_file(&self, relative: &str) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    pub id: String,
    pub status: HarnessStatus,
    pub detail: String,
    pub remedy: Option<String>,
    /// The launch that resolved, with its command as an absolute path — what
    /// the supervisor would actually spawn.
    pub launch: Option<Launch>,
    pub resolved_path: Option<PathBuf>,
    pub elapsed_ms: u64,
    /// `provider/model` lines the models probe printed, when it answered.
    pub models: Vec<String>,
}

impl Diagnosis {
    pub fn ready(&self) -> bool {
        self.status == HarnessStatus::Ready
    }
}

/// Diagnose every descriptor at once, returning results in catalog order.
///
/// One thread per harness: the work is waiting on other processes and sockets,
/// so the pool that matters is the machine's, not ours.
pub fn diagnose_all(descriptors: &[HarnessDescriptor], probe: &dyn ProbeHost) -> Vec<Diagnosis> {
    if descriptors.len() < 2 {
        return descriptors.iter().map(|d| diagnose(d, probe)).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = descriptors
            .iter()
            .map(|descriptor| scope.spawn(move || diagnose(descriptor, probe)))
            .collect();
        handles
            .into_iter()
            .zip(descriptors)
            .map(|(handle, descriptor)| {
                handle.join().unwrap_or_else(|_| Diagnosis {
                    id: descriptor.id.clone(),
                    status: HarnessStatus::Unknown,
                    detail: "the readiness probe panicked".into(),
                    remedy: None,
                    launch: None,
                    resolved_path: None,
                    elapsed_ms: 0,
                    models: Vec::new(),
                })
            })
            .collect()
    })
}

pub fn diagnose(descriptor: &HarnessDescriptor, probe: &dyn ProbeHost) -> Diagnosis {
    diagnose_with(descriptor, probe, &super::aider::SystemFacts)
}

fn diagnose_with(
    descriptor: &HarnessDescriptor,
    probe: &dyn ProbeHost,
    facts: &dyn AiderFacts,
) -> Diagnosis {
    let started = Instant::now();
    let finish = |status: HarnessStatus,
                  detail: String,
                  remedy: Option<String>,
                  launch: Option<Launch>,
                  resolved: Option<PathBuf>,
                  models: Vec<String>| Diagnosis {
        id: descriptor.id.clone(),
        status,
        detail,
        remedy,
        launch,
        resolved_path: resolved,
        elapsed_ms: started.elapsed().as_millis() as u64,
        models,
    };

    // The vendor CLI is asked about first, before any adapter resolves,
    // because an adapter that is present says nothing about the product it
    // drives. `npx -y pi-acp` resolves on every machine with Node, so without
    // this Pi would report ready on a box with no Pi — the PATH-only false
    // ready this Doctor exists to prevent (`setup-porting/buzz.md` §4). And a
    // readiness command whose binary is absent (`claude auth status` with no
    // `claude`) would come back as an unanswered question with a login remedy
    // the user cannot follow, when the answer was knowable up front.
    if !descriptor.cli.is_empty()
        && descriptor
            .cli
            .iter()
            .all(|cli| probe.resolve(cli).is_none())
    {
        let names = descriptor
            .cli
            .iter()
            .map(|cli| format!("`{cli}`"))
            .collect::<Vec<_>>()
            .join(" or ");
        return finish(
            HarnessStatus::CliMissing,
            format!(
                "{} is not installed — no {names} on PATH.",
                descriptor.label
            ),
            descriptor.install_hint.clone(),
            None,
            None,
            Vec::new(),
        );
    }

    // Aider's interesting failures (old version, no API key, no model) are
    // properties of the vendor CLI, not of the JaBot adapter, so they are
    // asked here — after `aider` is known to exist and before a missing
    // `jabot-aider-acp` would hide them.
    if descriptor.id == "aider" {
        if let Some(report) = super::aider::classify(descriptor, probe, facts, None, None, started)
        {
            return report;
        }
    }

    let resolved = descriptor
        .launches
        .iter()
        .find_map(|launch| probe.resolve(&launch.command).map(|path| (launch, path)));

    let Some((launch, path)) = resolved else {
        let commands = descriptor
            .launches
            .iter()
            .map(|l| l.command.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        // Which of the two sentences the user gets decides which page they go
        // read: install the product, or install its ACP adapter. The CLI is
        // known to be here by now, so this can only be the adapter.
        return match descriptor.cli.first() {
            Some(cli) => finish(
                HarnessStatus::AdapterMissing,
                format!("`{cli}` is installed but its ACP adapter is not (looked for {commands})."),
                descriptor.install_hint.clone(),
                None,
                None,
                Vec::new(),
            ),
            None => finish(
                HarnessStatus::AdapterMissing,
                format!("`{commands}` is not on PATH."),
                descriptor.install_hint.clone(),
                None,
                None,
                Vec::new(),
            ),
        };
    };

    match &descriptor.readiness {
        Readiness::Binary => {}
        Readiness::Inspect {
            kind: InspectKind::Copilot,
        } => {
            // Copilot's CLI *is* the adapter, so reaching here means
            // `copilot` resolved; what is left is version, login, policy,
            // and model.
            let report = super::copilot::classify(&super::copilot::gather(probe));
            return finish(
                report.status,
                report.detail,
                report.remedy,
                Some(launch.clone()),
                Some(path),
                Vec::new(),
            );
        }
        Readiness::Daemon { addr, remedy } => {
            if !probe.listening(addr) {
                return finish(
                    HarnessStatus::DaemonNotRunning,
                    format!(
                        "`{}` is installed, but nothing is listening on {addr}: it is a bridge to a daemon, not the agent itself.",
                        launch.command
                    ),
                    Some(remedy.clone()),
                    Some(launch.clone()),
                    Some(path),
                    Vec::new(),
                );
            }
        }
        Readiness::Command {
            command,
            args,
            on_failure,
            remedy,
        } => {
            let printable = std::iter::once(command.as_str())
                .chain(args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            match probe.run(command, args) {
                ProbeRun::Exit(0) => {}
                ProbeRun::Exit(code) => {
                    return finish(
                        *on_failure,
                        format!("`{printable}` exited {code}."),
                        Some(remedy.clone()),
                        Some(launch.clone()),
                        Some(path),
                        Vec::new(),
                    );
                }
                // A probe we could not run says nothing about the harness. It
                // must not read as "logged out" — that would send the user to
                // re-authenticate something that was never the problem.
                ProbeRun::TimedOut => {
                    return finish(
                        HarnessStatus::Unknown,
                        format!("`{printable}` did not answer in time."),
                        Some(remedy.clone()),
                        Some(launch.clone()),
                        Some(path),
                        Vec::new(),
                    );
                }
                ProbeRun::Failed(err) => {
                    return finish(
                        HarnessStatus::Unknown,
                        format!("could not run `{printable}`: {err}"),
                        Some(remedy.clone()),
                        Some(launch.clone()),
                        Some(path),
                        Vec::new(),
                    );
                }
            }
        }
        Readiness::AuthAndModels { .. } => {
            return diagnose_auth_and_models(finish, probe, descriptor, launch, path);
        }
        Readiness::Inspect {
            kind: InspectKind::Gemini,
        } => {
            let inspected = gemini::inspect(probe, launch);
            if inspected.status != HarnessStatus::Ready {
                return finish(
                    inspected.status,
                    inspected.detail,
                    inspected.remedy,
                    Some(inspected.launch),
                    Some(path),
                    Vec::new(),
                );
            }
            return finish(
                HarnessStatus::Ready,
                inspected.detail,
                None,
                Some(inspected.launch),
                Some(path),
                Vec::new(),
            );
        }
        Readiness::Inspect {
            kind: InspectKind::Cursor,
        } => {
            let cli = descriptor
                .cli
                .iter()
                .find(|name| probe.resolve(name).is_some())
                .cloned()
                .unwrap_or_else(|| launch.command.clone());
            return diagnose_cursor(descriptor, probe, &cli, launch, path, started);
        }
    }

    let detail = if launch.downloads_on_first_run {
        format!(
            "Ready via `{} {}` — the package is fetched on first use.",
            launch.command,
            launch.args.join(" ")
        )
    } else if launch.bundled {
        // `path` here is the Node that runs the adapter, and "Ready —
        // /opt/homebrew/bin/node" answers a question nobody asked. Name the
        // adapter this build ships instead, so a user who is wondering why
        // they never installed anything can see why.
        format!(
            "Ready — the adapter bundled with JaBot ({}).",
            launch
                .args
                .first()
                .map(String::as_str)
                .unwrap_or(launch.command.as_str())
        )
    } else {
        format!("Ready — {}", path.display())
    };
    finish(
        HarnessStatus::Ready,
        detail,
        None,
        Some(launch.clone()),
        Some(path),
        Vec::new(),
    )
}

fn diagnose_auth_and_models(
    finish: impl Fn(
        HarnessStatus,
        String,
        Option<String>,
        Option<Launch>,
        Option<PathBuf>,
        Vec<String>,
    ) -> Diagnosis,
    probe: &dyn ProbeHost,
    descriptor: &HarnessDescriptor,
    launch: &Launch,
    path: PathBuf,
) -> Diagnosis {
    let Readiness::AuthAndModels {
        acp_help_args,
        auth_args,
        models_args,
        logged_out_remedy,
        model_remedy,
        outdated_remedy,
        env_auth_keys,
    } = &descriptor.readiness
    else {
        unreachable!("diagnose_auth_and_models is only called for AuthAndModels");
    };
    let cli = descriptor
        .cli
        .first()
        .map(String::as_str)
        .unwrap_or(launch.command.as_str());
    let help = probe.run_capture(cli, acp_help_args);
    match help.run {
        ProbeRun::Exit(0) => {}
        ProbeRun::Exit(_) | ProbeRun::Failed(_) => {
            return finish(
                HarnessStatus::AdapterOutdated,
                format!("`{cli}` is installed but does not speak ACP (`{cli} acp` is missing)."),
                Some(outdated_remedy.to_string()),
                Some(launch.clone()),
                Some(path),
                Vec::new(),
            );
        }
        ProbeRun::TimedOut => {
            return finish(
                HarnessStatus::Unknown,
                format!("`{cli} acp --help` did not answer in time."),
                Some(outdated_remedy.to_string()),
                Some(launch.clone()),
                Some(path),
                Vec::new(),
            );
        }
    }

    let env_auth = env_auth_keys.iter().any(|key| probe.env_present(key));
    let auth = probe.run_capture(cli, auth_args);
    let auth_ok = match auth.run {
        ProbeRun::Exit(0) if env_auth || !auth_list_empty(&auth.stdout) => true,
        ProbeRun::Exit(0) | ProbeRun::Exit(_) => false,
        ProbeRun::TimedOut => {
            return finish(
                HarnessStatus::Unknown,
                format!("`{cli} auth list` did not answer in time."),
                Some(logged_out_remedy.to_string()),
                Some(launch.clone()),
                Some(path),
                Vec::new(),
            );
        }
        ProbeRun::Failed(err) => {
            return finish(
                HarnessStatus::Unknown,
                format!("could not run `{cli} auth list`: {err}"),
                Some(logged_out_remedy.to_string()),
                Some(launch.clone()),
                Some(path),
                Vec::new(),
            );
        }
    };
    if !auth_ok {
        return finish(
            HarnessStatus::LoggedOut,
            format!("`{cli}` is installed but no provider is signed in."),
            Some(logged_out_remedy.to_string()),
            Some(launch.clone()),
            Some(path),
            Vec::new(),
        );
    }

    let models = probe.run_capture(cli, models_args);
    let listed = parse_models(&models.stdout);
    match models.run {
        ProbeRun::Exit(0) if !listed.is_empty() => finish(
            HarnessStatus::Ready,
            format!("Ready — {} ({} models).", path.display(), listed.len()),
            None,
            Some(launch.clone()),
            Some(path),
            listed,
        ),
        ProbeRun::Exit(0) | ProbeRun::Exit(_) => finish(
            HarnessStatus::InvalidConfig,
            format!("`{cli}` is signed in but no model is available."),
            Some(model_remedy.to_string()),
            Some(launch.clone()),
            Some(path),
            listed,
        ),
        ProbeRun::TimedOut => finish(
            HarnessStatus::Unknown,
            format!("`{cli} models` did not answer in time."),
            Some(model_remedy.to_string()),
            Some(launch.clone()),
            Some(path),
            Vec::new(),
        ),
        ProbeRun::Failed(err) => finish(
            HarnessStatus::Unknown,
            format!("could not run `{cli} models`: {err}"),
            Some(model_remedy.to_string()),
            Some(launch.clone()),
            Some(path),
            Vec::new(),
        ),
    }
}

fn auth_list_empty(stdout: &str) -> bool {
    let text = stdout.trim();
    if text.is_empty() {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    lower.contains("no credential")
        || lower.contains("not authenticated")
        || lower.contains("no provider")
        || lower.contains("0 providers")
        || !text.lines().any(looks_like_provider_row)
}

fn looks_like_provider_row(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("provider")
        || lower.starts_with("name")
        || lower.starts_with("─")
        || lower.starts_with('-')
        || lower.starts_with("id")
    {
        return false;
    }
    trimmed
        .split_whitespace()
        .next()
        .is_some_and(|word| word.chars().any(|c| c.is_ascii_alphabetic()))
}

fn parse_models(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && line.contains('/')
                && !line.to_ascii_lowercase().starts_with("provider")
        })
        .map(str::to_string)
        .collect()
}

fn diagnose_cursor(
    descriptor: &HarnessDescriptor,
    probe: &dyn ProbeHost,
    cli: &str,
    launch: &Launch,
    path: PathBuf,
    started: Instant,
) -> Diagnosis {
    let finish = |status: HarnessStatus, detail: String, remedy: Option<String>| Diagnosis {
        id: descriptor.id.clone(),
        status,
        detail,
        remedy,
        launch: Some(launch.clone()),
        resolved_path: Some(path.clone()),
        elapsed_ms: started.elapsed().as_millis() as u64,
        models: Vec::new(),
    };

    let captured = probe.run_text(cli, &["--version".into()]);
    let version_detail = match &captured.run {
        ProbeRun::Exit(0) => String::new(),
        ProbeRun::Exit(code) => format!("`{cli} --version` exited {code}."),
        ProbeRun::TimedOut => format!("`{cli} --version` did not answer in time."),
        ProbeRun::Failed(err) => format!("could not run `{cli} --version`: {err}"),
    };
    let printed_version = match super::cursor::classify_version(
        matches!(captured.run, ProbeRun::Exit(0)),
        &captured.text,
        version_detail,
    ) {
        super::cursor::VersionCheck::Unsupported { printed, parsed } => {
            return finish(
                HarnessStatus::AdapterOutdated,
                format!(
                    "`{cli}` reports {printed} (parsed {parsed}), which is older than JaBot's Cursor Agent floor."
                ),
                Some("Update the Cursor Agent CLI (`agent update`) and try again.".into()),
            );
        }
        super::cursor::VersionCheck::Unknown(detail) => {
            return finish(
                HarnessStatus::Unknown,
                detail,
                Some("Update the Cursor Agent CLI (`agent update`) so `agent --version` and `agent acp` both work.".into()),
            );
        }
        super::cursor::VersionCheck::Ok(version) => Some(version.to_string()),
        super::cursor::VersionCheck::Unparseable => None,
    };

    if !super::cursor::env_has_cursor_credentials(|key| probe.env(key).is_some()) {
        match probe.run(cli, &["status".into()]) {
            ProbeRun::Exit(0) => {}
            ProbeRun::Exit(code) => {
                return finish(
                    HarnessStatus::LoggedOut,
                    format!("`{cli} status` exited {code}."),
                    Some(
                        "Run `agent login`, or export CURSOR_API_KEY (or CURSOR_AUTH_TOKEN) for this process.".into(),
                    ),
                );
            }
            ProbeRun::TimedOut => {
                return finish(
                    HarnessStatus::Unknown,
                    format!("`{cli} status` did not answer in time."),
                    Some("Run `agent login`, or export CURSOR_API_KEY.".into()),
                );
            }
            ProbeRun::Failed(err) => {
                return finish(
                    HarnessStatus::Unknown,
                    format!("could not run `{cli} status`: {err}"),
                    Some("Run `agent login`, or export CURSOR_API_KEY.".into()),
                );
            }
        }
    }

    match probe.run(cli, &["models".into()]) {
        ProbeRun::Exit(0) => {}
        ProbeRun::Exit(code) => {
            return finish(
                HarnessStatus::InvalidConfig,
                format!("`{cli} models` exited {code} — no usable model for this account."),
                Some(
                    "Pick a model in Cursor (`agent models`) or confirm this account has one available.".into(),
                ),
            );
        }
        ProbeRun::TimedOut => {
            return finish(
                HarnessStatus::Unknown,
                format!("`{cli} models` did not answer in time."),
                Some("Confirm `agent models` lists a model, then retry.".into()),
            );
        }
        ProbeRun::Failed(err) => {
            return finish(
                HarnessStatus::Unknown,
                format!("could not run `{cli} models`: {err}"),
                Some("Confirm `agent models` lists a model, then retry.".into()),
            );
        }
    }

    let version_bit = printed_version.map(|v| format!(" {v}")).unwrap_or_default();
    finish(
        HarnessStatus::Ready,
        format!(
            "Ready{version_bit} — {}. Permissions stay in JaBot (ACP); --force is not passed.",
            path.display()
        ),
        None,
    )
}

/// The real machine: the augmented PATH, real subprocesses, real sockets.
#[derive(Debug, Default)]
pub struct SystemProbe;

impl SystemProbe {
    /// The deadline is a parameter so the kill path can be tested without
    /// waiting out the real one.
    fn run_until(&self, command: &str, args: &[String], timeout: Duration) -> ProbeRun {
        self.run_captured(command, args, timeout, false).run
    }

    fn run_captured(
        &self,
        command: &str,
        args: &[String],
        timeout: Duration,
        capture: bool,
    ) -> ProbeOutput {
        // Resolve first so the child is exec'd from the same augmented PATH
        // the probe searched, and inherit that PATH so a CLI that shells out
        // to `node` finds the same one the terminal would.
        let Some(path) = self.resolve(command) else {
            return ProbeOutput {
                run: ProbeRun::Failed(format!("{command} is not on PATH")),
                stdout: String::new(),
                text: String::new(),
            };
        };
        let mut cmd = Command::new(path);
        cmd.args(args)
            .env("PATH", super::path::joined())
            .stdin(Stdio::null());
        if capture {
            // Help/version still count when the CLI prints to stderr.
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        // A probe is the one command most likely to hang — that is why the
        // user opened the Doctor — and every one of these CLIs is a wrapper
        // that forks work of its own. Killing the pid alone would leave that
        // subtree running for the rest of the session.
        let mut child = match procgroup::spawn(&mut cmd) {
            Ok(child) => child,
            Err(err) => {
                return ProbeOutput {
                    run: ProbeRun::Failed(err.to_string()),
                    stdout: String::new(),
                    text: String::new(),
                }
            }
        };
        let deadline = Instant::now() + timeout;
        let run = loop {
            match child.try_wait() {
                Ok(Some(status)) => break ProbeRun::Exit(status.code().unwrap_or(-1)),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    procgroup::terminate(&mut child);
                    break ProbeRun::TimedOut;
                }
                Err(err) => break ProbeRun::Failed(err.to_string()),
            }
        };
        let text = if capture {
            let mut combined = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                let mut buf = String::new();
                let _ = stdout.read_to_string(&mut buf);
                combined.push_str(&buf);
            }
            if let Some(mut stderr) = child.stderr.take() {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf);
                if !combined.is_empty() && !buf.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&buf);
            }
            combined
        } else {
            String::new()
        };
        ProbeOutput {
            run,
            stdout: text.clone(),
            text,
        }
    }

    fn output_until(
        &self,
        command: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<String, ProbeRun> {
        let Some(path) = self.resolve(command) else {
            return Err(ProbeRun::Failed(format!("{command} is not on PATH")));
        };
        let mut cmd = Command::new(path);
        cmd.args(args)
            .env("PATH", super::path::joined())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match procgroup::spawn(&mut cmd) {
            Ok(child) => child,
            Err(err) => return Err(ProbeRun::Failed(err.to_string())),
        };
        let mut stdout = child.stdout.take();
        let collected = std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut pipe) = stdout.take() {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    procgroup::terminate(&mut child);
                    let _ = collected.join();
                    return Err(ProbeRun::TimedOut);
                }
                Err(err) => {
                    let _ = collected.join();
                    return Err(ProbeRun::Failed(err.to_string()));
                }
            }
        };
        let text = collected.join().unwrap_or_default();
        if status.success() {
            Ok(text)
        } else {
            Err(ProbeRun::Exit(status.code().unwrap_or(-1)))
        }
    }
}

impl ProbeHost for SystemProbe {
    fn resolve(&self, command: &str) -> Option<PathBuf> {
        super::resolve_command(command)
    }

    fn run(&self, command: &str, args: &[String]) -> ProbeRun {
        self.run_until(command, args, PROBE_TIMEOUT)
    }

    fn run_capture(&self, command: &str, args: &[String]) -> ProbeOutput {
        self.run_captured(command, args, PROBE_TIMEOUT, true)
    }

    fn output(&self, command: &str, args: &[String]) -> Result<String, ProbeRun> {
        self.output_until(command, args, PROBE_TIMEOUT)
    }

    fn run_text(&self, command: &str, args: &[String]) -> ProbeOutput {
        self.run_captured(command, args, PROBE_TIMEOUT, true)
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    }

    fn home_file(&self, relative: &str) -> Option<String> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let path = relative.split('/').fold(home, |dir, part| dir.join(part));
        std::fs::read_to_string(path).ok()
    }

    fn listening(&self, addr: &str) -> bool {
        let Ok(mut resolved) = addr.to_socket_addrs() else {
            return false;
        };
        resolved.any(|socket| TcpStream::connect_timeout(&socket, DAEMON_TIMEOUT).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::harness::catalog::compiled_in;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A machine we describe exactly: what is installed, what each probe
    /// command answers, and which ports are open.
    #[derive(Default)]
    struct FakeMachine {
        installed: HashMap<String, PathBuf>,
        exits: HashMap<String, ProbeRun>,
        outputs: HashMap<String, String>,
        env: HashMap<String, String>,
        files: HashMap<String, String>,
        open_ports: Vec<String>,
        delay: Duration,
        calls: Mutex<Vec<String>>,
    }

    impl FakeMachine {
        fn with(installed: &[&str]) -> Self {
            Self {
                installed: installed
                    .iter()
                    .map(|c| ((*c).to_string(), PathBuf::from(format!("/opt/bin/{c}"))))
                    .collect(),
                ..Self::default()
            }
        }

        fn answering(mut self, command: &str, run: ProbeRun) -> Self {
            self.exits.insert(command.to_string(), run);
            self
        }

        fn printing(mut self, command: &str, text: impl Into<String>) -> Self {
            self.outputs.insert(command.to_string(), text.into());
            self
        }

        fn with_env(mut self, key: &str, value: &str) -> Self {
            self.env.insert(key.to_string(), value.to_string());
            self
        }

        fn with_file(mut self, path: &str, body: &str) -> Self {
            self.files.insert(path.to_string(), body.to_string());
            self
        }
    }

    impl ProbeHost for FakeMachine {
        fn resolve(&self, command: &str) -> Option<PathBuf> {
            self.installed.get(command).cloned()
        }

        fn run(&self, command: &str, args: &[String]) -> ProbeRun {
            self.run_capture(command, args).run
        }

        fn run_capture(&self, command: &str, args: &[String]) -> ProbeOutput {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            let invocation = format!("{command} {}", args.join(" "));
            self.calls
                .lock()
                .map(|mut calls| calls.push(invocation.clone()))
                .ok();
            // A command that is not installed cannot be run, and saying
            // otherwise is how a fake hides a real machine's diagnosis behind
            // a cheerful exit 0.
            if !self.installed.contains_key(command) {
                return ProbeOutput {
                    run: ProbeRun::Failed(format!("{command} is not on PATH")),
                    stdout: String::new(),
                    text: String::new(),
                };
            }
            let run = self
                .exits
                .get(&invocation)
                .cloned()
                .or_else(|| self.exits.get(command).cloned())
                .unwrap_or(ProbeRun::Exit(0));
            let captured = self
                .outputs
                .get(&invocation)
                .cloned()
                .or_else(|| self.outputs.get(command).cloned())
                .unwrap_or_default();
            ProbeOutput {
                run,
                stdout: captured.clone(),
                text: captured,
            }
        }

        fn run_text(&self, command: &str, args: &[String]) -> ProbeOutput {
            self.run_capture(command, args)
        }

        fn env_value(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned().filter(|value| !value.is_empty())
        }

        fn env_raw(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }

        fn read_file(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }

        fn listening(&self, addr: &str) -> bool {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            self.open_ports.iter().any(|open| open == addr)
        }

        fn output(&self, command: &str, args: &[String]) -> Result<String, ProbeRun> {
            match self.run(command, args) {
                ProbeRun::Exit(0) => Ok(self.outputs.get(command).cloned().unwrap_or_default()),
                other => Err(other),
            }
        }

        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned().filter(|value| !value.is_empty())
        }

        fn home_file(&self, relative: &str) -> Option<String> {
            self.files.get(relative).cloned()
        }
    }

    fn descriptor(id: &str) -> HarnessDescriptor {
        compiled_in().into_iter().find(|d| d.id == id).unwrap()
    }

    #[test]
    fn nothing_installed_blames_the_cli_not_the_adapter() {
        let machine = FakeMachine::default();
        let report = diagnose(&descriptor("claude"), &machine);
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("claude"), "{}", report.detail);
        assert!(report.remedy.is_some());
    }

    #[test]
    fn aider_without_the_cli_is_missing_aider_not_an_adapter() {
        let report = diagnose(&descriptor("aider"), &FakeMachine::default());
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("aider"), "{}", report.detail);
    }

    struct FakeAiderFacts {
        env: HashMap<String, String>,
        config: Option<String>,
    }

    impl AiderFacts for FakeAiderFacts {
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn home_config(&self) -> Option<String> {
            self.config.clone()
        }
    }

    fn aider_facts_with_key() -> FakeAiderFacts {
        FakeAiderFacts {
            env: HashMap::from([("OPENAI_API_KEY".into(), "sk-test".into())]),
            config: None,
        }
    }

    #[test]
    fn aider_without_a_key_is_logged_out_not_ready() {
        let machine =
            FakeMachine::with(&["aider", "jabot-aider-acp"]).printing("aider", "aider 0.82.2");
        let facts = FakeAiderFacts {
            env: HashMap::new(),
            config: None,
        };
        let report = diagnose_with(&descriptor("aider"), &machine, &facts);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("OPENAI_API_KEY"));
    }

    #[test]
    fn aider_with_cli_adapter_version_and_key_is_ready() {
        let machine =
            FakeMachine::with(&["aider", "jabot-aider-acp"]).printing("aider", "aider 0.82.2");
        let report = diagnose_with(&descriptor("aider"), &machine, &aider_facts_with_key());
        assert_eq!(report.status, HarnessStatus::Ready);
    }

    #[test]
    fn aider_auth_failure_is_named_before_a_missing_adapter() {
        let machine = FakeMachine::with(&["aider"]).printing("aider", "aider 0.82.2");
        let facts = FakeAiderFacts {
            env: HashMap::new(),
            config: None,
        };
        let report = diagnose_with(&descriptor("aider"), &machine, &facts);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
    }

    #[test]
    fn cli_without_its_adapter_is_a_different_problem() {
        let machine = FakeMachine::with(&["claude"]);
        let report = diagnose(&descriptor("claude"), &machine);
        assert_eq!(report.status, HarnessStatus::AdapterMissing);
        assert!(
            report.detail.contains("claude-agent-acp"),
            "{}",
            report.detail
        );
    }

    /// The rename is not an outage: a machine with only the older adapter runs
    /// Claude fine, and the Doctor must resolve to the binary that is there.
    #[test]
    fn the_legacy_adapter_name_still_resolves() {
        let machine = FakeMachine::with(&["claude", "claude-code-acp"]);
        let report = diagnose(&descriptor("claude"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert_eq!(report.launch.unwrap().command, "claude-code-acp");
    }

    #[test]
    fn installed_but_signed_out_says_so() {
        let machine =
            FakeMachine::with(&["codex", "codex-acp"]).answering("codex", ProbeRun::Exit(1));
        let report = diagnose(&descriptor("codex"), &machine);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert_eq!(report.remedy.as_deref(), Some("Run `codex login`."));
    }

    #[test]
    fn opencode_missing_cli_is_not_an_adapter_problem() {
        let report = diagnose(&descriptor("opencode"), &FakeMachine::default());
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("opencode"), "{}", report.detail);
        assert!(report.remedy.unwrap().contains("auth login"));
    }

    #[test]
    fn opencode_without_acp_is_outdated() {
        let machine =
            FakeMachine::with(&["opencode"]).answering("opencode acp --help", ProbeRun::Exit(1));
        let report = diagnose(&descriptor("opencode"), &machine);
        assert_eq!(report.status, HarnessStatus::AdapterOutdated);
        assert!(report.remedy.unwrap().contains("upgrade"));
    }

    #[test]
    fn opencode_without_auth_is_logged_out() {
        let machine = FakeMachine::with(&["opencode"]).printing("opencode auth list", "");
        let report = diagnose(&descriptor("opencode"), &machine);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("auth login"));
    }

    #[test]
    fn opencode_env_key_counts_as_signed_in() {
        let machine = FakeMachine::with(&["opencode"])
            .with_env("ANTHROPIC_API_KEY", "test")
            .printing("opencode auth list", "")
            .printing("opencode models", "anthropic/claude-sonnet-4-5\n");
        let report = diagnose(&descriptor("opencode"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert_eq!(report.models, ["anthropic/claude-sonnet-4-5"]);
    }

    #[test]
    fn opencode_signed_in_without_models_is_a_config_problem() {
        let machine = FakeMachine::with(&["opencode"])
            .printing("opencode auth list", "anthropic  api-key")
            .printing("opencode models", "");
        let report = diagnose(&descriptor("opencode"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("opencode.json"));
    }

    /// Hermes fails `--check` when no provider or model is configured. That is
    /// not "logged out": the fix is `hermes acp --setup`, not a login.
    #[test]
    fn hermes_check_failure_is_a_config_problem() {
        let machine = FakeMachine::with(&["hermes"]).answering("hermes", ProbeRun::Exit(2));
        let report = diagnose(&descriptor("hermes"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("--setup"));
    }

    /// The false-ready case Buzz warns about: `openclaw` on PATH proves
    /// nothing, because the binary is a bridge to a Gateway that is down.
    #[test]
    fn openclaw_on_path_without_its_gateway_is_not_ready() {
        let machine = FakeMachine::with(&["openclaw"]);
        let report = diagnose(&descriptor("openclaw"), &machine);
        assert_eq!(report.status, HarnessStatus::DaemonNotRunning);
        assert!(report.detail.contains("18789"), "{}", report.detail);

        let running = FakeMachine {
            open_ports: vec!["127.0.0.1:18789".into()],
            ..FakeMachine::with(&["openclaw"])
        };
        assert_eq!(
            diagnose(&descriptor("openclaw"), &running).status,
            HarnessStatus::Ready
        );
    }

    /// A probe that hangs must not be reported as a failed login.
    #[test]
    fn an_unanswered_probe_is_unknown_not_logged_out() {
        let machine =
            FakeMachine::with(&["codex", "codex-acp"]).answering("codex", ProbeRun::TimedOut);
        let report = diagnose(&descriptor("codex"), &machine);
        assert_eq!(report.status, HarnessStatus::Unknown);
    }

    /// The adapter being present is not the product being present, and the
    /// difference is what the user has to go and do next: install Claude Code,
    /// not `npm i -g` an adapter they already have. Before the CLI was probed
    /// first this came back as `unknown` with a "sign in" remedy, because
    /// `claude auth status` cannot run without `claude`.
    #[test]
    fn an_adapter_without_its_vendor_cli_blames_the_cli() {
        let machine = FakeMachine::with(&["claude-agent-acp"]);
        let report = diagnose(&descriptor("claude"), &machine);
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("claude"), "{}", report.detail);
        assert!(report.remedy.is_some());
    }

    /// `npx` is on every machine with Node, so `npx -y pi-acp` resolving says
    /// nothing about Pi. Pi's probe is `pi` on PATH
    /// (`setup-porting/findings.md`), and without it the card would claim
    /// ready everywhere.
    #[test]
    fn npx_does_not_make_pi_ready_on_a_machine_without_pi() {
        let machine = FakeMachine::with(&["npx"]);
        let report = diagnose(&descriptor("pi"), &machine);
        assert_eq!(report.status, HarnessStatus::CliMissing);
    }

    #[test]
    fn npx_fallback_is_ready_but_says_it_downloads() {
        let machine = FakeMachine::with(&["pi", "npx"]);
        let report = diagnose(&descriptor("pi"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert!(report.detail.contains("first use"), "{}", report.detail);
    }

    fn gemini_ready() -> FakeMachine {
        FakeMachine::with(&["gemini"])
            .printing("gemini", "Usage: gemini --acp --debug")
            .with_env("GEMINI_API_KEY", "test-key")
    }

    #[test]
    fn gemini_without_the_cli_blames_the_product() {
        let report = diagnose(&descriptor("gemini"), &FakeMachine::default());
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("gemini"), "{}", report.detail);
        assert!(report.remedy.is_some());
    }

    #[test]
    fn gemini_without_acp_flags_is_outdated() {
        let machine = FakeMachine::with(&["gemini"])
            .printing("gemini", "Usage: gemini --prompt --yolo")
            .with_env("GEMINI_API_KEY", "test-key");
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::AdapterOutdated);
        assert!(report.remedy.unwrap().contains("--acp"));
    }

    #[test]
    fn gemini_without_auth_says_logged_out() {
        let machine = FakeMachine::with(&["gemini"]).printing("gemini", "Usage: gemini --acp");
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("GEMINI_API_KEY"));
    }

    #[test]
    fn gemini_account_profile_counts_as_signed_in() {
        let machine = FakeMachine::with(&["gemini"])
            .printing("gemini", "Usage: gemini --acp")
            .with_file(
                ".gemini/settings.json",
                r#"{"security":{"auth":{"selectedType":"oauth-personal"}}}"#,
            );
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert_eq!(report.launch.unwrap().args, ["--acp"]);
    }

    #[test]
    fn gemini_vertex_without_a_project_is_invalid_config() {
        let machine = FakeMachine::with(&["gemini"])
            .printing("gemini", "Usage: gemini --acp")
            .with_file(
                ".gemini/settings.json",
                r#"{"security":{"auth":{"selectedType":"vertex-ai"}}}"#,
            );
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("GOOGLE_CLOUD_PROJECT"));
    }

    #[test]
    fn gemini_empty_model_name_is_invalid_config() {
        let machine = FakeMachine::with(&["gemini"])
            .printing("gemini", "Usage: gemini --acp")
            .with_env("GEMINI_API_KEY", "test-key")
            .with_file(".gemini/settings.json", r#"{"model":{"name":""}}"#);
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("model.name"));
    }

    #[test]
    fn gemini_falls_back_to_the_experimental_flag() {
        let machine = FakeMachine::with(&["gemini"])
            .printing("gemini", "  --experimental-acp  Start ACP mode")
            .with_env("GEMINI_API_KEY", "test-key");
        let report = diagnose(&descriptor("gemini"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert_eq!(report.launch.unwrap().args, ["--experimental-acp"]);
    }

    #[test]
    fn a_ready_gemini_uses_the_documented_acp_flag() {
        let report = diagnose(&descriptor("gemini"), &gemini_ready());
        assert_eq!(report.status, HarnessStatus::Ready);
        assert!(report.detail.contains("--acp"), "{}", report.detail);
    }

    /// The Doctor is the thing a user opens *because* a CLI is hanging, so the
    /// timeout path is the one that has to clean up — and these probes are all
    /// node wrappers that fork work of their own. Killing the pid we spawned
    /// would leave that subtree running with nothing to reap it.
    #[cfg(unix)]
    #[test]
    fn a_probe_that_times_out_takes_its_grandchildren_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grand.pid");
        let script = format!("sleep 30 & echo $! > {}; exec sleep 30", pidfile.display());

        let run = SystemProbe.run_until(
            "sh",
            &["-c".to_string(), script],
            Duration::from_millis(800),
        );
        assert_eq!(run, ProbeRun::TimedOut);

        let grandchild: i32 = std::fs::read_to_string(&pidfile)
            .expect("the probe's grandchild wrote its pid")
            .trim()
            .parse()
            .expect("a pid");
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !crate::host::procgroup::process_alive(grandchild as u32),
            "grandchild {grandchild} outlived the probe that started it"
        );
    }

    /// Serial probing costs the sum of every vendor CLI's latency. With one
    /// slow CLI per card, a serial run of the catalog would take at least
    /// `cards * delay`; this asserts the whole sweep costs about one delay.
    #[test]
    fn probes_run_concurrently() {
        let delay = Duration::from_millis(120);
        let descriptors = compiled_in();
        let machine = FakeMachine {
            delay,
            open_ports: vec!["127.0.0.1:18789".into()],
            ..FakeMachine::with(&[
                "claude",
                "claude-agent-acp",
                "codex",
                "codex-acp",
                "pi",
                "pi-acp",
                "hermes",
                "openclaw",
            ])
        };

        let started = Instant::now();
        let reports = diagnose_all(&descriptors, &machine);
        let elapsed = started.elapsed();

        assert_eq!(reports.len(), descriptors.len());
        let probing: Vec<_> = descriptors
            .iter()
            .filter(|d| !matches!(d.readiness, Readiness::Binary))
            .collect();
        assert!(probing.len() >= 3, "the catalog should have several probes");
        assert!(
            elapsed < delay * probing.len() as u32,
            "sweep took {elapsed:?}; serial would be at least {:?}",
            delay * probing.len() as u32
        );
    }

    #[test]
    fn copilot_without_acp_in_help_is_outdated() {
        let machine = FakeMachine::with(&["copilot"]).printing("copilot", "Usage: copilot login");
        let report = diagnose(&descriptor("copilot"), &machine);
        assert_eq!(report.status, HarnessStatus::AdapterOutdated);
        assert!(report.remedy.unwrap().contains("--acp"));
    }

    #[test]
    fn copilot_with_acp_but_no_login_is_logged_out() {
        let machine = FakeMachine::with(&["copilot"])
            .printing("copilot", "Usage:\n  --acp  Start ACP server\n");
        let report = diagnose(&descriptor("copilot"), &machine);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("copilot login"));
    }

    #[test]
    fn copilot_token_env_makes_it_ready() {
        let machine = FakeMachine::with(&["copilot"])
            .printing("copilot", "  --acp\n")
            .with_env("GH_TOKEN", "gho_test");
        let report = diagnose(&descriptor("copilot"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert!(report.detail.contains("GH_TOKEN"), "{}", report.detail);
    }

    #[test]
    fn copilot_stored_login_makes_it_ready_without_a_token() {
        let machine = FakeMachine::with(&["copilot"])
            .printing("copilot", "  --acp\n")
            .with_env("HOME", "/home/octo")
            .with_file(
                "/home/octo/.copilot/config.json",
                r#"{"loggedInUsers":[{"login":"octocat"}]}"#,
            );
        let report = diagnose(&descriptor("copilot"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        assert!(report.detail.contains("octocat"), "{}", report.detail);
    }

    #[test]
    fn copilot_empty_model_env_is_invalid_config() {
        let machine = FakeMachine::with(&["copilot"])
            .printing("copilot", "  --acp\n")
            .with_env("GH_TOKEN", "gho_test")
            .with_env("COPILOT_MODEL", "");
        let report = diagnose(&descriptor("copilot"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.detail.contains("COPILOT_MODEL"), "{}", report.detail);
    }

    #[test]
    fn cursor_missing_blames_the_cli() {
        let report = diagnose(&descriptor("cursor"), &FakeMachine::default());
        assert_eq!(report.status, HarnessStatus::CliMissing);
        assert!(report.detail.contains("agent"), "{}", report.detail);
        assert!(report.remedy.unwrap().contains("cursor.com/install"));
    }

    #[test]
    fn cursor_legacy_binary_name_still_resolves() {
        let machine = FakeMachine::with(&["cursor-agent"])
            .printing("cursor-agent --version", "2026.08.09")
            .with_env("CURSOR_API_KEY", "test");
        let report = diagnose(&descriptor("cursor"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready);
        let launch = report.launch.unwrap();
        assert_eq!(launch.command, "cursor-agent");
        assert_eq!(launch.args, ["acp"]);
    }

    #[test]
    fn cursor_old_version_is_outdated_not_ready() {
        let machine = FakeMachine::with(&["agent"]).printing("agent --version", "0.49.0");
        let report = diagnose(&descriptor("cursor"), &machine);
        assert_eq!(report.status, HarnessStatus::AdapterOutdated);
        assert!(report.remedy.unwrap().contains("agent update"));
    }

    #[test]
    fn cursor_signed_out_without_an_api_key_says_so() {
        let machine = FakeMachine::with(&["agent"])
            .printing("agent --version", "2026.08.09")
            .answering("agent status", ProbeRun::Exit(1));
        let report = diagnose(&descriptor("cursor"), &machine);
        assert_eq!(report.status, HarnessStatus::LoggedOut);
        assert!(report.remedy.unwrap().contains("agent login"));
    }

    #[test]
    fn cursor_api_key_counts_as_the_account_profile() {
        let machine = FakeMachine::with(&["agent"])
            .printing("agent --version", "2026.08.09")
            .answering("agent status", ProbeRun::Exit(1))
            .with_env("CURSOR_API_KEY", "test-key");
        let report = diagnose(&descriptor("cursor"), &machine);
        assert_eq!(report.status, HarnessStatus::Ready, "{}", report.detail);
    }

    #[test]
    fn cursor_without_models_is_a_config_problem() {
        let machine = FakeMachine::with(&["agent"])
            .printing("agent --version", "2026.08.09")
            .with_env("CURSOR_API_KEY", "test-key")
            .answering("agent models", ProbeRun::Exit(2));
        let report = diagnose(&descriptor("cursor"), &machine);
        assert_eq!(report.status, HarnessStatus::InvalidConfig);
        assert!(report.remedy.unwrap().contains("agent models"));
    }

    #[test]
    fn results_come_back_in_catalog_order() {
        let descriptors = compiled_in();
        let reports = diagnose_all(&descriptors, &FakeMachine::default());
        let ids: Vec<_> = reports.iter().map(|r| r.id.as_str()).collect();
        let expected: Vec<_> = descriptors.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, expected);
    }
}
