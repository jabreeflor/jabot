//! Shared bits for crate integration tests.

use std::path::PathBuf;

/// Path to the `fake-acp-agent` binary this crate just built.
///
/// `CARGO_BIN_EXE_fake_acp_agent` is the usual cargo answer, but
/// `cargo llvm-cov` compiles tests under `target/llvm-cov-target/` and
/// often leaves that env unset (or pointing at a path that was never
/// written). Fall back through the instrumented target dir,
/// `CARGO_TARGET_DIR`, and the directory next to this test binary
/// before the historical `target/debug/` default.
pub fn fake_agent() -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("CARGO_BIN_EXE_fake_acp_agent") {
        candidates.push(PathBuf::from(path));
    }
    // Same target dir that built *this* test. Prefer it over a leftover
    // `llvm-cov-target` binary from an earlier coverage run — that stale
    // copy may not speak the modes the current suite asks for.
    let bin = format!("fake-acp-agent{}", std::env::consts::EXE_SUFFIX);
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&bin));
            if let Some(debug_dir) = dir.parent() {
                candidates.push(debug_dir.join(&bin));
            }
        }
    }
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(dir).join("debug").join(&bin));
    }
    candidates.push(manifest.join("target/llvm-cov-target/debug").join(&bin));
    candidates.push(manifest.join("target/debug").join(&bin));
    candidates.push(manifest.join("../target/debug").join(&bin));

    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| manifest.join("target/debug").join(&bin))
        .to_string_lossy()
        .into_owned()
}
