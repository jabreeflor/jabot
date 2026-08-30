//! Paired devices, live from the host (#19, #29).
//!
//! The shape of `schedules.ts`, and for the same reason: `device/list` already
//! answers with everything a row draws — name, role, fingerprint, the safety
//! number the two humans compared, when it was paired and last seen, whether
//! it is connected right now, whether it has been revoked. This is a rename
//! from wire shape to prop shape and nothing else.
//!
//! `devices` stays `null` until the host answers. `[]` never happens in
//! practice — the console that spawned the host is always in the list — but
//! the distinction is kept because a screen that draws "no devices" while the
//! host is still thinking is telling the user something false about their own
//! machine.

import { useCallback, useEffect, useState } from "react";

import type { HostClient, PairedDeviceView } from "../host";

export interface Devices {
  /** `null` until the host answers. */
  devices: PairedDeviceView[] | null;
  error: string | null;
  reload: () => void;
  /** Revoke, then re-list. Rejects with the host's own sentence — "the local
      device cannot be revoked; it is the host's own console" is a better thing
      to show than "could not revoke". */
  revoke: (deviceId: string) => Promise<void>;
}

export function useDevices(client: HostClient | null): Devices {
  const [devices, setDevices] = useState<PairedDeviceView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `device/list` — a unit test's stub, an older host — should leave the
    // screen empty rather than take the render down.
    (async () => client.listDevices())()
      .then((listed) => {
        if (cancelled) return;
        setDevices(listed.devices);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const revoke = useCallback(
    async (deviceId: string) => {
      if (!client) throw new Error("No host connection.");
      await client.revokeDevice({ deviceId });
      reload();
    },
    [client, reload],
  );

  return { devices, error, reload, revoke };
}

/** What a device is *for*, in the words the pairing screen used.
 *
 *  The role is the only thing on the row that decides what the device can do
 *  to this Mac, so it is spelled out rather than shown as a bare token. */
export function describeRole(role: string): string {
  switch (role) {
    case "full":
      return "Everything this Mac can do";
    case "approver":
      return "Answer questions and read the Inbox";
    default:
      return role;
  }
}

/** How the pairing was carried, for the row's detail line.
 *
 *  `local` is not a channel and does not get "Paired via local": the console
 *  spawned the host, which is why it is device #1 and why nothing was ever
 *  compared. Saying what actually happened is the honest line. */
export function describeVia(via: string): string {
  switch (via) {
    case "qr":
      return "Paired by QR";
    case "code":
      return "Paired by code";
    case "local":
      return "Paired by spawning this host";
    default:
      return `Paired via ${via}`;
  }
}

/** Whether there is a safety number worth drawing.
 *
 *  The host sends `"—"` for the console, deliberately: nothing was compared,
 *  and it says so rather than inventing a number it never showed anyone. A row
 *  that drew that em dash under a "safety number" label would put the lie back. */
export function hasSas(sas: string): boolean {
  const trimmed = sas.trim();
  return trimmed.length > 0 && trimmed !== "—" && trimmed !== "-";
}

/** A fingerprint short enough to read out and long enough to mean something.
 *
 *  Truncated in the middle rather than the end: the tail is what differs
 *  between two keys generated a second apart, so an ellipsis at the end would
 *  hide exactly the part somebody is comparing. */
export function shortFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 20) return fingerprint;
  return `${fingerprint.slice(0, 10)}…${fingerprint.slice(-6)}`;
}
