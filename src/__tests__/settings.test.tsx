/**
 * Settings (#26).
 *
 * The pane exists because three decision records parked a preference on a
 * surface that did not exist, and the stuck backstop's threshold ended up as
 * an env var on the host process — which a bundled app gives nobody.
 *
 * What is asserted here is the half a live host cannot check: that the pane
 * shows the host's values, sends only what changed, says the host's own
 * refusal rather than "could not save", and does not draw a control that
 * decides nothing.
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { THEME_KEY } from "../theme";
import { SettingsView } from "../views/SettingsView";
import type { PairedDeviceView, SettingsView as HostSettings } from "../host";

const SETTINGS: HostSettings = {
  idleTimeoutMs: 600_000,
  defaultFoldPolicy: "default",
  idleTimeoutFromEnv: false,
};

function draw(over: Partial<Parameters<typeof SettingsView>[0]> = {}) {
  const props = {
    settings: SETTINGS,
    onSave: vi.fn(async () => SETTINGS),
    devices: null,
    devicesError: null,
    onReloadDevices: vi.fn(),
    onRevokeDevice: vi.fn(async () => undefined),
    ...over,
  };
  render(<SettingsView {...props} />);
  return props;
}

const minutes = () => screen.getByLabelText(/Go quiet after/);

describe("SettingsView", () => {
  it("declares Copilot capabilities instead of implying resume works", () => {
    draw({
      harnesses: [{
        id: "copilot",
        label: "GitHub Copilot",
        accent: "var(--h-copilot)",
        blurb: "GitHub's coding agent, over ACP",
        capabilities: {
          streaming: true,
          toolEvents: true,
          permissions: true,
          cancel: true,
          resume: false,
          notes: "Resume after the Copilot process exits is not supported.",
        },
      }],
    });
    expect(screen.getByText(/Resume after the Copilot process exits is not supported/)).toBeVisible();
  });

  it("enables a missing adapter while retaining other disabled harnesses and install guidance", async () => {
    const props = draw({
      settings: { ...SETTINGS, disabledHarnessIds: ["pi", "custom"] },
      harnesses: [{ id: "pi", label: "Pi", accent: "red", blurb: "Pi agent", available: false, installHint: "Install Pi adapter" }],
    });
    expect(screen.getByRole("checkbox", { name: /Pi/ })).not.toBeChecked();
    expect(screen.getByText("Install Pi adapter")).toBeVisible();
    await userEvent.click(screen.getByRole("checkbox", { name: /Pi/ }));
    expect(props.onSave).toHaveBeenCalledWith({ disabledHarnessIds: ["custom"] });
  });

  it("shows the host's values, in the units a person thinks in", () => {
    draw();

    // Ten minutes, not six hundred thousand milliseconds.
    expect(minutes()).toHaveValue(10);
    expect(
      screen.getByRole("radio", { name: /Disappear until done/ }),
    ).toBeChecked();
  });

  it("sends the timeout back in milliseconds", async () => {
    const props = draw();

    await userEvent.clear(minutes());
    await userEvent.type(minutes(), "2");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(props.onSave).toHaveBeenCalledWith({ idleTimeoutMs: 120_000 }),
    );
  });

  /** A patch, not the form. The host reads an absent field as "leave it
      alone", so changing the fold default must not resend a timeout the user
      never touched. */
  it("sends only the control that was used", async () => {
    const props = draw();

    await userEvent.click(screen.getByRole("radio", { name: /Wait for Inbox/ }));

    await waitFor(() =>
      expect(props.onSave).toHaveBeenCalledWith({
        defaultFoldPolicy: "wait_for_inbox",
      }),
    );
  });

  /**
   * The host refuses out-of-range values rather than clamping them, so its
   * sentence is the useful one. "Could not save" would send somebody looking
   * in the wrong place.
   */
  it("says the host's own refusal", async () => {
    draw({
      onSave: vi.fn(async () => {
        throw new Error("idleTimeoutMs must be between 1000 and 86400000, not 5");
      }),
    });

    await userEvent.clear(minutes());
    await userEvent.type(minutes(), "0");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "must be between 1000 and 86400000",
    );
  });

  it("takes the host's answer as the new state, not what was typed", async () => {
    // The host stored 90s and said so; the field has to show what was stored.
    const devices = {
      devices: null,
      devicesError: null,
      onReloadDevices: vi.fn(),
      onRevokeDevice: vi.fn(async () => undefined),
    };
    const { rerender } = render(
      <SettingsView
        settings={SETTINGS}
        onSave={vi.fn(async () => SETTINGS)}
        {...devices}
      />,
    );
    rerender(
      <SettingsView
        settings={{ ...SETTINGS, idleTimeoutMs: 90_000 }}
        onSave={vi.fn(async () => SETTINGS)}
        {...devices}
      />,
    );

    expect(minutes()).toHaveValue(2);
  });

  /**
   * A control that does nothing and does not say so is worse than a disabled
   * one. Only a test or a developer is ever in this state, and both would
   * rather be told than quietly ignored.
   */
  it("disables the timeout and says why when the environment is in force", () => {
    draw({ settings: { ...SETTINGS, idleTimeoutMs: 1500, idleTimeoutFromEnv: true } });

    expect(minutes()).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByText(/JABOT_IDLE_TIMEOUT_MS/)).toBeInTheDocument();
    // And the number shown is the one actually in force, not what is stored.
    expect(minutes()).toHaveValue(0);
  });

  /** `null` is "not asked yet", which a preview build and a unit test both
      are. Drawing zeros would be showing settings nobody chose. Appearance
      is local, so it still paints — it does not need the host. */
  it("waits rather than inventing values before the host answers", () => {
    draw({ settings: null });

    expect(screen.getByText("Asking the host…")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.getByRole("radio", { name: /Dark/ })).toBeChecked();
  });

  /**
   * Appearance is a renderer preference. Switching it must paint the
   * document and remember the choice without asking the host — a theme
   * that waits on settings/set flashes the shipped dark palette.
   */
  it("applies light theme to the document and remembers it", async () => {
    const props = draw();

    expect(screen.getByRole("radio", { name: /Dark/ })).toBeChecked();
    await userEvent.click(screen.getByRole("radio", { name: /^Light/ }));

    expect(screen.getByRole("radio", { name: /^Light/ })).toBeChecked();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem(THEME_KEY)).toBe("light");
    expect(props.onSave).not.toHaveBeenCalled();
  });

  it("says why when the host will not answer at all", () => {
    draw({ settings: null, error: "store is unavailable" });

    expect(screen.getByRole("alert")).toHaveTextContent("store is unavailable");
    expect(screen.queryByText("Asking the host…")).toBeNull();
  });

  /**
   * Pairing used to be a CODE row under Schedules. It is a fact about this
   * Mac, so it lives here — a tab, not a second heading, because the pane is
   * still Settings.
   */
  it("keeps Devices as a tab, and leaves General selected", () => {
    draw();

    expect(screen.getByRole("tab", { name: "General" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: "Devices" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(minutes()).toBeInTheDocument();
    expect(screen.queryByText("This Mac")).toBeNull();
  });

  it("shows the paired list when Devices is selected", async () => {
    const phone: PairedDeviceView = {
      deviceId: "dev-phone",
      name: "Jabree's iPhone",
      role: "approver",
      fingerprint: "ZZZZyyyyXXXXwwwwVVVVuuuuTTTT",
      pairedVia: "qr",
      sas: "1174-6602",
      createdAt: "2026-08-12T18:20:00Z",
      lastSeenAt: "2026-08-29T21:40:00Z",
      local: false,
      connected: false,
    };
    draw({ devices: [phone] });

    await userEvent.click(screen.getByRole("tab", { name: "Devices" }));

    expect(screen.getByRole("tab", { name: "Devices" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Jabree's iPhone")).toBeInTheDocument();
    expect(screen.queryByLabelText(/Go quiet after/)).toBeNull();
  });
});
