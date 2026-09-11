//! Window chrome (#282): overlay (macOS traffic lights on content) vs
//! decorated (native title bar outside the webview).
//!
//! The host reports which frame it actually drew. Outside Tauri the
//! preview stays overlay so live.sh / tests keep the macOS shell. A
//! `?chrome=decorated` query is the Windows layout without a Windows box.
//! Styles key off `data-window-chrome` the same way theme and translucency
//! key off their attributes.

import { invoke } from "@tauri-apps/api/core";

export const WINDOW_CHROME_PARAM = "chrome";

export type WindowChrome = "overlay" | "decorated";

export function isWindowChrome(value: string): value is WindowChrome {
  return value === "overlay" || value === "decorated";
}

/** `?chrome=overlay|decorated` — live.sh shots of a specific frame. */
export function previewWindowChrome(
  search: string = typeof window === "undefined" ? "" : window.location.search,
): WindowChrome | null {
  try {
    const raw = new URLSearchParams(search).get(WINDOW_CHROME_PARAM);
    return raw && isWindowChrome(raw) ? raw : null;
  } catch {
    return null;
  }
}

/**
 * First-paint guess before the host answers. Windows UA is decorated so
 * the title-bar inset does not flash on; everything else stays overlay
 * (live.sh on Linux/macOS, jsdom, the shipping Mac app).
 */
export function inferredWindowChrome(
  userAgent: string = typeof navigator === "undefined"
    ? ""
    : navigator.userAgent,
): WindowChrome {
  return /Windows/i.test(userAgent) ? "decorated" : "overlay";
}

/** Preview wins, then the host, then the UA guess, then overlay. */
export function resolveWindowChrome(
  native: WindowChrome | null,
  search?: string,
  userAgent?: string,
): WindowChrome {
  return (
    previewWindowChrome(search) ?? native ?? inferredWindowChrome(userAgent)
  );
}

/** Paint `data-window-chrome` on `<html>`. Layout tokens key off this. */
export function applyWindowChrome(chrome: WindowChrome): void {
  document.documentElement.dataset.windowChrome = chrome;
}

/**
 * Ask the host which frame it drew. Outside Tauri this is null: live.sh
 * and jsdom keep the overlay preview unless `?chrome=` says otherwise.
 */
export async function probeNativeWindowChrome(): Promise<WindowChrome | null> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return null;
  }
  try {
    const raw = await invoke<string>("window_chrome");
    return isWindowChrome(raw) ? raw : null;
  } catch {
    return null;
  }
}

/**
 * Paint a first-frame guess, then confirm with the host. Overlay is the
 * CSS default, so a missing attribute is the macOS shell.
 */
export async function bootWindowChrome(): Promise<WindowChrome> {
  applyWindowChrome(resolveWindowChrome(null));
  const chrome = resolveWindowChrome(await probeNativeWindowChrome());
  applyWindowChrome(chrome);
  return chrome;
}
