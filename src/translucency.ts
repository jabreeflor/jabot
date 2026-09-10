//! Window translucency (#250): when the chrome may go slightly see-through.
//!
//! Native under-window vibrancy lives in `src-tauri/src/window.rs` and is
//! macOS-only. This file is the renderer half: paint `data-translucency`
//! when that material is actually behind the webview (or when a live.sh
//! shot asks for a preview), and stay fully opaque otherwise — including
//! Reduce Transparency, Linux, Windows, and a unit test. Styles then mix
//! pane *and* raised chrome tokens plus a light backdrop-filter frost.

import { invoke } from "@tauri-apps/api/core";

export const TRANSLUCENCY_PREVIEW_PARAM = "translucency";
export const TRANSLUCENCY_PREVIEW_VALUE = "preview";

/** `?translucency=preview` — live.sh shots over a fake desktop. */
export function previewTranslucencyRequested(
  search: string = typeof window === "undefined" ? "" : window.location.search,
): boolean {
  try {
    return (
      new URLSearchParams(search).get(TRANSLUCENCY_PREVIEW_PARAM) ===
      TRANSLUCENCY_PREVIEW_VALUE
    );
  } catch {
    return false;
  }
}

/** Accessibility "Reduce transparency" — missing API means "not reduced". */
export function prefersReducedTransparency(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-transparency: reduce)").matches;
  } catch {
    return false;
  }
}

/**
 * The chrome may go translucent only when a native (or preview) backdrop
 * exists and the OS has not asked us to stay solid.
 */
export function resolveTranslucency(
  nativeApplied: boolean,
  search?: string,
): boolean {
  if (prefersReducedTransparency()) return false;
  return nativeApplied || previewTranslucencyRequested(search);
}

/** Paint or clear `data-translucency` on `<html>`. Styles key off this. */
export function applyTranslucency(enabled: boolean): void {
  const root = document.documentElement;
  if (enabled) root.dataset.translucency = "on";
  else delete root.dataset.translucency;
}

export function subscribeReducedTransparency(onChange: () => void): () => void {
  try {
    const mq = window.matchMedia("(prefers-reduced-transparency: reduce)");
    const listener = () => onChange();
    mq.addEventListener("change", listener);
    return () => mq.removeEventListener("change", listener);
  } catch {
    return () => {};
  }
}

/**
 * Ask the host whether the native material stuck. Outside Tauri this is
 * false: the live loop and jsdom have no vibrancy to show through.
 */
export async function probeNativeTranslucency(): Promise<boolean> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return false;
  }
  try {
    return (await invoke<boolean>("window_translucency_applied")) === true;
  } catch {
    return false;
  }
}

/**
 * Boot the attribute and keep Reduce Transparency live for the session.
 * Settings does not own this — it is an OS bit, not a preference.
 */
export async function bootTranslucency(): Promise<boolean> {
  const native = await probeNativeTranslucency();
  const paint = () => applyTranslucency(resolveTranslucency(native));
  paint();
  subscribeReducedTransparency(paint);
  return resolveTranslucency(native);
}
