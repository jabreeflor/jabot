//! Window translucency (#250): a hair of desktop through the chrome.
//!
//! Native vibrancy is a macOS window material applied by the host. The
//! renderer only decides whether the chrome fills go slightly transparent
//! (`data-translucency="on"`) and asks the host to match. Text, inputs, and
//! raised surfaces keep the solid tokens.
//!
//! Fallback is opaque: `prefers-reduced-transparency`, a host that cannot
//! apply effects, or a missing matchMedia. The live Chromium loop has no
//! native window, so it still paints the CSS fills — that is how the
//! screenshots under `docs/img/window-translucency/` show the 4% gap.

import type { ResolvedTheme } from "./theme";

export const TRANSLUCENCY_QUERY = "(prefers-reduced-transparency: reduce)";

export function prefersReducedTransparency(): boolean {
  try {
    return window.matchMedia(TRANSLUCENCY_QUERY).matches;
  } catch {
    return false;
  }
}

/** Paint or clear `data-translucency` on `<html>`. Stylesheets key off it. */
export function applyTranslucency(enabled: boolean): void {
  const root = document.documentElement;
  if (enabled) root.dataset.translucency = "on";
  else delete root.dataset.translucency;
}

/**
 * Ask the host to apply or clear the native material. Returns whether a
 * material is actually on the window. A missing IPC (unit tests, the live
 * Chromium loop) is "no material", not an error.
 */
export async function applyNativeWindowEffects(
  theme: ResolvedTheme,
  enabled: boolean,
): Promise<boolean> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<boolean>("apply_window_translucency", {
      theme,
      enabled,
    });
  } catch {
    return false;
  }
}

/**
 * Chrome fills go translucent unless the OS asked us not to. Native
 * vibrancy is additive on macOS; elsewhere the 4% gap composites against
 * the window backing and reads as nearly opaque.
 */
export function shouldUseTranslucentChrome(): boolean {
  return !prefersReducedTransparency();
}

export async function syncWindowTranslucency(
  theme: ResolvedTheme,
): Promise<boolean> {
  const enabled = shouldUseTranslucentChrome();
  applyTranslucency(enabled);
  await applyNativeWindowEffects(theme, enabled);
  return enabled;
}

export function subscribeReducedTransparency(onChange: () => void): () => void {
  try {
    const mq = window.matchMedia(TRANSLUCENCY_QUERY);
    const listener = () => onChange();
    mq.addEventListener("change", listener);
    return () => mq.removeEventListener("change", listener);
  } catch {
    return () => {};
  }
}

/**
 * Boot the chrome treatment and keep it in lockstep with the OS reduced-
 * transparency preference for the whole session — Settings is not always
 * mounted, and this is not a user toggle.
 */
function themeFromDocument(): ResolvedTheme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function bootTranslucency(theme: ResolvedTheme): boolean {
  const enabled = shouldUseTranslucentChrome();
  applyTranslucency(enabled);
  void applyNativeWindowEffects(theme, enabled);
  subscribeReducedTransparency(() => {
    void syncWindowTranslucency(themeFromDocument());
  });
  return enabled;
}
