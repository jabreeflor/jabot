/**
 * Window translucency (#250), pure: when the chrome may go slightly
 * see-through, and what lands on `<html>`. The native material is a Rust
 * window effect; this file is the renderer decision.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  applyTranslucency,
  prefersReducedTransparency,
  previewTranslucencyRequested,
  resolveTranslucency,
  subscribeReducedTransparency,
} from "../translucency";

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
