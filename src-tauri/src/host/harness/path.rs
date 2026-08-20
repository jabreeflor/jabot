//! The PATH a GUI app has to build for itself.
//!
//! A macOS `.app` launched from Finder or the Dock inherits launchd's
//! environment, not the login shell's: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`.
//! Every harness people install through Homebrew, npm, pipx, or nvm is
//! therefore invisible, which is the whole of "it works in my terminal but
//! JaBot says it is not installed". Buzz merges the login-shell PATH,
//! `~/.local/bin` and nvm for the same reason
//! (`docs/research/setup-porting/buzz.md` §4).
//!
//! The augmented list is used twice: to *probe* (so the Doctor sees what the
//! terminal sees) and to *spawn* (so an adapter can find `node`, `git`, and
//! the vendor CLI it shells out to).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// How long the login shell gets. A shell that hangs on its own rc files must
/// not hang JaBot's first PATH probe.
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_millis(1500);

/// Directories people install CLIs into that a Finder launch never sees.
const WELL_KNOWN: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/homebrew/sbin",
    "/usr/local/sbin",
];

/// Under `$HOME`, in preference order.
const HOME_RELATIVE: &[&str] = &[".local/bin", "bin", ".cargo/bin", ".bun/bin"];

/// The search path, resolved once per host process.
///
/// Cached because the expensive part is a login shell, and because a PATH that
/// changed halfway through a session would make two probes of the same harness
/// disagree for no reason the user could see.
pub fn search_path() -> &'static [PathBuf] {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(|| {
        augment(
            std::env::var_os("PATH").as_deref(),
            login_shell_path().as_deref(),
            home_dir().as_deref(),
        )
    })
}

/// The same list as a `PATH` value for a child process.
pub fn joined() -> OsString {
    std::env::join_paths(search_path()).unwrap_or_else(|_| {
        // join_paths only fails on a directory containing the separator; fall
        // back to the process PATH rather than handing the child nothing.
        std::env::var_os("PATH").unwrap_or_default()
    })
}

/// Merge the inputs, first occurrence wins.
///
/// Process PATH first: an explicitly exported PATH is a deliberate choice and
/// stays authoritative. The login shell and the well-known directories are
/// additions for the launchd case, not replacements.
pub fn augment(
    process_path: Option<&OsStr>,
    login_shell_path: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if dir.as_os_str().is_empty() || out.contains(&dir) {
            return;
        }
        out.push(dir);
    };

    if let Some(path) = process_path {
        for dir in std::env::split_paths(path) {
            push(dir);
        }
    }
    if let Some(path) = login_shell_path {
        for dir in std::env::split_paths(path) {
            push(dir);
        }
    }
    if let Some(home) = home {
        for suffix in HOME_RELATIVE {
            push(home.join(suffix));
        }
        for dir in nvm_bin_dirs(home) {
            push(dir);
        }
    }
    for dir in WELL_KNOWN {
        push(PathBuf::from(dir));
    }
    out
}

/// nvm keeps every installed Node under `~/.nvm/versions/node/<version>/bin`
/// and puts the selected one on PATH from `.zshrc` — which a Finder launch
/// never runs. Newest first, so an adapter shipped as an npm package gets the
/// most recent runtime rather than whichever version sorts first as a string
/// (`v9` is not newer than `v20`).
pub fn nvm_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".nvm/versions/node");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u32>, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let bin = entry.path().join("bin");
            if !bin.is_dir() {
                return None;
            }
            let name = entry.file_name();
            Some((parse_version(&name.to_string_lossy()), bin))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, bin)| bin).collect()
}

fn parse_version(name: &str) -> Vec<u32> {
    name.trim_start_matches('v')
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Ask the user's login shell what PATH it would have.
///
/// macOS only: it is the only platform where the app is routinely started
/// without a shell in its ancestry, and on Linux/CI spawning a login shell
/// during a probe buys nothing and costs a process.
fn login_shell_path() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let shell = std::env::var("SHELL").ok()?;
    if shell.trim().is_empty() {
        return None;
    }
    // `-l -c` so rc files that set PATH actually run; `printf` rather than
    // `echo` so nothing adds a trailing newline of its own.
    let mut child = Command::new(&shell)
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + LOGIN_SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_shell_and_home_dirs_are_added_to_a_launchd_path() {
        let home = tempfile::tempdir().unwrap();
        let merged = augment(
            Some(OsStr::new("/usr/bin:/bin")),
            Some("/opt/homebrew/bin:/usr/bin"),
            Some(home.path()),
        );

        // The launchd PATH keeps its priority...
        assert_eq!(merged[0], PathBuf::from("/usr/bin"));
        // ...and everything the terminal had is reachable now.
        assert!(merged.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(merged.contains(&home.path().join(".local/bin")));
        assert!(merged.contains(&home.path().join(".cargo/bin")));
    }

    #[test]
    fn duplicates_collapse_to_the_first_occurrence() {
        let merged = augment(
            Some(OsStr::new("/usr/bin:/bin")),
            Some("/bin:/usr/bin:/opt/homebrew/bin"),
            None,
        );
        let usr_bin = merged
            .iter()
            .filter(|dir| *dir == &PathBuf::from("/usr/bin"))
            .count();
        assert_eq!(usr_bin, 1, "{merged:?}");
        assert_eq!(merged[1], PathBuf::from("/bin"));
    }

    /// The reason this is not a string sort: nvm's directories are `v9.11.2`,
    /// `v20.11.0`, and a lexicographic newest-first would hand an npm-shipped
    /// adapter the oldest Node on the machine.
    #[test]
    fn nvm_versions_come_out_newest_first() {
        let home = tempfile::tempdir().unwrap();
        for version in ["v9.11.2", "v20.11.0", "v18.20.4"] {
            std::fs::create_dir_all(
                home.path()
                    .join(".nvm/versions/node")
                    .join(version)
                    .join("bin"),
            )
            .unwrap();
        }
        let dirs = nvm_bin_dirs(home.path());
        let names: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.parent()?.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(names, ["v20.11.0", "v18.20.4", "v9.11.2"]);
    }

    #[test]
    fn a_missing_nvm_is_not_an_error() {
        let home = tempfile::tempdir().unwrap();
        assert!(nvm_bin_dirs(home.path()).is_empty());
    }
}
