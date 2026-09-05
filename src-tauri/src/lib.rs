//! JaBot host — in-process Rust supervisor inside the Tauri binary.
//!
//! The webview talks JSON-RPC to these commands and events, never to ACP
//! stdio. The message types are the future Unix-socket / WebSocket frames.

pub mod host;
pub mod notify;

pub use host::{
    HostSession, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, NewThread, RequestId, Store,
    StoreError, ThreadRepo, ThreadRow, HOST_HELLO, PERMISSION_ASK, PERMISSION_PENDING,
    PERMISSION_REPLY, PERMISSION_RESOLVED, PROTOCOL_VERSION, SESSION_CANCEL, SESSION_PROMPT,
    SESSION_UPDATE,
};

use std::sync::Mutex;
use std::time::Duration;

use host::AdapterWake;
use tauri::{Emitter, Manager, State, WindowEvent};

struct HostState(Mutex<HostSession>);

/// JSON-RPC 2.0 request/response. Same payload a socket transport will frame.
#[tauri::command]
fn host_rpc(
    app: tauri::AppHandle,
    state: State<HostState>,
    request: JsonRpcRequest,
) -> JsonRpcResponse {
    let mut session = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let response = session.handle_request(request);
    session.pump_acp();
    let outbound = session.take_outbound();
    // Emitted while the lock is held, and `spawn_acp_pump` does the same. Two
    // drainers that release first can interleave their emits, and the webview
    // would see a thread's `seq` 3 before its `seq` 1 — the one thing the
    // envelope's counter is there to rule out (#14 de-duplicates a stored
    // replay against the live stream with it).
    for notification in outbound {
        emit_host_notification(&app, &notification);
    }
    drop(session);
    response
}

fn emit_host_notification(app: &tauri::AppHandle, notification: &JsonRpcNotification) {
    if let Err(err) = app.emit("host-rpc", notification) {
        eprintln!("failed to emit host-rpc notification: {err}");
    }
    // Persist, then notify — and the OS banner is the *last* step of the
    // second half (#27). The `inbox_events` row was written before this
    // notification was queued, and the webview has just been told, so a
    // refused permission or a machine with no Notification Center costs
    // nothing but the banner. `announce` decides on its own which frames
    // deserve one; almost none do.
    if let Some(params) = notification.params.as_ref() {
        notify::announce(&notification.method, params);
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

fn load_session(app: &tauri::AppHandle) -> HostSession {
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
            for notification in outbound {
                emit_host_notification(&app, &notification);
            }
            drop(session);
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
            let session = load_session(app.handle());
            let wake = session.adapter_wake();
            app.manage(HostState(Mutex::new(session)));
            spawn_acp_pump(app.handle().clone(), wake);
            // Ask for notification permission and start listening for clicks
            // (#27). A refusal is not an error: the Inbox is the record and
            // this is only the tap on the shoulder. Off macOS `install` is a
            // genuine no-op, and no click can ever arrive to reach the sink.
            route_notification_clicks(app.handle().clone());
            notify::install();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            host_rpc,
            host::repo::workspace::pick_workspace,
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
