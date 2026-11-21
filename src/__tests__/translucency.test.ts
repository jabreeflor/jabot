/**
 * Window translucency (#250), pure: when the chrome may go slightly
 * see-through, and what lands on `<html>`. The native material is a Rust
 * window effect; this file is the renderer decision.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import {
  applyTranslucency,
  bootTranslucency,
  prefersReducedTransparency,
  previewTranslucencyRequested,
  probeNativeTranslucency,
  resolveTranslucency,
  subscribeReducedTransparency,
} from "../translucency";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function stubMatchMedia(reduced: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mq = {
    matches: reduced,
    media: "(prefers-reduced-transparency: reduce)",
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
  delete document.documentElement.dataset.translucency;
});

describe("previewTranslucencyRequested", () => {
  it("is on only for the live.sh preview query", () => {
    expect(previewTranslucencyRequested("")).toBe(false);
    expect(previewTranslucencyRequested("?theme=light")).toBe(false);
    expect(previewTranslucencyRequested("?translucency=on")).toBe(false);
    expect(previewTranslucencyRequested("?translucency=preview")).toBe(true);
    expect(
      previewTranslucencyRequested("?theme=dark&translucency=preview"),
    ).toBe(true);
  });
});

describe("resolveTranslucency", () => {
  it("stays opaque when the native material is missing", () => {
    stubMatchMedia(false);
    expect(resolveTranslucency(false, "")).toBe(false);
  });

  it("allows the live.sh preview without a native material", () => {
    stubMatchMedia(false);
    expect(resolveTranslucency(false, "?translucency=preview")).toBe(true);
  });

  it("follows a successful native apply", () => {
    stubMatchMedia(false);
    expect(resolveTranslucency(true)).toBe(true);
  });

  it("stays opaque when Reduce Transparency is on, even if native applied", () => {
    stubMatchMedia(true);
    expect(prefersReducedTransparency()).toBe(true);
    expect(resolveTranslucency(true)).toBe(false);
  });
});

describe("applyTranslucency", () => {
  it("paints and clears data-translucency on the document", () => {
    applyTranslucency(true);
    expect(document.documentElement.dataset.translucency).toBe("on");
    applyTranslucency(false);
    expect(document.documentElement.dataset.translucency).toBeUndefined();
  });

  it("re-applies when Reduce Transparency flips", () => {
    const mq = stubMatchMedia(false);
    applyTranslucency(true);
    const stop = subscribeReducedTransparency(() => {
      applyTranslucency(resolveTranslucency(true));
    });
    mq.dispatch(true);
    expect(document.documentElement.dataset.translucency).toBeUndefined();
    mq.dispatch(false);
    expect(document.documentElement.dataset.translucency).toBe("on");
    stop();
  });
});

describe("probeNativeTranslucency / bootTranslucency", () => {
  it("is false outside Tauri", async () => {
    expect(await probeNativeTranslucency()).toBe(false);
    stubMatchMedia(false);
    expect(await bootTranslucency()).toBe(false);
    expect(document.documentElement.dataset.translucency).toBeUndefined();
  });

  it("follows the host when Tauri reports the material stuck", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(invoke).mockResolvedValue(true);
    stubMatchMedia(false);
    expect(await probeNativeTranslucency()).toBe(true);
    expect(await bootTranslucency()).toBe(true);
    expect(document.documentElement.dataset.translucency).toBe("on");
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("stays opaque when the host invoke fails", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(invoke).mockRejectedValue(new Error("no window"));
    expect(await probeNativeTranslucency()).toBe(false);
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("treats a missing matchMedia as not reduced", () => {
    vi.spyOn(window, "matchMedia").mockImplementation(() => {
      throw new Error("no mq");
    });
    expect(prefersReducedTransparency()).toBe(false);
    const stop = subscribeReducedTransparency(() => {});
    stop();
  });
});

describe("chrome mix tokens", () => {
  const css = readFileSync("src/styles/tokens.css", "utf8");
  const glass = css.slice(css.indexOf('html[data-translucency="on"]'));
  const reduced = glass.slice(
    glass.indexOf("@media (prefers-reduced-transparency"),
  );

  it("mixes raised chrome from solid swatches, not a second palette", () => {
    expect(css).toMatch(/--raise-solid:\s*#2a2a2c/);
    expect(glass).toMatch(
      /--raise:\s*color-mix\(in srgb,\s*var\(--raise-solid\) 96%,\s*transparent\)/,
    );
    expect(glass).toMatch(
      /--side:\s*color-mix\(in srgb,\s*var\(--side-solid\) 92%,\s*transparent\)/,
    );
    expect(glass).toMatch(/--chrome-frost:\s*blur\(/);
  });

  it("keeps cream and ink solid in the glass block", () => {
    const glassRules = glass.slice(0, glass.indexOf("@media"));
    expect(glassRules).not.toMatch(/--cream:/);
    expect(glassRules).not.toMatch(/--ink:/);
  });

  it("restores solids and drops frost when Reduce Transparency is on", () => {
    expect(reduced).toMatch(/--raise:\s*var\(--raise-solid\)/);
    expect(reduced).toMatch(/--chrome-frost:\s*none/);
  });
});
