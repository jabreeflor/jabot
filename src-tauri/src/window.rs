//! Window frame and Cursor-style translucency (#250, #282).
//!
//! Two independent bits:
//!
//! - **Chrome** — how the OS draws the frame around the webview. macOS keeps
//!   the overlay title bar (traffic lights on our content). Windows and
//!   Linux take a decorated, opaque window so they never depend on
//!   `macos-private-api` or under-window vibrancy (#282).
//! - **Translucency** — native under-window vibrancy behind nearly-opaque
//!   CSS. The renderer asks [`window_translucency_applied`] and only then
//!   paints `data-translucency="on"`; without that bit the surfaces stay
//!   the solid theme tokens.
//!
//! Platform limits, on purpose:
//!
//! - **macOS 13+** (the bundle floor): overlay chrome plus a material that
//!   follows the app theme — `Effect::HudWindow` for dark, `Effect::Popover`
//!   for light — so the desktop reads through the 36–70% CSS mixes as
//!   colour rather than a grey wash. Reduce Transparency makes the
//!   material opaque at the OS; the CSS `prefers-reduced-transparency`
//!   query is the matching fallback.
//! - **Linux:** Tauri's `set_effects` is unsupported. The window stays
//!   opaque and decorated. `scripts/live.sh` is a Chromium tab that
//!   previews the macOS overlay unless `?chrome=decorated`.
//! - **Windows:** a normal Win10/11 title bar. Close exits (no tray, no
//!   hide-to-Dock). Mica/Acrylic exist in Tauri but are a different look
//!   and are not applied here.
//!
//! Both branches type-check on Linux (`cfg!`, not `#[cfg]`), same as the
//! login-shell PATH probe in `host/harness/path.rs`.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    window::{Color, Effect, EffectState, EffectsBuilder},
    AppHandle, Manager, Runtime, Theme,
};

static APPLIED: AtomicBool = AtomicBool::new(false);

/// The glass material for a palette. The app theme is renderer-local
/// (#178) and need not match the OS, so the window appearance and the
/// material follow `data-theme`, not `prefers-color-scheme`: a dark palette
/// over a light-appearance material is a grey wash, not glass.
fn material(theme: Theme) -> Effect {
    match theme {
        Theme::Light => Effect::Popover,
        _ => Effect::HudWindow,
    }
}

fn set_effects<R: Runtime>(window: &tauri::WebviewWindow<R>, theme: Theme) -> bool {
    match window.set_effects(
        EffectsBuilder::new()
            .effect(material(theme))
            .state(EffectState::Active)
            .build(),
    ) {
        Ok(()) => {
            APPLIED.store(true, Ordering::Relaxed);
            true
        }
        Err(err) => {
            eprintln!("window translucency: native effect unavailable ({err}); staying opaque");
            APPLIED.store(false, Ordering::Relaxed);
            false
        }
    }
}

/// Follow the renderer's resolved theme: NSWindow appearance plus the
/// matching material. No-op off macOS (the decorated window has no glass).
#[tauri::command]
pub fn window_set_theme<R: Runtime>(app: AppHandle<R>, theme: String) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let theme = if theme == "light" {
        Theme::Light
    } else {
        Theme::Dark
    };
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if let Err(err) = window.set_theme(Some(theme)) {
        eprintln!("window translucency: could not set window appearance: {err}");
    }
    set_effects(&window, theme)
}

/// How the OS draws the frame around the webview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    /// macOS overlay title bar: traffic lights sit on our content.
    Overlay,
    /// Native decorations: the OS title bar is outside the webview.
    Decorated,
}

/// The chrome this build actually draws. Overlay is macOS-only.
pub fn chrome() -> Chrome {
    if cfg!(target_os = "macos") {
        Chrome::Overlay
    } else {
        Chrome::Decorated
    }
}

/// Whether [`apply`] last succeeded. The renderer reads this once at boot.
#[tauri::command]
pub fn window_translucency_applied() -> bool {
    APPLIED.load(Ordering::Relaxed)
}

/// `"overlay"` or `"decorated"` — the renderer keys layout insets off this.
#[tauri::command]
pub fn window_chrome() -> &'static str {
    match chrome() {
        Chrome::Overlay => "overlay",
        Chrome::Decorated => "decorated",
    }
}

/// Try to put the native material behind the main window.
///
/// Off macOS this returns false without touching the webview fill: a
/// transparent clear on a decorated window is how you get holes (#282).
/// A missing window is also false: the renderer then keeps the opaque tokens.
pub fn apply<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !cfg!(target_os = "macos") {
        let _ = app;
        APPLIED.store(false, Ordering::Relaxed);
        return false;
    }

    let Some(window) = app.get_webview_window("main") else {
        APPLIED.store(false, Ordering::Relaxed);
        return false;
    };

    // So the nearly-opaque CSS can show the material (or the desktop)
    // rather than WKWebView's default solid fill. macOS-only: clearing
    // this on Windows/Linux punches a hole through an opaque window.
    if let Err(err) = window.set_background_color(Some(Color(0, 0, 0, 0))) {
        eprintln!("window translucency: could not clear the webview fill: {err}");
    }

    // Dark is the renderer's default palette; `window_set_theme` re-applies
    // once the stored preference is on the document.
    if let Err(err) = window.set_theme(Some(Theme::Dark)) {
        eprintln!("window translucency: could not set window appearance: {err}");
    }
    set_effects(&window, Theme::Dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_applied_only_after_a_successful_apply() {
        // No window in a unit test: apply cannot succeed, and the bit
        // stays false so the renderer would keep the opaque tokens.
        assert!(!window_translucency_applied());
    }

    #[test]
    fn chrome_matches_the_os_frame() {
        if cfg!(target_os = "macos") {
            assert_eq!(chrome(), Chrome::Overlay);
            assert_eq!(window_chrome(), "overlay");
        } else {
            assert_eq!(chrome(), Chrome::Decorated);
            assert_eq!(window_chrome(), "decorated");
        }
    }
}
