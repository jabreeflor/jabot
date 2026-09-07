//! Subtle Cursor-style window translucency (#250).
//!
//! **macOS** is the only platform that gets a native material. The window is
//! already `transparent` in `tauri.conf.json` (effects will not show through
//! an opaque backing) and this module paints a restrained vibrancy —
//! [`Effect::HudWindow`] in Dark, [`Effect::HeaderView`] in Light — so a faint
//! hint of the desktop reads through the chrome. The renderer then overlays
//! ~96% opaque fills; text, inputs, and raised surfaces stay solid.
//!
//! **Everywhere else** [`apply`] is a documented no-op that returns `false`.
//! Tauri's `set_effects` is unsupported on Linux and ignores the inner error,
//! and JaBot does not ship a Windows bundle. The renderer treats `false` as
//! "no material" and keeps an opaque fallback when the OS also asks for
//! reduced transparency (`prefers-reduced-transparency`).
//!
//! Native effects need `macos-private-api` (already on for the overlay
//! titlebar). They do not need AppKit.

use tauri::window::Effect;

/// The material that matches a resolved palette. Dark gets the HUD material
/// Cursor-like apps sit on; Light gets the header material so the 4% gap
/// does not pick up a dark frost.
pub fn effect_for_theme(theme: &str) -> Effect {
    if theme == "light" {
        Effect::HeaderView
    } else {
        Effect::HudWindow
    }
}

/// Apply or clear the material on the main window.
///
/// Returns `true` only when a native effect is actually on the window. Off
/// macOS that is never, so CI's Linux verify job compiles a real no-op.
pub fn apply<R: tauri::Runtime>(app: &tauri::AppHandle<R>, theme: &str, enabled: bool) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        // Keep the mapping compiled on Linux so clippy does not treat it as
        // macOS-only dead code, and so the test below runs in CI.
        let _ = (app, enabled, effect_for_theme(theme));
        false
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::window::{EffectState, EffectsBuilder};
        use tauri::Manager;

        let Some(window) = app.get_webview_window("main") else {
            return false;
        };
        if !enabled {
            if let Err(err) = window.set_effects(None::<tauri::utils::config::WindowEffectsConfig>)
            {
                eprintln!("failed to clear window effects: {err}");
            }
            return false;
        }
        let effects = EffectsBuilder::new()
            .effect(effect_for_theme(theme))
            .state(EffectState::FollowsWindowActiveState)
            .build();
        if let Err(err) = window.set_effects(effects) {
            eprintln!("window effects unavailable: {err}");
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_uses_the_hud_material() {
        assert_eq!(effect_for_theme("dark"), Effect::HudWindow);
        // Anything that is not an explicit light palette stays on the
        // shipped dark material — same default as the renderer.
        assert_eq!(effect_for_theme("system"), Effect::HudWindow);
        assert_eq!(effect_for_theme(""), Effect::HudWindow);
    }

    #[test]
    fn light_uses_the_header_material() {
        assert_eq!(effect_for_theme("light"), Effect::HeaderView);
    }
}
