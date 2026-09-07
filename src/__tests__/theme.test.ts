/**
 * Appearance preference (#178), pure: the stored choice, the resolve
 * against the OS, and what lands on `<html>`. The Settings radios are
 * covered in settings.test.tsx; this file is the store and the paint.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  THEME_KEY,
  applyTheme,
  isThemePreference,
  loadThemePreference,
  resolveTheme,
  saveThemePreference,
  subscribeSystemTheme,
  systemPrefersLight,
} from "../theme";

function stubMatchMedia(light: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mq = {
    matches: light,
    media: "(prefers-color-scheme: light)",
    addEventListener: (
      _type: string,
      listener: (event: MediaQueryListEvent) => void,
    ) => {
      listeners.add(listener);
    },
    removeEventListener: (
      _type: string,
      listener: (event: MediaQueryListEvent) => void,
    ) => {
      listeners.delete(listener);
    },
    dispatch: (next: boolean) => {
      mq.matches = next;
      for (const listener of listeners) {
        listener({ matches: next } as MediaQueryListEvent);
      }
    },
  };
  vi.spyOn(window, "matchMedia").mockImplementation(
    () => mq as unknown as MediaQueryList,
  );
  return mq;
}

afterEach(() => {
  vi.restoreAllMocks();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.style.colorScheme = "";
});

describe("loadThemePreference / saveThemePreference", () => {
  it("defaults to dark so an existing install does not flip", () => {
    window.localStorage.removeItem(THEME_KEY);
    expect(loadThemePreference()).toBe("dark");
  });

  it("round-trips a stored choice", () => {
    saveThemePreference("light");
    expect(window.localStorage.getItem(THEME_KEY)).toBe("light");
    expect(loadThemePreference()).toBe("light");
  });

  it("treats garbage as dark rather than inventing a third palette", () => {
    window.localStorage.setItem(THEME_KEY, "sepia");
    expect(loadThemePreference()).toBe("dark");
    expect(isThemePreference("sepia")).toBe(false);
    expect(isThemePreference("system")).toBe(true);
  });

  it("does not throw when the store refuses a write", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(() => saveThemePreference("light")).not.toThrow();
  });
});

describe("resolveTheme", () => {
  it("honours an explicit choice without asking the OS", () => {
    stubMatchMedia(true);
    expect(resolveTheme("dark")).toBe("dark");
    stubMatchMedia(false);
    expect(resolveTheme("light")).toBe("light");
  });

  it("follows prefers-color-scheme when the choice is system", () => {
    stubMatchMedia(true);
    expect(systemPrefersLight()).toBe(true);
    expect(resolveTheme("system")).toBe("light");
    stubMatchMedia(false);
    expect(resolveTheme("system")).toBe("dark");
  });
});

describe("applyTheme", () => {
  it("paints data-theme and color-scheme on the document", () => {
    expect(applyTheme("light")).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.colorScheme).toBe("light");
  });

  it("re-applies when the OS appearance changes under Match system", () => {
    const mq = stubMatchMedia(false);
    applyTheme("system");
    expect(document.documentElement.dataset.theme).toBe("dark");

    const stop = subscribeSystemTheme(() => applyTheme("system"));
    mq.dispatch(true);
    expect(document.documentElement.dataset.theme).toBe("light");
    stop();
  });
});
