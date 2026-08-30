//! The paired devices screen (#19, #29).
//!
//! `device/list` and `device/revoke` have been on the host and wrapped in the
//! client since #19, and nothing drew any of it — so the only way to see which
//! phones can answer your permission prompts, or to take one away, was a test.
//! That is the wrong shape for a security surface: revoke is the answer to "my
//! phone was stolen", and an answer that exists only in a protocol is not an
//! answer.
//!
//! Two decisions worth stating.
//!
//! **Revoked devices are tombstones, not deletions.** A row that vanishes
//! leaves the user unable to tell "I revoked it" from "it was never there", and
//! the second reading is the alarming one. The row stays, dimmed, saying when.
//!
//! **Revoke asks first, in place.** It cannot be undone — the device has to be
//! paired again from scratch — and the row is small enough that a modal would
//! be heavier than the decision. So the row turns into its own confirmation and
//! turns back if you change your mind.
//!
//! Scope is the list and revoke. The QR canvas and the safety-number sheet need
//! a QR encoder and a second device that can reach this host, and stay where
//! #19 left them.

import { useEffect, useState } from "react";

import type { PairedDeviceView } from "../host";
import { DeviceIcon } from "../components/Icon";
import { describeRole, describeVia, hasSas, shortFingerprint } from "./devices";
import { shortTime } from "./schedules";

/** `connected` and `lastSeenAt` go stale on their own, so the list asks again
    on the same cadence Schedules does. */
const POLL_MS = 10_000;

export function DevicesView({
  devices,
  error,
  onReload,
  onRevoke,
}: {
  /** `null` means the host has not answered. Different from `[]`, which is a
      list that should never be empty — this Mac's own console is always in it. */
  devices: readonly PairedDeviceView[] | null;
  error: string | null;
  onReload: () => void;
  /** Rejects with the host's own sentence, which the row shows verbatim. */
  onRevoke: (deviceId: string) => Promise<unknown>;
}) {
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setInterval(onReload, POLL_MS);
    return () => window.clearInterval(timer);
  }, [onReload]);

  function revoke(deviceId: string) {
    setBusy(deviceId);
    setFailure(null);
    onRevoke(deviceId)
      .then(() => setConfirming(null))
      .catch((err: unknown) =>
        setFailure(err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setBusy(null));
  }

  const rows = devices ?? [];

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page dev-page">
          <div className="page-top">
            <h1>Devices</h1>
            <p>
              Everything paired with this Mac. A device can answer permission
              prompts and read your Inbox — revoking one cuts it off
              immediately, including a connection it already has open.
            </p>
          </div>

          {error && (
            <p className="modal-error" role="alert">
              {error}
            </p>
          )}
          {failure && (
            <p className="modal-error" role="alert">
              {failure}
            </p>
          )}

          {devices === null && !error && (
            <div className="page-empty">Asking the host…</div>
          )}

          {devices !== null && rows.length === 0 && (
            <div className="page-empty">No devices are paired with this Mac.</div>
          )}

          {rows.length > 0 && (
            <ul className="dev-list">
              {rows.map((device) => (
                <DeviceRow
                  key={device.deviceId}
                  device={device}
                  confirming={confirming === device.deviceId}
                  busy={busy === device.deviceId}
                  onAsk={() => {
                    setFailure(null);
                    setConfirming(device.deviceId);
                  }}
                  onCancel={() => setConfirming(null)}
                  onConfirm={() => revoke(device.deviceId)}
                />
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

function DeviceRow({
  device,
  confirming,
  busy,
  onAsk,
  onCancel,
  onConfirm,
}: {
  device: PairedDeviceView;
  confirming: boolean;
  busy: boolean;
  onAsk: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const revoked = Boolean(device.revokedAt);
  return (
    <li className={`dev-row${revoked ? " is-revoked" : ""}`}>
      <span className="dev-mark" aria-hidden="true">
        <DeviceIcon />
      </span>
      <div className="dev-main">
        <div className="dev-name">
          {device.name}
          {device.local && <span className="dev-tag">This Mac</span>}
          {/* Only for a device that is here *now*. A dot on a revoked row
              would be describing a connection the host has already cut. */}
          {device.connected && !revoked && (
            <span className="dev-live" title="Connected now">
              <span className="dot" aria-hidden="true" />
              Connected
            </span>
          )}
        </div>
        <div className="dev-meta">
          {revoked ? (
            <span className="dev-dead">Revoked {shortTime(device.revokedAt)}</span>
          ) : (
            <span>{describeRole(device.role)}</span>
          )}
          <span className="sep" aria-hidden="true">
            ·
          </span>
          <span>{describeVia(device.pairedVia)}</span>
          {/* No "last seen" for the console: the host does not track one for
              it, and "Last seen never" about the machine you are sitting at
              would be the screen's most obviously wrong sentence. */}
          {!device.local && (
            <>
              <span className="sep" aria-hidden="true">
                ·
              </span>
              {/* "never" is a real answer for a device paired and never used
                  since. `shortTime` says so rather than leaving a gap. */}
              <span title={device.lastSeenAt ?? "never seen"}>
                Last seen {shortTime(device.lastSeenAt)}
              </span>
            </>
          )}
        </div>
        <div className="dev-keys">
          {/* The safety number is the thing the two humans read to each other
              when they paired. Keeping it on the row is what lets somebody
              check, months later, that this really is that device. The console
              has none — nothing was compared — and the host says so with an em
              dash, which is not a number to put under that heading. */}
          {hasSas(device.sas) && (
            <span className="dev-sas" title="The safety number you compared">
              {device.sas}
            </span>
          )}
          <code className="dev-fp" title={device.fingerprint}>
            {shortFingerprint(device.fingerprint)}
          </code>
        </div>
      </div>

      <div className="dev-actions">
        {device.local ? (
          // Not a disabled button: the host refuses this outright, and a
          // greyed control invites a click that would only ever fail.
          <span className="dev-note">Cannot be revoked</span>
        ) : revoked ? null : confirming ? (
          <>
            <button
              type="button"
              className="btn danger sm"
              disabled={busy}
              onClick={onConfirm}
            >
              {busy ? "Revoking…" : "Revoke"}
            </button>
            <button type="button" className="btn ghost sm" onClick={onCancel}>
              Keep
            </button>
          </>
        ) : (
          <button
            type="button"
            className="btn ghost sm"
            onClick={onAsk}
            aria-label={`Revoke ${device.name}`}
          >
            Revoke…
          </button>
        )}
      </div>
    </li>
  );
}
