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

use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::super::procgroup;
use super::super::protocol::methods::HarnessStatus;
use super::catalog::{HarnessDescriptor, Launch, Readiness};

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

/// Everything the classifier needs from the machine, behind a seam so the
/// classification rules can be tested without installing five vendor CLIs.
pub trait ProbeHost: Sync {
    fn resolve(&self, command: &str) -> Option<PathBuf>;
    fn run(&self, command: &str, args: &[String]) -> ProbeRun;
    fn listening(&self, addr: &str) -> bool;
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
                })
            })
            .collect()
    })
}

pub fn diagnose(descriptor: &HarnessDescriptor, probe: &dyn ProbeHost) -> Diagnosis {
    let started = Instant::now();
    let finish = |status: HarnessStatus,
                  detail: String,
                  remedy: Option<String>,
                  launch: Option<Launch>,
                  resolved: Option<PathBuf>| Diagnosis {
        id: descriptor.id.clone(),
        status,
        detail,
        remedy,
        launch,
        resolved_path: resolved,
        elapsed_ms: started.elapsed().as_millis() as u64,
    };

    // The vendor CLI is asked about first, before any adapter resolves,
    // because an adapter that is present says nothing about the product it
    // drives. `npx -y pi-acp` resolves on every machine with Node, so without
    // this Pi would report ready on a box with no Pi — the PATH-only false
    // ready this Doctor exists to prevent (`setup-porting/buzz.md` §4). And a
    // readiness command whose binary is absent (`claude auth status` with no
    // `claude`) would come back as an unanswered question with a login remedy
    // the user cannot follow, when the answer was knowable up front.
    if let Some(cli) = descriptor.cli.as_deref() {
        if probe.resolve(cli).is_none() {
            return finish(
                HarnessStatus::CliMissing,
                format!(
                    "{} is not installed — no `{cli}` on PATH.",
                    descriptor.label
                ),
                descriptor.install_hint.clone(),
                None,
                None,
            );
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
        return match descriptor.cli.as_deref() {
            Some(cli) => finish(
                HarnessStatus::AdapterMissing,
                format!("`{cli}` is installed but its ACP adapter is not (looked for {commands})."),
                descriptor.install_hint.clone(),
                None,
                None,
            ),
            None => finish(
                HarnessStatus::AdapterMissing,
                format!("`{commands}` is not on PATH."),
                descriptor.install_hint.clone(),
                None,
                None,
            ),
        };
    };

    match &descriptor.readiness {
        Readiness::Binary => {}
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
                    );
                }
                ProbeRun::Failed(err) => {
                    return finish(
                        HarnessStatus::Unknown,
                        format!("could not run `{printable}`: {err}"),
                        Some(remedy.clone()),
                        Some(launch.clone()),
                        Some(path),
                    );
                }
            }
        }
    }

    let detail = if launch.downloads_on_first_run {
        format!(
            "Ready via `{} {}` — the package is fetched on first use.",
            launch.command,
            launch.args.join(" ")
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
    )
}

/// The real machine: the augmented PATH, real subprocesses, real sockets.
#[derive(Debug, Default)]
pub struct SystemProbe;

impl SystemProbe {
    /// The deadline is a parameter so the kill path can be tested without
    /// waiting out the real one.
    fn run_until(&self, command: &str, args: &[String], timeout: Duration) -> ProbeRun {
        // Resolve first so the child is exec'd from the same augmented PATH
        // the probe searched, and inherit that PATH so a CLI that shells out
        // to `node` finds the same one the terminal would.
        let Some(path) = self.resolve(command) else {
            return ProbeRun::Failed(format!("{command} is not on PATH"));
        };
        let mut cmd = Command::new(path);
        cmd.args(args)
            .env("PATH", super::path::joined())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A probe is the one command most likely to hang — that is why the
        // user opened the Doctor — and every one of these CLIs is a wrapper
        // that forks work of its own. Killing the pid alone would leave that
        // subtree running for the rest of the session.
        procgroup::own_group(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => return ProbeRun::Failed(err.to_string()),
        };
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return ProbeRun::Exit(status.code().unwrap_or(-1)),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    procgroup::terminate(&mut child);
                    return ProbeRun::TimedOut;
                }
                Err(err) => return ProbeRun::Failed(err.to_string()),
            }
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
    }

    impl ProbeHost for FakeMachine {
        fn resolve(&self, command: &str) -> Option<PathBuf> {
            self.installed.get(command).cloned()
        }

        fn run(&self, command: &str, args: &[String]) -> ProbeRun {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            self.calls
                .lock()
                .map(|mut calls| calls.push(format!("{command} {}", args.join(" "))))
                .ok();
            // A command that is not installed cannot be run, and saying
            // otherwise is how a fake hides a real machine's diagnosis behind
            // a cheerful exit 0.
            if !self.installed.contains_key(command) {
                return ProbeRun::Failed(format!("{command} is not on PATH"));
            }
            self.exits
                .get(command)
                .cloned()
                .unwrap_or(ProbeRun::Exit(0))
        }

        fn listening(&self, addr: &str) -> bool {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            self.open_ports.iter().any(|open| open == addr)
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
            !crate::host::procgroup::process_alive(grandchild),
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
    fn results_come_back_in_catalog_order() {
        let descriptors = compiled_in();
        let reports = diagnose_all(&descriptors, &FakeMachine::default());
        let ids: Vec<_> = reports.iter().map(|r| r.id.as_str()).collect();
        let expected: Vec<_> = descriptors.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, expected);
    }
}
