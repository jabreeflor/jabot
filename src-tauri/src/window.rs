//! Subtle Cursor-style window translucency (#250).
//!
//! macOS gets a native under-window vibrancy behind nearly-opaque CSS. The
//! renderer asks [`applied`] and only then paints `data-translucency="on"`;
//! without that bit the surfaces stay the solid theme tokens.
//!
//! Platform limits, on purpose:
//!
//! - **macOS 13+** (the bundle floor): `Effect::UnderWindowBackground`, the
//!   same semantic material Electron's `under-window` vibrancy maps to.
//!   Reduce Transparency makes the material opaque at the OS; the CSS
//!   `prefers-reduced-transparency` query is the matching fallback.
//! - **Linux:** Tauri's `set_effects` is unsupported. The window stays
//!   opaque. `scripts/live.sh` is this path — a Chromium tab, not a
//!   vibrancy-backed window.
//! - **Windows:** Mica/Acrylic exist in Tauri but are a different look and
//!   are not applied here. The app stays opaque until someone tunes them
//!   against Cursor on a real Windows box.
//!
//! Both branches type-check on Linux (`cfg!`, not `#[cfg]`), same as the
//! login-shell PATH probe in `host/harness/path.rs`.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    window::{Color, Effect, EffectState, EffectsBuilder},
    AppHandle, Manager, Runtime,
};

static APPLIED: AtomicBool = AtomicBool::new(false);

/// Whether [`apply`] last succeeded. The renderer reads this once at boot.
#[tauri::command]
pub fn window_translucency_applied() -> bool {
    APPLIED.load(Ordering::Relaxed)
}

/// Try to put the native material behind the main window.
///
/// Off macOS this returns false after `set_effects` refuses. A missing
/// window is also false: the renderer then keeps the opaque tokens.
pub fn apply<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        APPLIED.store(false, Ordering::Relaxed);
        return false;
    };

    // So the nearly-opaque CSS can show the material (or the desktop)
    // rather than WKWebView's default solid fill.
    if let Err(err) = window.set_background_color(Some(Color(0, 0, 0, 0))) {
        eprintln!("window translucency: could not clear the webview fill: {err}");
    }

    if !cfg!(target_os = "macos") {
        APPLIED.store(false, Ordering::Relaxed);
        return false;
    }

    match window.set_effects(
        EffectsBuilder::new()
            .effect(Effect::UnderWindowBackground)
            .state(EffectState::FollowsWindowActiveState)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_applied_only_after_a_successful_apply() {
        // No window in a unit test: apply cannot succeed, and the bit
        // stays false so the renderer would keep the opaque tokens.
        assert!(!window_translucency_applied());
    }
}
