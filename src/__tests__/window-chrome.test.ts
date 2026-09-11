/**
 * Window chrome (#282), pure: overlay vs decorated, and what lands on
 * `<html>`. The native frame is a Rust/Tauri config; this file is the
 * renderer decision and the CSS contract.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import {
  applyWindowChrome,
  bootWindowChrome,
  inferredWindowChrome,
  isWindowChrome,
  previewWindowChrome,
  probeNativeWindowChrome,
  resolveWindowChrome,
} from "../windowChrome";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
  vi.restoreAllMocks();
  delete document.documentElement.dataset.windowChrome;
});

describe("previewWindowChrome", () => {
  it("is set only for an explicit chrome query", () => {
    expect(previewWindowChrome("")).toBeNull();
    expect(previewWindowChrome("?theme=light")).toBeNull();
    expect(previewWindowChrome("?chrome=mica")).toBeNull();
    expect(previewWindowChrome("?chrome=decorated")).toBe("decorated");
    expect(previewWindowChrome("?theme=dark&chrome=overlay")).toBe("overlay");
  });
});

describe("inferredWindowChrome", () => {
  it("stays overlay for a Windows UA outside Tauri (Playwright Desktop Chrome)", () => {
    expect(
      inferredWindowChrome("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", false),
    ).toBe("overlay");
    expect(inferredWindowChrome("Mozilla/5.0 (X11; Linux x86_64)", false)).toBe(
      "overlay",
    );
  });

  it("is decorated only for a Windows Tauri webview", () => {
    expect(
      inferredWindowChrome("Mozilla/5.0 (Macintosh; Intel Mac OS X)", true),
    ).toBe("overlay");
    expect(
      inferredWindowChrome("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", true),
    ).toBe("decorated");
  });
});

describe("resolveWindowChrome", () => {
  it("lets the live.sh query override the host and the UA", () => {
    expect(resolveWindowChrome("overlay", "?chrome=decorated")).toBe(
      "decorated",
    );
    expect(
      resolveWindowChrome(
        null,
        "?chrome=overlay",
        "Mozilla/5.0 (Windows NT 10.0)",
        true,
      ),
    ).toBe("overlay");
  });

  it("follows the host when no query is set", () => {
    expect(resolveWindowChrome("decorated", "")).toBe("decorated");
    expect(resolveWindowChrome("overlay", "")).toBe("overlay");
  });

  it("falls back to Tauri+Windows, then overlay", () => {
    expect(
      resolveWindowChrome(null, "", "Mozilla/5.0 (Windows NT 10.0)", true),
    ).toBe("decorated");
    expect(
      resolveWindowChrome(null, "", "Mozilla/5.0 (Windows NT 10.0)", false),
    ).toBe("overlay");
    expect(resolveWindowChrome(null, "", "Mozilla/5.0 (X11; Linux)")).toBe(
      "overlay",
    );
  });
});

describe("applyWindowChrome / bootWindowChrome", () => {
  it("paints data-window-chrome on the document", () => {
    applyWindowChrome("decorated");
    expect(document.documentElement.dataset.windowChrome).toBe("decorated");
    applyWindowChrome("overlay");
    expect(document.documentElement.dataset.windowChrome).toBe("overlay");
  });

  it("is overlay outside Tauri", async () => {
    expect(await probeNativeWindowChrome()).toBeNull();
    expect(await bootWindowChrome()).toBe("overlay");
    expect(document.documentElement.dataset.windowChrome).toBe("overlay");
  });

  it("follows the host when Tauri reports decorated", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(invoke).mockResolvedValue("decorated");
    expect(await probeNativeWindowChrome()).toBe("decorated");
    expect(await bootWindowChrome()).toBe("decorated");
    expect(document.documentElement.dataset.windowChrome).toBe("decorated");
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("stays on the UA guess when the host invoke fails", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(invoke).mockRejectedValue(new Error("no window"));
    expect(await probeNativeWindowChrome()).toBeNull();
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("rejects an unknown host string", () => {
    expect(isWindowChrome("mica")).toBe(false);
    expect(isWindowChrome("decorated")).toBe(true);
  });
});

describe("decorated chrome tokens", () => {
  const tokens = readFileSync("src/styles/tokens.css", "utf8");
  const shell = readFileSync("src/styles/shell.css", "utf8");
  const sidebar = readFileSync("src/styles/sidebar.css", "utf8");
  const decorated = tokens.slice(
    tokens.indexOf('html[data-window-chrome="decorated"]'),
  );

  it("zeros overlay insets and keeps a toggle-wide collapsed rail", () => {
    expect(tokens).toMatch(/--titlebar-h:\s*28px/);
    expect(tokens).toMatch(/--traffic-lights-w:\s*78px/);
    expect(tokens).toMatch(/--collapsed-rail-w:\s*var\(--traffic-lights-w\)/);
    expect(decorated).toMatch(/--titlebar-h:\s*0px/);
    expect(decorated).toMatch(/--traffic-lights-w:\s*0px/);
    expect(decorated).toMatch(/--collapsed-rail-w:\s*56px/);
    expect(decorated).toMatch(/background:\s*var\(--win-solid\)/);
  });

  it("hides the overlay drag region when the OS already drew a title bar", () => {
    expect(shell).toMatch(
      /html\[data-window-chrome="decorated"\]\s+\.titlebar-drag/,
    );
  });

  it("sizes the collapsed rail from --collapsed-rail-w, not the lights inset", () => {
    expect(sidebar).toMatch(
      /\.sidebar-slot\.is-collapsed\s*\{[^}]*width:\s*var\(--collapsed-rail-w\)/s,
    );
    expect(sidebar).toMatch(
      /\.sidebar\.is-collapsed\s*\{[^}]*width:\s*var\(--collapsed-rail-w\)/s,
    );
  });
});

describe("shared window config stays portable", () => {
  const base = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
  const macos = JSON.parse(
    readFileSync("src-tauri/tauri.macos.conf.json", "utf8"),
  );
  const windows = JSON.parse(
    readFileSync("src-tauri/tauri.windows.conf.json", "utf8"),
  );
  const baseWin = base.app.windows[0];
  const macWin = macos.app.windows[0];
  const winWin = windows.app.windows[0];

  it("keeps overlay / private API / vibrancy out of the shared config", () => {
    expect(base.app.macOSPrivateApi).toBeUndefined();
    expect(baseWin.transparent).toBeUndefined();
    expect(baseWin.titleBarStyle).toBeUndefined();
    expect(baseWin.windowEffects).toBeUndefined();
  });

  it("keeps the macOS overlay frame in the macos merge file", () => {
    expect(macos.app.macOSPrivateApi).toBe(true);
    expect(macWin.titleBarStyle).toBe("Overlay");
    expect(macWin.transparent).toBe(true);
    expect(macWin.windowEffects.effects).toContain("underWindowBackground");
  });

  it("declares a decorated opaque Windows window", () => {
    expect(winWin.decorations).toBe(true);
    expect(winWin.transparent).toBe(false);
    expect(winWin.titleBarStyle).toBeUndefined();
    expect(winWin.windowEffects).toBeUndefined();
  });

  it("keeps the window size in lockstep across the three files", () => {
    for (const key of [
      "label",
      "title",
      "width",
      "height",
      "minWidth",
      "minHeight",
    ] as const) {
      expect(macWin[key]).toBe(baseWin[key]);
      expect(winWin[key]).toBe(baseWin[key]);
    }
  });
});
