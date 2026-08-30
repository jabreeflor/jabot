/**
 * The paired devices screen (#19, #29).
 *
 * `device/list` and `device/revoke` have been on the host since #19 and
 * nothing drew them, so the only way to see what could reach your Mac — or to
 * take one away — was to write a test. Revoke is the answer to "my phone was
 * stolen", and an answer that exists only in a protocol is not one.
 *
 * What is asserted here is the part a live host cannot check: that a revoked
 * device stays on screen as a tombstone, that the local console offers no
 * control the host would refuse anyway, and that a revoke is a decision the
 * user confirms rather than a single click on a row.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DevicesView } from "../views/DevicesView";
import type { PairedDeviceView } from "../host";

const console_: PairedDeviceView = {
  deviceId: "dev-console",
  name: "Jabree's MacBook Pro",
  role: "full",
  fingerprint: "AAAAbbbbCCCCddddEEEEffffGGGG",
  pairedVia: "local",
  // Exactly what `device_list` synthesizes: nothing was compared, so there is
  // no safety number, and the host tracks no last-seen for its own console.
  sas: "—",
  createdAt: "2026-08-01T09:00:00Z",
  local: true,
  connected: true,
};

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

const revoked: PairedDeviceView = {
  ...phone,
  deviceId: "dev-old",
  name: "Old iPad",
  sas: "9930-1155",
  revokedAt: "2026-08-20T11:05:00Z",
  // Deliberately still `connected` in the fixture: the host cuts the socket,
  // and a row that drew a live dot off a stale flag would be claiming a
  // connection that no longer exists.
  connected: true,
  local: false,
};

function draw(over: Partial<Parameters<typeof DevicesView>[0]> = {}) {
  const props = {
    devices: [console_, phone, revoked],
    error: null,
    onReload: vi.fn(),
    onRevoke: vi.fn(async () => undefined),
    ...over,
  };
  const view = render(<DevicesView {...props} />);
  return { ...props, ...view };
}

const rowFor = (name: string) =>
  screen.getByText(name).closest("li") as HTMLElement;

describe("DevicesView", () => {
  it("shows what each device may do and when it was last here", () => {
    draw();

    expect(
      within(rowFor("Jabree's iPhone")).getByText(
        "Answer questions and read the Inbox",
      ),
    ).toBeInTheDocument();
    expect(
      within(rowFor("Jabree's MacBook Pro")).getByText(
        "Everything this Mac can do",
      ),
    ).toBeInTheDocument();
    // The safety number the two humans compared, kept on the row so somebody
    // can still check months later that this is that device.
    expect(within(rowFor("Jabree's iPhone")).getByText("1174-6602")).toBeInTheDocument();
    expect(within(rowFor("Jabree's iPhone")).getByText("Paired by QR")).toBeInTheDocument();
  });

  /**
   * The console is not a paired device in the sense the rest of the list is:
   * it spawned the host. The host says so — `pairedVia: "local"`, `sas: "—"`,
   * no `lastSeenAt` — and the row has to say the same rather than dressing
   * those up as an ordinary pairing. "Paired via local", an em dash under
   * "safety number", and "Last seen never" about the machine you are sitting
   * at would all be the screen lying quietly.
   */
  it("does not dress the console up as an ordinary pairing", () => {
    draw();

    const row = rowFor("Jabree's MacBook Pro");
    expect(within(row).getByText("Paired by spawning this host")).toBeInTheDocument();
    expect(within(row).queryByText(/Paired via/)).toBeNull();
    expect(within(row).queryByText(/Last seen/)).toBeNull();
    expect(within(row).queryByText("—")).toBeNull();
    // The fingerprint is still real and still shown: it is what a second
    // device compares against when it pairs.
    expect(within(row).getByTitle(console_.fingerprint)).toBeInTheDocument();

    // And a real pairing keeps all three.
    const phoneRow = rowFor("Jabree's iPhone");
    expect(within(phoneRow).getByText("1174-6602")).toBeInTheDocument();
    expect(within(phoneRow).getByText(/Last seen /)).toBeInTheDocument();
  });

  /**
   * The decision this screen turns on. A row that vanished would leave "I
   * revoked it" and "it was never paired" looking identical, and only one of
   * those readings is reassuring.
   */
  it("keeps a revoked device on screen as a tombstone", () => {
    draw();

    const row = rowFor("Old iPad");
    expect(row).toHaveClass("is-revoked");
    expect(within(row).getByText(/^Revoked /)).toBeInTheDocument();
    // No control: there is nothing left to do to it.
    expect(within(row).queryByRole("button")).toBeNull();
    // And no live dot, whatever the stale flag says — the host cut it.
    expect(within(row).queryByText("Connected")).toBeNull();
  });

  it("offers no revoke on the console, because the host refuses it", () => {
    draw();

    const row = rowFor("Jabree's MacBook Pro");
    // Not a disabled button: the host refuses this outright, and a greyed
    // control invites a click that could only ever fail.
    expect(within(row).queryByRole("button")).toBeNull();
    expect(within(row).getByText("Cannot be revoked")).toBeInTheDocument();
  });

  /** Revoke cannot be undone — the device has to be paired again from scratch
      — so one click is not enough. */
  it("asks before it revokes, and can be talked out of it", async () => {
    const props = draw();

    await userEvent.click(
      screen.getByRole("button", { name: "Revoke Jabree's iPhone" }),
    );
    const row = rowFor("Jabree's iPhone");
    await userEvent.click(within(row).getByRole("button", { name: "Keep" }));

    expect(props.onRevoke).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Revoke Jabree's iPhone" }),
    ).toBeInTheDocument();
  });

  it("revokes the device the row is about", async () => {
    const props = draw();

    await userEvent.click(
      screen.getByRole("button", { name: "Revoke Jabree's iPhone" }),
    );
    await userEvent.click(
      within(rowFor("Jabree's iPhone")).getByRole("button", { name: "Revoke" }),
    );

    await waitFor(() => expect(props.onRevoke).toHaveBeenCalledWith("dev-phone"));
  });

  /** The host's sentence, not ours. "The local device cannot be revoked; it is
      the host's own console" says where to look; "could not revoke" does not. */
  it("says the host's own refusal", async () => {
    draw({
      onRevoke: vi.fn(async () => {
        throw new Error("the local device cannot be revoked");
      }),
    });

    await userEvent.click(
      screen.getByRole("button", { name: "Revoke Jabree's iPhone" }),
    );
    await userEvent.click(
      within(rowFor("Jabree's iPhone")).getByRole("button", { name: "Revoke" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "the local device cannot be revoked",
    );
  });

  /** `null` is "not asked yet". Drawing "no devices are paired" while the host
      is still thinking would be telling the user something false about their
      own machine — this Mac's console is always in that list. */
  it("waits rather than claiming an empty list before the host answers", () => {
    draw({ devices: null });

    expect(screen.getByText("Asking the host…")).toBeInTheDocument();
    expect(screen.queryByText(/No devices/)).toBeNull();
  });

  it("says why when the host will not answer at all", () => {
    draw({ devices: null, error: "store is unavailable" });

    expect(screen.getByRole("alert")).toHaveTextContent("store is unavailable");
    expect(screen.queryByText("Asking the host…")).toBeNull();
  });

  it("draws a live dot only for a device that is here now", () => {
    draw();

    expect(
      within(rowFor("Jabree's MacBook Pro")).getByText("Connected"),
    ).toBeInTheDocument();
    expect(within(rowFor("Jabree's iPhone")).queryByText("Connected")).toBeNull();
  });
});
