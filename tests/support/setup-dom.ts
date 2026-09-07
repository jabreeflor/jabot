import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

import { applyTheme } from "../../src/theme";
import { seedOnboarded } from "./onboarding";

/**
 * jsdom normally provides `localStorage`. On Node 26, Vitest workers have
 * been seen with `window.localStorage` undefined at this file
 * (`#215`). Supported CI/runtime is Node 22; this shim keeps setup from
 * cascading 553 failures if a worker is missing the Web Storage globals.
 */
function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear() {
      map.clear();
    },
    getItem(key) {
      return map.has(key) ? map.get(key)! : null;
    },
    key(index) {
      return [...map.keys()][index] ?? null;
    },
    removeItem(key) {
      map.delete(key);
    },
    setItem(key, value) {
      map.set(String(key), String(value));
    },
  };
}

function ensureWebStorage(): void {
  if (typeof window === "undefined") return;
  for (const name of ["localStorage", "sessionStorage"] as const) {
    try {
      const store = window[name];
      if (store && typeof store.getItem === "function") {
        store.setItem("__jabot_storage_probe", "1");
        store.removeItem("__jabot_storage_probe");
        continue;
      }
    } catch {
      // Missing, null, or SecurityError — install a memory store.
    }
    Object.defineProperty(window, name, {
      configurable: true,
      writable: true,
      value: memoryStorage(),
    });
  }
}

ensureWebStorage();

// The unit default is "this Mac has already been through first-run setup".
// SIX suites render <App/> — app, crew-store, fold, folders, inbox-host,
// notifications — and every one of them wants the shell, not the takeover.
// Do not "optimize" this into a per-file beforeEach; that breaks five of them.
// To exercise first run, call clearOnboarding() in your own beforeEach.
beforeEach(() => {
  ensureWebStorage();
  window.localStorage.clear();
  seedOnboarded();
  applyTheme("dark");
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
