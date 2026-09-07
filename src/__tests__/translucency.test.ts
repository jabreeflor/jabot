/**
 * Window translucency (#250), pure: the reduced-transparency fallback,
 * the `data-translucency` paint, and the host call that applies the
 * native material. The Settings copy is covered in settings.test.tsx;
 * this file is the store and the paint.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";

import {
  TRANSLUCENCY_QUERY,
  applyNativeWindowEffects,
  applyTranslucency,
  prefersReducedTransparency,
  shouldUseTranslucentChrome,
  subscribeReducedTransparency,
  syncWindowTranslucency,
} from "../translucency";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function stubMatchMedia(reduced: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const mq = {
    matches: reduced,
    media: TRANSLUCENCY_QUERY,
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
  vi.spyOn(window, "matchMedia").mockImplementation((query: string) => {
    if (query === TRANSLUCENCY_QUERY) return mq as unknown as MediaQueryList;
    return { matches: false } as MediaQueryList;
  });
  return mq;
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(true);
});

afterEach(() => {
  vi.restoreAllMocks();
  delete document.documentElement.dataset.translucency;
});

describe("shouldUseTranslucentChrome", () => {
  it("is on unless the OS asked for reduced transparency", () => {
    stubMatchMedia(false);
    expect(prefersReducedTransparency()).toBe(false);
    expect(shouldUseTranslucentChrome()).toBe(true);
  });

  it("falls back to opaque when reduced transparency is on", () => {
    stubMatchMedia(true);
    expect(prefersReducedTransparency()).toBe(true);
    expect(shouldUseTranslucentChrome()).toBe(false);
  });

  it("treats a missing matchMedia as not-reduced, same as a first run", () => {
    vi.spyOn(window, "matchMedia").mockImplementation(() => {
      throw new Error("no matchMedia");
    });
    expect(prefersReducedTransparency()).toBe(false);
    expect(shouldUseTranslucentChrome()).toBe(true);
  });
});

describe("applyTranslucency", () => {
  it("paints data-translucency on the document and clears it", () => {
    applyTranslucency(true);
    expect(document.documentElement.dataset.translucency).toBe("on");
    applyTranslucency(false);
    expect(document.documentElement.dataset.translucency).toBeUndefined();
  });
});

describe("applyNativeWindowEffects", () => {
  it("asks the host to apply the material for the resolved palette", async () => {
    await expect(applyNativeWindowEffects("light", true)).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("apply_window_translucency", {
      theme: "light",
      enabled: true,
    });
  });

  it("treats a missing host as no material, not an error", async () => {
    invokeMock.mockRejectedValue(new Error("IPC unavailable"));
    await expect(applyNativeWindowEffects("dark", true)).resolves.toBe(false);
  });
});

describe("syncWindowTranslucency", () => {
  it("paints the chrome and asks the host when reduced transparency is off", async () => {
    stubMatchMedia(false);
    await expect(syncWindowTranslucency("dark")).resolves.toBe(true);
    expect(document.documentElement.dataset.translucency).toBe("on");
    expect(invokeMock).toHaveBeenCalledWith("apply_window_translucency", {
      theme: "dark",
      enabled: true,
    });
  });

  it("clears the chrome and the material when reduced transparency is on", async () => {
    stubMatchMedia(true);
    document.documentElement.dataset.translucency = "on";
    await expect(syncWindowTranslucency("dark")).resolves.toBe(false);
    expect(document.documentElement.dataset.translucency).toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("apply_window_translucency", {
      theme: "dark",
      enabled: false,
    });
  });
});

describe("subscribeReducedTransparency", () => {
  it("re-paints when the OS preference flips", () => {
    const mq = stubMatchMedia(false);
    const onChange = vi.fn();
    const stop = subscribeReducedTransparency(onChange);
    mq.dispatch(true);
    expect(onChange).toHaveBeenCalledTimes(1);
    stop();
    mq.dispatch(false);
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});
