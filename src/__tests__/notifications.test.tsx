/**
 * Clicking a native notification opens the thread it names (#27).
 *
 * Delivery is macOS-only and needs a signed app bundle, so what is testable
 * here is the half that runs in the renderer: the shell subscribes to the
 * activation event, and an activation moves the main pane onto that thread.
 * The Rust side of the same journey — which Inbox kinds ring, what rides in
 * `userInfo`, and that a click decodes back to the thread it was sent for — is
 * asserted in `src-tauri/src/notify/mod.rs`.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import {
  connectHost,
  onNotificationActivated,
  type HelloResult,
  type HostClient,
  type NotificationActivated,
} from "../host";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return {
    ...actual,
    connectHost: vi.fn(),
    onNotificationActivated: vi.fn(),
  };
});

const HELLO: HelloResult = {
  protocolVersion: 1,
  hostId: "host-1",
  hostName: "This Mac",
  hostMode: "in-process",
  version: "0.1.0",
  platform: "macos",
  device: { deviceId: "dev-1", name: "This Mac", role: "full" },
  methods: [],
  notifications: [],
};

const connected = vi.mocked(connectHost);
const activated = vi.mocked(onNotificationActivated);

/** The handler the shell registered, so a test can be the operating system. */
let click: ((event: NotificationActivated) => void) | null = null;
const unlisten = vi.fn();

beforeEach(() => {
  click = null;
  unlisten.mockClear();
  connected.mockResolvedValue({
    client: { disconnect: vi.fn() } as unknown as HostClient,
    hello: HELLO,
  });
  activated.mockImplementation(async (handler) => {
    click = handler;
    return unlisten;
  });
});

async function renderApp() {
  render(<App />);
  await screen.findByText("This Mac · v0.1.0");
  await waitFor(() => expect(click).not.toBeNull());
}

describe("notification clicks", () => {
  it("opens the thread the banner named", async () => {
    await renderApp();
    // The shell opens on Chief, not on a thread.
    expect(screen.getByRole("heading", { name: "Chief" })).toBeInTheDocument();

    await act(async () => {
      click?.({ threadId: "auth", kind: "done" });
    });

    expect(
      screen.getByRole("heading", { name: "Auth migration" }),
    ).toBeInTheDocument();
  });

  /**
   * A banner can outlive the thread it was about — the user deleted it, or a
   * different Mac did. Pointing at the Inbox is the honest answer; a blank pane
   * or a crash is not.
   */
  it("says so rather than blanking when the thread is gone", async () => {
    await renderApp();

    await act(async () => {
      click?.({ threadId: "a-thread-that-never-existed", kind: "failed" });
    });

    expect(screen.getByText(/That thread is gone/)).toBeInTheDocument();
  });

  /**
   * The real subscriber, in a world with no Tauri event bus — a preview build,
   * or this test. A refused or absent OS integration has to degrade to the
   * in-app Inbox, which starts with not taking the render down.
   */
  it("subscribes to nothing rather than throwing outside the app", async () => {
    const host = await vi.importActual<typeof import("../host")>("../host");
    const off = await host.onNotificationActivated(() => {});
    expect(() => off()).not.toThrow();
  });

  it("stops listening when the shell unmounts", async () => {
    const { unmount } = render(<App />);
    await screen.findByText("This Mac · v0.1.0");
    await waitFor(() => expect(click).not.toBeNull());

    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });
});
