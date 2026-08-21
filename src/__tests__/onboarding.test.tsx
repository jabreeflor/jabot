/**
 * First-run setup, end to end through <App/>: the gate renders the takeover
 * instead of the shell, the host connects *during* setup rather than after,
 * and what the panes capture is what the shell wears — the sidebar name, the
 * me-row initials, and New Chat's default harness.
 *
 * This is the one suite that simulates a first run: setup-dom.ts seeds every
 * jsdom as already-onboarded, and `clearOnboarding()` here is the opt-out.
 */
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { connectHost, type HelloResult, type HostClient } from "../host";
import { ONBOARDING_KEY } from "../onboarding/state";
import { clearOnboarding, seedOnboarded } from "../../tests/support/onboarding";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return { ...actual, connectHost: vi.fn() };
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

beforeEach(() => {
  connected.mockResolvedValue({
    client: { disconnect: vi.fn() } as unknown as HostClient,
    hello: HELLO,
  });
  clearOnboarding();
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** Render a first run and wait for the takeover's host footer to settle. */
async function renderFirstRun() {
  render(<App />);
  await screen.findByText("This Mac · v0.1.0");
}

async function walkToShell(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await user.click(screen.getByRole("button", { name: "Enter JaBot" }));
}

describe("Onboarding", () => {
  it("opens a first launch on the name pane, not the shell", async () => {
    await renderFirstRun();

    expect(
      screen.getByRole("heading", { name: /What should the crew call you/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^Inbox —/ }),
    ).not.toBeInTheDocument();
  });

  it("connects the host during setup, not after it", async () => {
    await renderFirstRun();

    // The footer under the card is the hoisted handshake made visible. The
    // exactly-once holds in this test env; the real app renders under
    // StrictMode, which double-invokes the effect in dev — this is not a
    // production call-count guarantee.
    expect(
      screen.getByRole("heading", { name: /What should the crew call you/ }),
    ).toBeInTheDocument();
    expect(connected).toHaveBeenCalledTimes(1);
  });

  it("walks the flow and lands in the shell wearing the typed name", async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    await user.type(
      screen.getByLabelText("YOUR NAME"),
      "Ada Lovelace{Enter}",
    );
    await user.click(screen.getByRole("button", { name: /Codex/ }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Enter JaBot" }));

    // The shell's first frame is the fixture fallback — Chief's chat, the
    // crew, the sidebar — now wearing the profile.
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.getByText("AL")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Chief" })).toBeInTheDocument();
    // The connection opened on pane 1 survived the swap into the shell.
    expect(connected).toHaveBeenCalledTimes(1);
  });

  it("makes the picked engine New Chat's default", async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    await user.click(screen.getByRole("button", { name: "Continue" }));
    // The first card is pre-selected before any click.
    expect(
      screen.getByRole("button", { name: /Claude Code/ }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: /Codex/ }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Enter JaBot" }));

    await user.click(screen.getByRole("button", { name: /New Chat/ }));
    const modal = screen.getByRole("dialog");
    expect(
      within(modal).getByRole("button", { name: /Codex/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      within(modal).getByRole("button", { name: /Claude Code/ }),
    ).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps what was typed across Back", async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    await user.type(screen.getByLabelText("YOUR NAME"), "Ada");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Back" }));

    expect(screen.getByLabelText("YOUR NAME")).toHaveValue("Ada");
  });

  it("leaves setup on Escape the way Skip does, keeping what was typed", async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    await user.type(screen.getByLabelText("YOUR NAME"), "Ada");
    await user.keyboard("{Escape}");

    expect(screen.getByRole("button", { name: /^Inbox —/ })).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
  });

  it('enters the shell as "You" when skipped with nothing typed', async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    await user.click(screen.getByRole("button", { name: "Skip setup" }));

    expect(screen.getByRole("button", { name: /^Inbox —/ })).toBeInTheDocument();
    expect(screen.getByText("You")).toBeInTheDocument();
  });

  it("does not run setup twice", async () => {
    const user = userEvent.setup();
    await renderFirstRun();
    await walkToShell(user);
    cleanup();

    // Real jsdom localStorage: this exercises the actual write → read trip.
    render(<App />);
    expect(
      screen.queryByRole("heading", { name: /What should the crew call you/ }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
  });

  it("honours a record from a previous launch", async () => {
    seedOnboarded({ userName: "Grace Hopper", harnessId: "pi" });
    render(<App />);

    expect(
      await screen.findByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Grace Hopper")).toBeInTheDocument();
  });

  it("honours a newer record without replaying or clobbering it", async () => {
    const raw = '{"version":2,"userName":"Grace Hopper","theme":"light"}';
    window.localStorage.setItem(ONBOARDING_KEY, raw);
    render(<App />);

    expect(
      await screen.findByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Grace Hopper")).toBeInTheDocument();
    // A downgrade neither re-onboards nor writes over the newer record.
    expect(window.localStorage.getItem(ONBOARDING_KEY)).toBe(raw);
  });

  it("enters the shell instead of looping when the store is unreadable", async () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    render(<App />);

    expect(
      await screen.findByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("You")).toBeInTheDocument();
    expect(setItem).not.toHaveBeenCalled();
  });

  it("moves focus with the panes", async () => {
    const user = userEvent.setup();
    await renderFirstRun();

    expect(screen.getByLabelText("YOUR NAME")).toHaveFocus();

    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      screen.getByRole("heading", { name: "Pick your default engine" }),
    ).toHaveFocus();
  });

  it("re-enters setup from Crew with the record intact and the draft seeded", async () => {
    const user = userEvent.setup();
    await renderFirstRun();
    await user.type(screen.getByLabelText("YOUR NAME"), "Ada");
    await walkToShell(user);

    await user.click(screen.getByRole("button", { name: /Crew/ }));
    await user.click(screen.getByRole("button", { name: "Run setup again" }));

    expect(
      screen.getByRole("heading", { name: /What should the crew call you/ }),
    ).toBeInTheDocument();
    // The wipe is deferred until finish: quitting the app mid-re-run must not
    // make the next launch a first run.
    expect(window.localStorage.getItem(ONBOARDING_KEY)).not.toBeNull();
    // And the draft starts from the record being edited — a re-run can
    // *change* a name, not only replace it.
    expect(screen.getByLabelText("YOUR NAME")).toHaveValue("Ada");
  });

  it("aborting a re-run keeps the stored name instead of resetting it", async () => {
    const user = userEvent.setup();
    await renderFirstRun();
    await user.type(screen.getByLabelText("YOUR NAME"), "Ada");
    await walkToShell(user);

    await user.click(screen.getByRole("button", { name: /Crew/ }));
    await user.click(screen.getByRole("button", { name: "Run setup again" }));
    await user.keyboard("{Escape}");

    expect(screen.getByRole("button", { name: /^Inbox —/ })).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
    const stored = JSON.parse(window.localStorage.getItem(ONBOARDING_KEY)!);
    expect(stored.userName).toBe("Ada");
  });

  it("keeps a newer record's version across a re-run", async () => {
    window.localStorage.setItem(
      ONBOARDING_KEY,
      '{"version":2,"userName":"Grace Hopper"}',
    );
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("button", { name: /^Inbox —/ });

    await user.click(screen.getByRole("button", { name: /Crew/ }));
    await user.click(screen.getByRole("button", { name: "Run setup again" }));
    await user.keyboard("{Escape}");

    // The re-run write carries the stored version instead of stamping 1 —
    // an older build must never downgrade a newer install's record.
    const stored = JSON.parse(window.localStorage.getItem(ONBOARDING_KEY)!);
    expect(stored.version).toBe(2);
    expect(stored.userName).toBe("Grace Hopper");
  });

  it("does not run setup twice after Skip", async () => {
    const user = userEvent.setup();
    await renderFirstRun();
    await user.type(screen.getByLabelText("YOUR NAME"), "Ada");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    cleanup();

    // Skip must write the record too, or the takeover replays every launch.
    render(<App />);
    expect(
      screen.queryByRole("heading", { name: /What should the crew call you/ }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();
  });
});
