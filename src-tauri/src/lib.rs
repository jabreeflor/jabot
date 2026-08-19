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
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![host_health])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window must exist");
            window.set_decorations(true)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide-to-Dock: closing the last window hides instead of quitting (#4).
                #[cfg(target_os = "macos")]
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {});
}
