/**
 * Self-update banner: what each `UpdateStatus` from updates.rs puts on
 * screen, and that Restart is the one thing that reaches the host.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { UpdateBanner } from "../components/UpdateBanner";
import { bannerFor, previewUpdateStatus, type UpdateStatus } from "../updates";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

let emit: (status: UpdateStatus) => void = () => {};

beforeEach(() => {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  listenMock.mockImplementation((_event, handler) => {
    emit = (status) =>
      (handler as (e: { payload: UpdateStatus }) => void)({ payload: status });
    return Promise.resolve(() => {});
  });
  invokeMock.mockImplementation((cmd) =>
    cmd === "app_update_status"
      ? Promise.resolve({ state: "idle" })
      : Promise.resolve(undefined),
  );
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.clearAllMocks();
});

async function mount() {
  render(<UpdateBanner />);
  await act(async () => {});
}

describe("bannerFor", () => {
  it("always shows a staged update", () => {
    expect(
      bannerFor({ state: "ready", version: "0.1.8", current: "0.1.7" }),
    ).toEqual({ tone: "ready", text: "JaBot 0.1.8 is ready." });
  });

  it("stays quiet about background checks", () => {
    expect(
      bannerFor({ state: "current", version: "0.1.7", manual: false }),
    ).toBeNull();
    expect(
      bannerFor({ state: "failed", message: "offline", manual: false }),
    ).toBeNull();
    expect(bannerFor({ state: "checking", manual: false })).toBeNull();
    expect(bannerFor({ state: "idle" })).toBeNull();
    expect(bannerFor({ state: "disabled", reason: "Local" })).toBeNull();
  });

  it("answers a check the user asked for", () => {
    expect(
      bannerFor({ state: "current", version: "0.1.7", manual: true })?.text,
    ).toMatch(/latest JaBot \(0\.1\.7\)/);
    expect(
      bannerFor({ state: "failed", message: "offline", manual: true }),
    ).toEqual({
      tone: "error",
      text: "Couldn’t check for updates: offline",
    });
  });
});

describe("previewUpdateStatus", () => {
  it("previews only on the documented param", () => {
    expect(previewUpdateStatus("?update=ready")?.state).toBe("ready");
    expect(previewUpdateStatus("?update=current")?.state).toBe("current");
    expect(previewUpdateStatus("?update=nope")).toBeNull();
    expect(previewUpdateStatus("")).toBeNull();
  });
});

describe("<UpdateBanner />", () => {
  it("renders nothing until there is something to say", async () => {
    await mount();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("picks up an update that was staged before it mounted", async () => {
    invokeMock.mockImplementation((cmd) =>
      cmd === "app_update_status"
        ? Promise.resolve({
            state: "ready",
            version: "0.1.8",
            current: "0.1.7",
          })
        : Promise.resolve(undefined),
    );
    await mount();
    expect(screen.getByRole("status").textContent).toContain(
      "JaBot 0.1.8 is ready.",
    );
  });

  it("restarts into the update when asked", async () => {
    await mount();
    act(() => emit({ state: "ready", version: "0.1.8", current: "0.1.7" }));
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    expect(invokeMock).toHaveBeenCalledWith("app_update_restart");
    expect(
      (screen.getByRole("button", { name: "Restarting…" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("says why a restart failed and lets the user retry", async () => {
    await mount();
    act(() => emit({ state: "ready", version: "0.1.8", current: "0.1.7" }));
    invokeMock.mockRejectedValueOnce(new Error("disk full"));
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    await act(async () => {});
    expect(screen.getByRole("status").textContent).toContain("disk full");
    expect(screen.getByRole("button", { name: "Restart" })).toBeTruthy();
  });

  it("dismisses one message without muting the next", async () => {
    await mount();
    act(() => emit({ state: "current", version: "0.1.7", manual: true }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status")).toBeNull();
    act(() => emit({ state: "ready", version: "0.1.8", current: "0.1.7" }));
    expect(screen.getByRole("status").textContent).toContain("0.1.8");
  });

  it("is inert outside the desktop app", async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await mount();
    expect(listenMock).not.toHaveBeenCalled();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
