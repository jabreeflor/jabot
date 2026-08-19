//! JaBot host — in-process Rust supervisor inside the Tauri binary.
//!
//! The host API is shaped like a future socket protocol (#8). The webview
//! talks only to these commands and events, never to ACP stdio directly.

mod host;

use host::HostInfo;
use tauri::{Manager, WindowEvent};

/// Health probe for the host API. Returns version and platform metadata.
#[tauri::command]
fn host_health() -> HostInfo {
    host::health()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![host_health])
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
        });
}
