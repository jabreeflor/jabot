//! JaBot host — in-process Rust supervisor inside the Tauri binary.
//!
//! The webview talks JSON-RPC to these commands and events, never to ACP
//! stdio. The message types are the future Unix-socket / WebSocket frames.

pub mod acceptance;
pub mod host;
pub mod notify;
pub mod window;

pub use host::{
    HostSession, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, NewThread, RequestId, Store,
    StoreError, ThreadRepo, ThreadRow, HOST_HELLO, PERMISSION_ASK, PERMISSION_PENDING,
    PERMISSION_REPLY, PERMISSION_RESOLVED, PROTOCOL_VERSION, SESSION_CANCEL, SESSION_PROMPT,
    SESSION_UPDATE,
};

/// stdio ACP adapter that drives Aider's scripting CLI (`jabot --aider-acp`).
pub fn run_aider_acp() -> i32 {
    host::harness::aider_acp::run()
}

use std::sync::{Arc, Mutex};
use std::time::Duration;

use host::AdapterWake;
use tauri::{Emitter, Manager, State, WindowEvent};

struct HostState(Arc<Mutex<HostSession>>);

/// JSON-RPC 2.0 request/response. Same payload a socket transport will frame.
#[tauri::command]
fn host_rpc(
    app: tauri::AppHandle,
    state: State<HostState>,
    request: JsonRpcRequest,
) -> JsonRpcResponse {
    // The probe talks to HostSession directly. This command is the webview's
    // only path, so the first call here is the Tauri IPC cell (#235).
    acceptance::record_ipc(&request.method);
    let mut session = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let response = session.handle_request(request);
    session.pump_acp();
    let outbound = session.take_outbound();
    // Emitted while the lock is held, and `spawn_acp_pump` does the same. Two
    // drainers that release first can interleave their emits, and the webview
    // would see a thread's `seq` 3 before its `seq` 1 — the one thing the
    // envelope's counter is there to rule out (#14 de-duplicates a stored
    // replay against the live stream with it).
    for notification in &outbound {
        emit_host_notification(&app, notification);
    }
    drop(session);
    // OS banners are last and can block: Windows `Show` is synchronous.
    // Do not hold `HostState` across it — the ACP pump and the next RPC
    // must not wait on Action Center.
    announce_host_notifications(&outbound);
    response
}

fn emit_host_notification(app: &tauri::AppHandle, notification: &JsonRpcNotification) {
    if let Err(err) = app.emit("host-rpc", notification) {
        eprintln!("failed to emit host-rpc notification: {err}");
    }
}

/// Persist, then notify — and the OS banner is the *last* step of the
/// second half (#27). The `inbox_events` row was written before this
/// notification was queued, and the webview has just been told, so a
/// refused permission or a machine with no Notification Center costs
/// nothing but the banner. `announce` decides on its own which frames
/// deserve one; almost none do.
fn announce_host_notifications(outbound: &[JsonRpcNotification]) {
    for notification in outbound {
        if let Some(params) = notification.params.as_ref() {
            notify::announce(&notification.method, params);
        }
    }
}

/// Clicking a banner opens the thread it names (#27).
///
/// In this order: bring the window back first, because the click nearly always
/// arrives while JaBot is hidden in the Dock (#4), and only then tell the
/// renderer which thread to select. The other order lands a selection on a
/// webview nobody can see.
fn route_notification_clicks(app: tauri::AppHandle) {
    notify::on_click(move |open| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        if let Err(err) = app.emit(notify::ACTIVATED_EVENT, open) {
            eprintln!("failed to emit a notification click: {err}");
        }
    });
}

/// Tell the harness catalog where this build's bundled ACP adapters are.
///
/// Tauri is the only thing that knows where `Contents/Resources` ended up, and
/// the harness catalog is what needs it — so the path is handed over once,
/// here, before the session loads. It has to be before: the first thing a
/// loaded session does is mirror the catalog into the store.
///
/// A build with nothing staged simply resolves nothing. `bundled.rs` looks for
/// the adapter itself and offers no candidate when it is not there, so this is
/// never the thing that decides whether the Claude card works.
fn point_at_bundled_adapters(app: &tauri::AppHandle) {
    match app.path().resource_dir() {
        Ok(dir) => host::harness::bundled::set_resource_dir(dir),
        Err(err) => eprintln!("failed to resolve the resource dir: {err}; bundled adapters will only be found next to the executable"),
    }
}

fn load_session(app: &tauri::AppHandle) -> HostSession {
    match acceptance::isolated_data_dir() {
        Ok(Some(dir)) => {
            if let Err(err) = std::fs::create_dir_all(&dir) {
                eprintln!(
                    "failed to create isolated app data dir {}: {err}",
                    dir.display()
                );
                return HostSession::ephemeral();
            }
            return HostSession::load(&dir);
        }
        Ok(None) => {}
        Err(err) => {
            // Isolation failed closed: do not fall through to the user's
            // production app-support directory (#235).
            eprintln!("acceptance isolation refused: {err}");
            std::process::exit(2);
        }
    }
    match app.path().app_data_dir() {
        Ok(dir) => {
            if let Err(err) = std::fs::create_dir_all(&dir) {
                eprintln!("failed to create app data dir {}: {err}", dir.display());
                return HostSession::ephemeral();
            }
            HostSession::load(&dir)
        }
        Err(err) => {
            eprintln!("failed to resolve app data dir: {err}; using ephemeral identity");
            HostSession::ephemeral()
        }
    }
}

fn spawn_acp_pump(app: tauri::AppHandle, wake: std::sync::Arc<AdapterWake>) {
    std::thread::Builder::new()
        .name("jabot-acp-pump".into())
        .spawn(move || loop {
            wake.wait_timeout(Duration::from_millis(250));
            let Some(state) = app.try_state::<HostState>() else {
                break;
            };
            let mut session = match state.0.lock() {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
            session.pump_acp();
            let outbound = session.take_outbound();
            // Under the lock, for the ordering reason in `host_rpc`.
            for notification in &outbound {
                emit_host_notification(&app, notification);
            }
            drop(session);
            announce_host_notifications(&outbound);
        })
        .expect("acp pump thread");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // The signed release feed (#12) is only consumed if the plugin is
            // registered; the endpoint and pubkey in tauri.conf.json are read
            // by the bundler, never at runtime. macOS-only because the
            // dependency is (see src-tauri/Cargo.toml).
            #[cfg(target_os = "macos")]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            point_at_bundled_adapters(app.handle());
            let session = load_session(app.handle());
            let wake = session.adapter_wake();
            let state = Arc::new(Mutex::new(session));
            if acceptance::requested() {
                let data_dir = acceptance::isolated_data_dir()
                    .ok()
                    .flatten()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                acceptance::record_launch(acceptance::LaunchInfo {
                    bundle_id: app.config().identifier.clone(),
                    version: app.package_info().version.to_string(),
                    data_dir,
                    resource_dir: app
                        .path()
                        .resource_dir()
                        .ok()
                        .map(|p| p.display().to_string()),
                });
                acceptance::spawn_host_probe(Arc::clone(&state));
            }
            app.manage(HostState(Arc::clone(&state)));
            spawn_acp_pump(app.handle().clone(), wake);
            // Ask for notification permission and start listening for clicks
            // (#27, #284). A refusal is not an error: the Inbox is the record
            // and this is only the tap on the shoulder. Off macOS and Windows
            // `install` is a genuine no-op, and no click can ever arrive to
            // reach the sink. On Windows a toast click uses the same sink.
            route_notification_clicks(app.handle().clone());
            notify::install();
            // Under-window vibrancy (#250). False off macOS and when the
            // material cannot be applied; the renderer stays opaque then.
            window::apply(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            host_rpc,
            window::window_translucency_applied,
            host::repo::workspace::pick_workspace,
            host::repo::workspace::pick_sources,
            host::repo::workspace::github_repositories,
            host::repo::workspace::clone_repository,
            host::repo::workspace::scratch_workspace,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide-to-Dock (macOS only, MVP per #4): closing the last window hides
                // instead of quitting. On other platforms, close quits the app.
                #[cfg(target_os = "macos")]
                {
                    api.prevent_close();
                    if let Err(err) = window.hide() {
                        eprintln!("failed to hide main window: {err}");
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    // Non-macOS closes for real; nothing to intercept.
                    let _ = (window, api);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<HostState>() {
                    if let Ok(mut session) = state.0.lock() {
                        session.shutdown_adapters();
                        session.checkpoint_store();
                    }
                }
            }
        });
}
