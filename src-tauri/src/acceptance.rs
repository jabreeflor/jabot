//! Packaged-app acceptance probe (#235).
//!
//! Active only when `JABOT_ACCEPTANCE_DIR` is set. Writes structured evidence
//! files under that directory so `scripts/macos-acceptance.sh` can assert the
//! native boundary without talking to the user's install.
//!
//! What this is **not**:
//!
//! * A listener inside `JaBot.app`. Decision #4 keeps the shipping host behind
//!   Tauri IPC; `jabot-hostd --listen` is the test binary. Driving a turn
//!   here goes through the same in-process [`HostSession`] the webview uses.
//! * Playwright. A WebKit browser run is renderer compatibility, not WKWebView
//!   + Tauri IPC, and must not be labelled as this gate.
//!
//! Isolation is load-bearing. The process refuses to start the probe if the
//! data directory is the production app-support path or if the Keychain
//! service is still `com.jabot.app`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::host::{
    keychain_service, HostSession, JsonRpcRequest, RequestId, Secrets, HOST_HEALTH, HOST_HELLO,
    KEYCHAIN_SERVICE, SESSION_PROMPT, SESSION_UPDATE,
};

/// Directory the probe writes evidence into.
pub const DIR_ENV: &str = "JABOT_ACCEPTANCE_DIR";
/// Isolated SQLite / identity directory. Required when [`DIR_ENV`] is set.
pub const DATA_DIR_ENV: &str = "JABOT_APP_DATA_DIR";
/// Absolute path of `fake-acp-agent` for the synthetic turn.
pub const FAKE_ACP_ENV: &str = "JABOT_FAKE_ACP_BIN";
/// Prefix every isolated Keychain service must use.
pub const KEYCHAIN_ACCEPTANCE_PREFIX: &str = "com.jabot.app.acceptance.";

const PRODUCTION_APP_SUPPORT: &str = "Library/Application Support/com.jabot.app";
static IPC_ONCE: AtomicBool = AtomicBool::new(false);

/// Whether this process was asked to run the packaged-app probe.
pub fn requested() -> bool {
    std::env::var_os(DIR_ENV).is_some()
}

/// Evidence directory, if acceptance was requested and the path is usable.
pub fn evidence_dir() -> Option<PathBuf> {
    let raw = std::env::var_os(DIR_ENV)?;
    let path = PathBuf::from(raw);
    if path.as_os_str().is_empty() {
        return None;
    }
    Some(path)
}

/// Isolated app-data directory, or `None` when this is a normal launch.
///
/// When acceptance is on, a missing or production path is a hard error: the
/// alternative is writing into the user's real `JaBot.app` data.
pub fn isolated_data_dir() -> Result<Option<PathBuf>, String> {
    if !requested() {
        return Ok(None);
    }
    let raw = std::env::var(DATA_DIR_ENV).map_err(|_| {
        format!("{DATA_DIR_ENV} must be set when {DIR_ENV} is set — refusing to use production app data")
    })?;
    let path = PathBuf::from(raw);
    refuse_production_data_dir(&path)?;
    refuse_production_keychain()?;
    Ok(Some(path))
}

/// Production Application Support path, or anything that resolves to it.
pub fn refuse_production_data_dir(path: &Path) -> Result<(), String> {
    let rendered = path.to_string_lossy();
    if rendered.trim().is_empty() {
        return Err(format!(
            "{DATA_DIR_ENV} is empty — refusing to use production app data"
        ));
    }
    if looks_like_production_app_support(path) {
        return Err(format!(
            "{} is the production app-data path ({PRODUCTION_APP_SUPPORT}); acceptance must use a temp directory",
            path.display()
        ));
    }
    Ok(())
}

/// True for `~/Library/Application Support/com.jabot.app` and the same suffix
/// under any home. A temp dir named `jabot-acceptance-…/data` is not this.
pub fn looks_like_production_app_support(path: &Path) -> bool {
    let rendered = normalize_path(path);
    if rendered.ends_with(PRODUCTION_APP_SUPPORT) {
        // An explicit isolation marker in a parent keeps a checkout that
        // happens to contain those words from being treated as production.
        return !rendered.contains("/jabot-acceptance-")
            && !rendered.contains("/jabot-acceptance/");
    }
    false
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn refuse_production_keychain() -> Result<(), String> {
    let service = keychain_service();
    if service == KEYCHAIN_SERVICE {
        return Err(format!(
            "JABOT_KEYCHAIN_SERVICE is unset or {KEYCHAIN_SERVICE}; acceptance must use {KEYCHAIN_ACCEPTANCE_PREFIX}<id>"
        ));
    }
    if !service.starts_with(KEYCHAIN_ACCEPTANCE_PREFIX) {
        return Err(format!(
            "JABOT_KEYCHAIN_SERVICE={service} must start with {KEYCHAIN_ACCEPTANCE_PREFIX}"
        ));
    }
    Ok(())
}

/// Create the evidence directory and write `launch.json`.
pub fn record_launch(info: LaunchInfo) {
    let Some(dir) = evidence_dir() else {
        return;
    };
    if let Err(err) = prepare_dir(&dir) {
        eprintln!("acceptance: {err}");
        return;
    }
    let payload = json!({
        "cell": "launch",
        "bundleId": info.bundle_id,
        "version": info.version,
        "dataDir": info.data_dir,
        "resourceDir": info.resource_dir,
        "keychainService": keychain_service(),
        "secretsBackend": Secrets::platform().backend().as_str(),
        "notify": notify_snapshot(),
        "bundledAdapter": bundled_adapter_snapshot(),
        "fakeAcp": std::env::var(FAKE_ACP_ENV).ok(),
        "transport": "tauri-ipc",
        "playwrightIsNotAcceptance": true,
    });
    write_json(&dir, "launch.json", &payload);
}

pub struct LaunchInfo {
    pub bundle_id: String,
    pub version: String,
    pub data_dir: String,
    pub resource_dir: Option<String>,
}

/// First `host_rpc` from the webview. The in-process probe never goes through
/// that command, so this file is proof of real Tauri IPC, not of the probe.
pub fn record_ipc(method: &str) {
    if IPC_ONCE.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(dir) = evidence_dir() else {
        return;
    };
    write_json(
        &dir,
        "ipc-connected.json",
        &json!({
            "cell": "tauri-ipc",
            "firstMethod": method,
            "transport": "tauri-ipc",
            "note": "written from host_rpc, which only the webview invoke path calls",
        }),
    );
}

/// Host-side cells that do not need the webview: synthetic turn, Keychain,
/// notify/status, bundled adapter path, quit/relaunch durability.
pub fn spawn_host_probe(state: Arc<Mutex<HostSession>>) {
    if !requested() {
        return;
    }
    thread::Builder::new()
        .name("jabot-acceptance".into())
        .spawn(move || {
            if let Err(err) = run_host_probe(&state) {
                if let Some(dir) = evidence_dir() {
                    write_json(
                        &dir,
                        "host-probe-error.json",
                        &json!({ "ok": false, "error": err }),
                    );
                }
                eprintln!("acceptance: host probe failed: {err}");
            }
        })
        .expect("acceptance probe thread");
}

fn run_host_probe(state: &Mutex<HostSession>) -> Result<(), String> {
    let dir = evidence_dir().ok_or("acceptance dir missing")?;
    prepare_dir(&dir)?;
    write_json(&dir, "notify-status.json", &notify_snapshot());
    write_json(&dir, "bundled-adapter.json", &bundled_adapter_snapshot());
    write_json(&dir, "keychain.json", &keychain_probe()?);

    let previous = read_json(&dir.join("synthetic-turn.json"));
    if let Some(Value::Object(prev)) = previous {
        let thread_id = prev
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or("t-macos-acceptance");
        let durable = durable_reloaded(state, thread_id)?;
        write_json(&dir, "durable-reloaded.json", &durable);
        return Ok(());
    }

    write_json(&dir, "synthetic-turn.json", &synthetic_turn(state)?);
    Ok(())
}

fn synthetic_turn(state: &Mutex<HostSession>) -> Result<Value, String> {
    let fake = std::env::var(FAKE_ACP_ENV)
        .map_err(|_| format!("{FAKE_ACP_ENV} is unset; the script must point at fake-acp-agent"))?;
    if !Path::new(&fake).is_file() {
        return Err(format!("fake-acp-agent is not a file: {fake}"));
    }
    let thread_id = "t-macos-acceptance";
    {
        let mut session = lock_session(state)?;
        let hello =
            session.handle_request(req(1, HOST_HELLO, Some(json!({ "protocolVersion": 1 }))));
        if hello.error.is_some() {
            return Err(format!("host/hello failed: {:?}", hello.error));
        }
        let health = session.handle_request(req(2, HOST_HEALTH, None));
        if health.error.is_some() {
            return Err(format!("host/health failed: {:?}", health.error));
        }
        let prompt = session.handle_request(req(
            3,
            SESSION_PROMPT,
            Some(json!({
                "threadId": thread_id,
                "content": "acceptance synthetic turn",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "runtime": { "command": fake, "args": [] }
            })),
        ));
        if prompt.error.is_some() {
            return Err(format!("session/prompt failed: {:?}", prompt.error));
        }
    }

    let started = Instant::now();
    let mut chunks = Vec::new();
    while started.elapsed() < Duration::from_secs(15) {
        let mut session = lock_session(state)?;
        session.pump_acp();
        for note in session.take_outbound() {
            if note.method == SESSION_UPDATE {
                if let Some(text) = agent_chunk_text(&note.params) {
                    chunks.push(text);
                }
            }
        }
        if chunks
            .iter()
            .any(|text| text.contains("hello from fake-acp"))
        {
            return Ok(json!({
                "cell": "synthetic-turn",
                "ok": true,
                "threadId": thread_id,
                "agentText": chunks,
                "harness": "fake-acp",
            }));
        }
        drop(session);
        thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "synthetic turn timed out waiting for fake-acp; saw {chunks:?}"
    ))
}

fn durable_reloaded(state: &Mutex<HostSession>, thread_id: &str) -> Result<Value, String> {
    let session = lock_session(state)?;
    let store = session
        .store()
        .ok_or("store missing on relaunch — isolated data dir did not reopen")?;
    let row = store
        .get_thread(thread_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("thread {thread_id} missing after relaunch"))?;
    Ok(json!({
        "cell": "quit-relaunch-durability",
        "ok": true,
        "threadId": row.id,
        "title": row.title,
    }))
}

fn keychain_probe() -> Result<Value, String> {
    refuse_production_keychain()?;
    let service = keychain_service();
    let mut vault = Secrets::platform();
    let backend = vault.backend().as_str().to_string();
    let account = "macos-acceptance-probe";
    let secret = "jabot-acceptance-not-a-user-credential";
    match vault.put(account, secret) {
        Ok(()) => {}
        Err(err) => {
            return Ok(json!({
                "cell": "isolated-keychain",
                "ok": false,
                "backend": backend,
                "service": service,
                "error": err.to_string(),
            }));
        }
    }
    let got = vault.get(account).map_err(|err| err.to_string())?;
    let _ = vault.delete(account);
    if got != secret {
        return Err("keychain round-trip returned different bytes".into());
    }
    Ok(json!({
        "cell": "isolated-keychain",
        "ok": true,
        "backend": backend,
        "service": service,
        "account": account,
        "usedProductionService": service == KEYCHAIN_SERVICE,
    }))
}

fn notify_snapshot() -> Value {
    json!({
        "cell": "notify-status",
        "supported": crate::notify::supported(),
        "authorization": crate::notify::authorization().as_str(),
        "kinds": crate::notify::notifying_kinds(),
        "clickToThread": "manual-release",
        "permissionPrompt": "manual-release",
        "d019": "https://github.com/jabreeflor/jabot/issues/73",
    })
}

fn bundled_adapter_snapshot() -> Value {
    let entry = crate::host::harness::bundled::staged_claude_entry();
    json!({
        "cell": "packaged-adapter-resolution",
        "found": entry.is_some(),
        "path": entry.map(|p| p.to_string_lossy().into_owned()),
    })
}

fn agent_chunk_text(params: &Option<Value>) -> Option<String> {
    let params = params.as_ref()?;
    let acp = params.get("acp")?;
    acp.pointer("/update/content/text")
        .or_else(|| acp.pointer("/content/text"))
        .or_else(|| params.pointer("/acp/content/text"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let rendered = params.to_string();
            if rendered.contains("hello from fake-acp") {
                Some("hello from fake-acp".into())
            } else {
                None
            }
        })
}

fn lock_session(
    state: &Mutex<HostSession>,
) -> Result<std::sync::MutexGuard<'_, HostSession>, String> {
    state
        .lock()
        .map_err(|_| "host session lock poisoned".into())
}

fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest::new(RequestId::Number(id), method, params)
}

fn prepare_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))
}

fn write_json(dir: &Path, name: &str, value: &Value) {
    let path = dir.join(name);
    if let Err(err) = fs::write(&path, format!("{}\n", pretty(value))) {
        eprintln!("acceptance: write {}: {err}", path.display());
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read_to_string(path).ok()?;
    serde_json::from_str(&bytes).ok()
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn production_app_support_is_refused() {
        let path = PathBuf::from("/Users/ada/Library/Application Support/com.jabot.app");
        assert!(looks_like_production_app_support(&path));
        assert!(refuse_production_data_dir(&path).is_err());
    }

    #[test]
    fn acceptance_temp_dir_is_allowed() {
        let path = PathBuf::from("/tmp/jabot-acceptance-1a2b/data");
        assert!(!looks_like_production_app_support(&path));
        assert!(refuse_production_data_dir(&path).is_ok());
    }

    #[test]
    fn empty_data_dir_is_refused() {
        assert!(refuse_production_data_dir(Path::new("")).is_err());
        assert!(refuse_production_data_dir(Path::new("   ")).is_err());
    }

    #[test]
    fn production_keychain_service_is_refused_when_unset() {
        if std::env::var("JABOT_KEYCHAIN_SERVICE").is_err() {
            let err = refuse_production_keychain().expect_err("default service is production");
            assert!(err.contains(KEYCHAIN_SERVICE), "{err}");
        }
    }
}
