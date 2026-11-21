//! Appearance preference (#178): dark, light, or follow the OS.
//!
//! Renderer-local, not a host setting. Theme has to be on the document
//! before the host answers — a round trip that flashes the shipped dark
//! palette is worse than a preference the host cannot see. localStorage
//! is the same home as the sidebar open state.
//!
//! Dark is the default so an existing install does not flip on upgrade.
//! "Match system" is the opt-in that follows `prefers-color-scheme`.

import { useEffect, useState } from "react";

export const THEME_KEY = "jabot.theme";

export type ThemePreference = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

export const THEME_PREFERENCES = ["dark", "light", "system"] as const;

export function isThemePreference(value: string): value is ThemePreference {
  return value === "dark" || value === "light" || value === "system";
}

/** The stored choice, or Dark when the store is empty, garbage, or unreadable. */
export function loadThemePreference(): ThemePreference {
  try {
    const raw = window.localStorage.getItem(THEME_KEY);
    if (raw && isThemePreference(raw)) return raw;
  } catch {
    // Quota / private mode: same default as a first run.
  }
  return "dark";
}

export function saveThemePreference(preference: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_KEY, preference);
  } catch {
    // The session still switches; the next launch will be Dark.
  }
}

/** Light only when the OS is explicitly light. Missing API → Dark. */
export function systemPrefersLight(): boolean {
  try {
    return window.matchMedia("(prefers-color-scheme: light)").matches;
  } catch {
    return false;
  }
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "light") return "light";
  if (preference === "dark") return "dark";
  return systemPrefersLight() ? "light" : "dark";
}

/**
 * Paint the resolved palette onto `<html>`. `data-theme` is what the
 * stylesheets key off; `color-scheme` is what native controls and scrollbars
 * follow.
 */
export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);
  const root = document.documentElement;
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
  return resolved;
}

/** Re-apply when the OS appearance changes. No-op if matchMedia is missing. */
export function subscribeSystemTheme(onChange: () => void): () => void {
  try {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const listener = () => onChange();
    mq.addEventListener("change", listener);
    return () => mq.removeEventListener("change", listener);
  } catch {
    return () => {};
  }
}

/**
 * Boot the stored preference and keep "Match system" live for the whole
 * session — Settings is not always mounted.
 */
export function bootTheme(): ResolvedTheme {
  const preference = loadThemePreference();
  const resolved = applyTheme(preference);
  subscribeSystemTheme(() => {
    if (loadThemePreference() === "system") applyTheme("system");
  });
  return resolved;
}

export function useTheme(): {
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
} {
  const [preference, setPref] = useState(loadThemePreference);

  useEffect(() => {
    applyTheme(preference);
    if (preference !== "system") return;
    return subscribeSystemTheme(() => applyTheme("system"));
  }, [preference]);

  function setPreference(next: ThemePreference) {
    saveThemePreference(next);
    applyTheme(next);
    setPref(next);
  }

  return { preference, setPreference };
}
