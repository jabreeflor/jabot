//! Self-update from the GitHub release feed.
//!
//! Every push to `main` publishes a release (`.github/workflows/release.yml`),
//! and this is the half that makes an installed JaBot notice. Registering the
//! updater plugin (lib.rs) only loads the endpoint and pubkey; nothing checks
//! until something here asks.
//!
//! The flow, all on the Rust side:
//!
//! 1. Ten seconds after launch, then every four hours — and whenever the
//!    user picks **JaBot → Check for Updates…** — ask the feed.
//! 2. A newer version is downloaded into memory and its signature checked
//!    against the pubkey compiled into this binary. Nothing on disk changes.
//! 3. The renderer is told (`app-update`, status `ready`) and shows a banner.
//!    **Restart** installs and relaunches; quitting installs on the way out,
//!    so the next launch is the new version either way.
//!
//! Staging in memory instead of installing straight away is deliberate: the
//! install swaps the whole `.app` bundle, and a running JaBot whose
//! `Contents/Resources` (the bundled ACP adapters) changed underneath it
//! would spawn adapters from a version it is not.
//!
//! Only the app that ships runs any of this. JaBot Local
//! (`tauri.local.conf.json`, `com.jabot.app.local`) is built from a checkout
//! and must never replace itself with whatever is on `main`.

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::time::Duration;

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri::{AppHandle, Manager, State};

/// The bundle identifier of the released app (`tauri.conf.json`). Pinned by
/// `scripts/tests/release.test.mjs`.
pub const RELEASE_IDENTIFIER: &str = "com.jabot.app";
/// Renderer event carrying an [`UpdateStatus`].
pub const UPDATE_EVENT: &str = "app-update";
/// Menu item id for **Check for Updates…**.
pub const MENU_CHECK_ID: &str = "jabot-check-for-updates";

#[cfg(target_os = "macos")]
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);
#[cfg(target_os = "macos")]
const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);

/// What the renderer needs to draw (or not draw) the update banner.
///
/// `manual` marks the result of a check the user asked for. A background
/// check that finds nothing, or fails because the laptop is offline, is not
/// worth interrupting anyone for; one the user asked for always answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    Disabled { reason: String },
    Idle,
    Checking { manual: bool },
    Current { version: String, manual: bool },
    Ready { version: String, current: String },
    Failed { message: String, manual: bool },
}

/// Why this process must not self-update, or `None` when it should.
pub fn disabled_reason(identifier: &str, acceptance: bool) -> Option<&'static str> {
    if !cfg!(target_os = "macos") {
        return Some("self-update ships on macOS only");
    }
    if acceptance {
        return Some("acceptance runs never update");
    }
    if identifier != RELEASE_IDENTIFIER {
        return Some("JaBot Local is built from your checkout and does not update itself");
    }
    None
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    #[cfg(target_os = "macos")]
    busy: AtomicBool,
    #[cfg(target_os = "macos")]
    pending: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>,
}

impl Updates {
    fn new(status: UpdateStatus) -> Self {
        Self {
            status: Mutex::new(status),
            #[cfg(target_os = "macos")]
            busy: AtomicBool::new(false),
            #[cfg(target_os = "macos")]
            pending: Mutex::new(None),
        }
    }

    fn status(&self) -> UpdateStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[cfg(target_os = "macos")]
fn set_status(app: &AppHandle, status: UpdateStatus) {
    if let Some(updates) = app.try_state::<Updates>() {
        *updates.status.lock().unwrap_or_else(|e| e.into_inner()) = status.clone();
    }
    if let Err(err) = app.emit(UPDATE_EVENT, status) {
        eprintln!("failed to emit {UPDATE_EVENT}: {err}");
    }
}

/// Manage the state every command reads, and — in the shipped app only —
/// add the menu item and start the periodic check.
pub fn start(app: &AppHandle, acceptance: bool) {
    let identifier = app.config().identifier.clone();
    match disabled_reason(&identifier, acceptance) {
        Some(reason) => {
            app.manage(Updates::new(UpdateStatus::Disabled {
                reason: reason.into(),
            }));
        }
        None => {
            app.manage(Updates::new(UpdateStatus::Idle));
            #[cfg(target_os = "macos")]
            {
                if let Err(err) = install_menu(app) {
                    eprintln!("failed to add Check for Updates… to the menu: {err}");
                }
                spawn_periodic_check(app.clone());
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn install_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};

    // The default menu, plus one item under the app menu right after About —
    // where every Mac app keeps it. Replacing the menu wholesale would lose
    // Edit → Copy/Paste, which the webview relies on for ⌘C/⌘V.
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        let item = MenuItem::with_id(app, MENU_CHECK_ID, "Check for Updates…", true, None::<&str>)?;
        app_menu.insert(&item, 1)?;
        app_menu.insert(&PredefinedMenuItem::separator(app)?, 2)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id() == MENU_CHECK_ID {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { check(app, true).await });
        }
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_periodic_check(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("jabot-updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                tauri::async_runtime::block_on(check(app.clone(), false));
                std::thread::sleep(CHECK_EVERY);
            }
        });
    if let Err(err) = spawned {
        eprintln!("failed to start the update checker: {err}");
    }
}

#[cfg(target_os = "macos")]
async fn check(app: AppHandle, manual: bool) {
    use tauri_plugin_updater::UpdaterExt;

    let Some(updates) = app.try_state::<Updates>() else {
        return;
    };
    // Already staged: re-announce it rather than download it again.
    if let ready @ UpdateStatus::Ready { .. } = updates.status() {
        if manual {
            set_status(&app, ready);
        }
        return;
    }
    if updates.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    set_status(&app, UpdateStatus::Checking { manual });

    let result: Result<_, tauri_plugin_updater::Error> = async {
        match app.updater()?.check().await? {
            None => Ok(None),
            Some(update) => {
                let bytes = update.download(|_, _| {}, || {}).await?;
                Ok(Some((update, bytes)))
            }
        }
    }
    .await;

    let status = match result {
        Ok(None) => UpdateStatus::Current {
            version: app.package_info().version.to_string(),
            manual,
        },
        Ok(Some((update, bytes))) => {
            let status = UpdateStatus::Ready {
                version: update.version.clone(),
                current: update.current_version.clone(),
            };
            *updates.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some((update, bytes));
            status
        }
        Err(err) => {
            eprintln!("update check failed: {err}");
            UpdateStatus::Failed {
                message: err.to_string(),
                manual,
            }
        }
    };
    set_status(&app, status);
    updates.busy.store(false, Ordering::SeqCst);
}

/// Install a staged update, if there is one. Called on the way out, so a
/// user who quits instead of clicking Restart still launches the new version
/// next time. Returns whether anything was installed.
pub fn install_pending(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(updates) = app.try_state::<Updates>() else {
            return false;
        };
        let staged = updates
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((update, bytes)) = staged {
            return match update.install(bytes) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("failed to install JaBot {}: {err}", update.version);
                    false
                }
            };
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    false
}

/// The banner's current state, for a renderer that mounted after the event.
#[tauri::command]
pub fn app_update_status(updates: State<Updates>) -> UpdateStatus {
    updates.status()
}

/// Install the staged update and relaunch into it.
#[tauri::command]
pub fn app_update_restart(app: AppHandle) -> Result<(), String> {
    if !install_pending(&app) {
        return Err("no update is ready to install".into());
    }
    // `request_restart`, not `restart`: the normal exit path runs first, so
    // adapters are shut down and the store is checkpointed (lib.rs `Exit`).
    app.request_restart();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_shipped_identifier_updates() {
        let shipped = disabled_reason(RELEASE_IDENTIFIER, false);
        let local = disabled_reason("com.jabot.app.local", false);
        let acceptance = disabled_reason(RELEASE_IDENTIFIER, true);
        if cfg!(target_os = "macos") {
            assert_eq!(shipped, None);
            assert!(local.unwrap().contains("Local"));
            assert!(acceptance.is_some());
        } else {
            assert!(shipped.unwrap().contains("macOS"));
            assert!(local.is_some());
        }
    }

    #[test]
    fn status_serializes_as_the_renderer_reads_it() {
        let ready = UpdateStatus::Ready {
            version: "0.1.8".into(),
            current: "0.1.7".into(),
        };
        assert_eq!(
            serde_json::to_value(ready).unwrap(),
            serde_json::json!({ "state": "ready", "version": "0.1.8", "current": "0.1.7" })
        );
        let current = UpdateStatus::Current {
            version: "0.1.7".into(),
            manual: true,
        };
        assert_eq!(
            serde_json::to_value(current).unwrap(),
            serde_json::json!({ "state": "current", "version": "0.1.7", "manual": true })
        );
        assert_eq!(
            serde_json::to_value(UpdateStatus::Idle).unwrap(),
            serde_json::json!({ "state": "idle" })
        );
    }
}
