//! The ACP adapters that ship inside JaBot.app.
//!
//! The catalog's rule is that the host never installs anything
//! (`harness/mod.rs`), and that stays true: this does not download, unpack or
//! write. It looks for an adapter that the *build* already put inside the app
//! bundle and, when it is there, offers it as one more launch candidate.
//!
//! It exists because "install Claude Code, then `npm i -g` an adapter" is two
//! installs for one product, and a user who has done the first and not the
//! second sees `Harness unavailable: claude-agent-acp` on every launch with no
//! way out of it inside the app. `scripts/bundle-adapters.sh` stages the
//! adapter, `bundle.resources` in tauri.conf.json copies it, and this finds it.
//!
//! Two things gate the bundled launch, and both are the point:
//!
//! * **Node.** The adapter is a JavaScript program. We ship the program, not a
//!   runtime for it — a second Node inside the DMG would be larger than the
//!   rest of the app. No `node` on the augmented PATH, no bundled candidate,
//!   and the card falls back to whatever the user installed themselves.
//! * **`claude`.** The staged tree deliberately omits
//!   `@anthropic-ai/claude-agent-sdk`'s per-platform binaries, each a ~200 MB
//!   copy of Claude Code, because the Claude card drives the Claude Code the
//!   user already has. `CLAUDE_CODE_EXECUTABLE` is how the adapter is told to
//!   use it; without that the SDK raises "Native CLI binary not found". So the
//!   env var is part of the launch rather than a default, and the launch is
//!   only offered once `claude` resolves.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::catalog::Launch;

/// An override for the search below, for a machine that keeps the adapters
/// somewhere else and for driving `bundled.rs` from a shell.
pub const RESOURCE_DIR_ENV: &str = "JABOT_BUNDLED_ADAPTERS_DIR";

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where Tauri resolved this build's resources to.
///
/// The host is in-process inside the app (decision #4) but holds no
/// `AppHandle`, and `jabot-hostd` has no Tauri at all — so `lib.rs` hands the
/// path over here during setup rather than the catalog reaching for one.
/// Deliberately not a `set_var`: the process is already multi-threaded by
/// then, and mutating the environment underneath a concurrent `getenv` is the
/// kind of bug that reproduces once a month on one machine.
///
/// First call wins, and a later one is ignored — [`claude_launch`] caches, so
/// a second answer would only be true for whoever asked first anyway.
pub fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

/// Where the staged tree sits under a root we have been handed. `resources`
/// entries keep their repo-relative path in the bundle, and a future move to a
/// flatter layout should not need a coordinated release, so both are tried.
const RELATIVE_ROOTS: &[&str] = &["vendor/adapters", "adapters", "."];

/// The adapter, relative to whichever of [`RELATIVE_ROOTS`] resolved.
const CLAUDE_ENTRY: &str = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";

/// The adapter reads this before falling back to the SDK's own native binary
/// (`claudeCliPath()` in `dist/acp-agent.js`).
const CLAUDE_EXECUTABLE_ENV: &str = "CLAUDE_CODE_EXECUTABLE";

/// Every bundled candidate for a catalog id, in the order they should be
/// tried. Empty for a card this build ships nothing for — which is Codex and
/// Pi today, for the reasons in `scripts/bundle-adapters.sh`.
pub fn launches_for(id: &str) -> Vec<Launch> {
    match id {
        "claude" => claude_launch().cloned().into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The bundled Claude adapter, if this build has one and the machine can run
/// it. Resolved once: it is three `stat` calls and a PATH walk, and a catalog
/// that answered differently between two probes in one session would make the
/// Doctor and the spawner disagree about what is installed.
pub fn claude_launch() -> Option<&'static Launch> {
    static CACHE: OnceLock<Option<Launch>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            claude_launch_with(
                entry(CLAUDE_ENTRY).as_deref(),
                super::resolve_command("node").as_deref(),
                super::resolve_command("claude").as_deref(),
            )
        })
        .as_ref()
}

/// The launch itself, with every question about this machine already answered
/// — so the rules above are testable without a Node, a `claude`, or an app
/// bundle to put them in.
pub fn claude_launch_with(
    entry: Option<&Path>,
    node: Option<&Path>,
    claude: Option<&Path>,
) -> Option<Launch> {
    let (entry, node, claude) = (entry?, node?, claude?);
    let mut env = BTreeMap::new();
    // The absolute path, not the bare name: the adapter hands this to the SDK,
    // which spawns it, and a name would be resolved against whatever PATH that
    // subprocess ended up with rather than the augmented one the Doctor probed.
    env.insert(
        CLAUDE_EXECUTABLE_ENV.to_string(),
        claude.display().to_string(),
    );
    Some(Launch::bundled(
        &node.display().to_string(),
        &[&entry.display().to_string()],
        env,
    ))
}

/// The staged adapter's entry script, or `None` when this build has none.
fn entry(relative: &str) -> Option<PathBuf> {
    let resource_dir = RESOURCE_DIR
        .get()
        .cloned()
        .or_else(|| std::env::var_os(RESOURCE_DIR_ENV).map(PathBuf::from));
    entry_in(
        &roots(
            resource_dir.as_deref(),
            std::env::current_exe().ok().as_deref(),
        ),
        relative,
    )
}

fn entry_in(roots: &[PathBuf], relative: &str) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join(relative))
        .find(|path| path.is_file())
}

/// Every directory the staged tree might be under, best first.
///
/// Both inputs are parameters rather than reads of the process environment so
/// the layout rules can be tested without a `set_var`, which is racy under a
/// parallel test runner.
fn roots(resource_dir: Option<&Path>, exe: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut push = |dir: PathBuf| {
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    };

    if let Some(dir) = resource_dir {
        for relative in RELATIVE_ROOTS {
            push(dir.join(relative));
        }
    }

    // The .app layout, for the case where the env var never arrived: the
    // binary is `Contents/MacOS/JaBot` and resources are `Contents/Resources`.
    if let Some(dir) = exe.and_then(Path::parent) {
        for relative in RELATIVE_ROOTS {
            push(dir.join("../Resources").join(relative));
            push(dir.join(relative));
        }
    }

    // The checkout, for `npm run tauri dev`, `scripts/live.sh` and the tests.
    // Compiled out of a release build, so a shipped app can never be pointed
    // at a developer's tree by whatever happens to exist at that path.
    if cfg!(debug_assertions) {
        push(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/adapters"
        )));
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (PathBuf, PathBuf, PathBuf) {
        (
            PathBuf::from("/Applications/JaBot.app/Contents/Resources/vendor/adapters")
                .join(CLAUDE_ENTRY),
            PathBuf::from("/opt/homebrew/bin/node"),
            PathBuf::from("/opt/homebrew/bin/claude"),
        )
    }

    #[test]
    fn a_bundled_launch_runs_the_staged_script_with_node() {
        let (entry, node, claude) = paths();
        let launch = claude_launch_with(Some(&entry), Some(&node), Some(&claude))
            .expect("everything resolved");
        assert_eq!(launch.command, node.display().to_string());
        assert_eq!(launch.args, vec![entry.display().to_string()]);
        assert!(launch.bundled);
        assert!(!launch.downloads_on_first_run);
    }

    /// The reason the platform binaries are omitted from the staged tree. The
    /// adapter would otherwise look for a 200 MB copy of Claude Code that this
    /// build does not ship, and fail at the first prompt rather than at launch.
    #[test]
    fn the_launch_points_the_adapter_at_the_installed_claude() {
        let (entry, node, claude) = paths();
        let launch = claude_launch_with(Some(&entry), Some(&node), Some(&claude)).unwrap();
        assert_eq!(
            launch.env.get(CLAUDE_EXECUTABLE_ENV).map(String::as_str),
            Some(claude.display().to_string().as_str())
        );
    }

    /// Each of the three is a different reason this machine cannot run the
    /// bundled copy, and every one of them means "offer nothing" rather than
    /// "offer a command that will fail".
    #[test]
    fn a_missing_piece_offers_nothing() {
        let (entry, node, claude) = paths();
        assert!(claude_launch_with(None, Some(&node), Some(&claude)).is_none());
        assert!(claude_launch_with(Some(&entry), None, Some(&claude)).is_none());
        assert!(claude_launch_with(Some(&entry), Some(&node), None).is_none());
    }

    /// Tauri's resource directory is what the shipping app has, and it is
    /// asked first — an app bundle must never resolve its adapter out of
    /// whatever happens to sit next to the binary.
    #[test]
    fn the_resource_dir_is_searched_before_the_executables_directory() {
        let resources = PathBuf::from("/Applications/JaBot.app/Contents/Resources");
        let exe = PathBuf::from("/Applications/JaBot.app/Contents/MacOS/JaBot");
        let roots = roots(Some(&resources), Some(&exe));
        assert_eq!(roots[0], resources.join("vendor/adapters"));
        assert!(roots.contains(&exe.parent().unwrap().join("../Resources/vendor/adapters")));
    }

    /// `roots()` is not used here: its last entry is the checkout's own staged
    /// tree, which exists on a developer's machine and not in CI, and a test
    /// that answers differently in the two places is not a test.
    #[test]
    fn the_entry_is_found_under_a_staged_root_and_nowhere_else() {
        let dir = tempfile::tempdir().unwrap();
        let candidates = vec![dir.path().join("vendor/adapters")];
        let staged = candidates[0].join(CLAUDE_ENTRY);
        assert!(entry_in(&candidates, CLAUDE_ENTRY).is_none());

        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::write(&staged, "#!/usr/bin/env node\n").unwrap();
        assert_eq!(
            entry_in(&candidates, CLAUDE_ENTRY).as_deref(),
            Some(staged.as_path())
        );
    }
}
