//! Settings (#26): the two knobs that already decide something.
//!
//! Three records parked a preference here before the pane existed — the stuck
//! backstop's threshold, a remembered permission scope, the cron interval —
//! and D-018 said plainly that naming #26 for it had been optimistic, since
//! nothing in that issue's scope created a place to put one. So the threshold
//! was an env var on the host process, which a bundled app gives nobody.
//!
//! Two controls, not five. A remembered permission scope has no host support
//! at all, and a pane offering a control that decides nothing is worse than no
//! pane — the user would set it, and it would do nothing, and they would have
//! no way to find that out.
//!
//! Minutes on screen, milliseconds on the wire. Nobody thinks about a
//! backstop in milliseconds, and the wire keeps them because that is what
//! every other duration on the protocol uses.

import { useEffect, useState } from "react";

import type { FoldPolicy, SettingsView as HostSettings } from "../host";

/** The two the fold path accepts, with what each actually does. The wording is
    the fold menu's, because they are the same choice — this one is just the
    answer a thread starts with. */
const POLICIES: ReadonlyArray<{
  id: FoldPolicy;
  label: string;
  detail: string;
}> = [
  {
    id: "default",
    label: "Disappear until done",
    detail:
      "Keeps working while it is folded. Comes back to the Inbox when it finishes, fails, or needs you.",
  },
  {
    id: "wait_for_inbox",
    label: "Wait for Inbox",
    detail:
      "Quieter: reads are allowed while you are away, never an execute or a delete.",
  },
];

export function SettingsView({
  settings,
  error,
  onSave,
}: {
  /** `null` until the host answers — a preview build has no settings. */
  settings: HostSettings | null;
  error?: string | null;
  onSave: (patch: {
    idleTimeoutMs?: number;
    defaultFoldPolicy?: FoldPolicy;
  }) => Promise<unknown>;
}) {
  const [minutes, setMinutes] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  // Seeded from the host, and re-seeded when it answers again: a save returns
  // the whole view, and the field has to show what was actually stored rather
  // than what was typed at it.
  useEffect(() => {
    if (settings) setMinutes(String(Math.round(settings.idleTimeoutMs / 60_000)));
  }, [settings]);

  async function send(patch: {
    idleTimeoutMs?: number;
    defaultFoldPolicy?: FoldPolicy;
  }) {
    setSaving(true);
    setSaveError(null);
    setSaved(false);
    try {
      await onSave(patch);
      setSaved(true);
    } catch (err) {
      // The host's own sentence. It refuses out-of-range values rather than
      // clamping them, so "must be between 1000 and 86400000" is the useful
      // thing to say — not "could not save".
      setSaveError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Settings</h1>
            <p>What JaBot does when you are not watching</p>
          </div>

          {error && (
            <div className="page-empty" role="alert">
              {error}
            </div>
          )}

          {!settings && !error && (
            <div className="page-empty">Asking the host…</div>
          )}

          {settings && (
            <>
              <section className="setting">
                <h2>Go quiet after</h2>
                <p className="setting-note">
                  How long a running thread can say nothing before it comes back
                  to the Inbox as stuck. The thread keeps working and its
                  process stays alive — this is a nudge, not a timeout.
                </p>
                <div className="setting-row">
                  <input
                    type="number"
                    min={1}
                    max={1440}
                    aria-label="Go quiet after, in minutes"
                    value={minutes}
                    disabled={settings.idleTimeoutFromEnv}
                    onChange={(event) => setMinutes(event.target.value)}
                  />
                  <span className="unit">minutes</span>
                  <button
                    type="button"
                    className="btn"
                    disabled={saving || settings.idleTimeoutFromEnv}
                    onClick={() =>
                      void send({
                        idleTimeoutMs: Math.round(Number(minutes) * 60_000),
                      })
                    }
                  >
                    {saving ? "Saving…" : "Save"}
                  </button>
                </div>
                {/* Said out loud rather than silently ignored: a control that
                    does nothing and does not say so is worse than a disabled
                    one. Only a test or a developer is ever in this state. */}
                {settings.idleTimeoutFromEnv && (
                  <p className="setting-note" role="status">
                    Set by <code>JABOT_IDLE_TIMEOUT_MS</code> on this host, which
                    wins over anything saved here.
                  </p>
                )}
              </section>

              <section className="setting">
                <h2>New threads fold as</h2>
                <p className="setting-note">
                  What a thread's fold policy starts as. Every thread can still
                  be folded either way from its own menu — this is only the
                  answer it begins with.
                </p>
                {POLICIES.map((policy) => (
                  <label className="checkline" key={policy.id}>
                    <input
                      type="radio"
                      name="fold-policy"
                      checked={settings.defaultFoldPolicy === policy.id}
                      disabled={saving}
                      onChange={() =>
                        void send({ defaultFoldPolicy: policy.id })
                      }
                    />
                    <span>
                      {policy.label}
                      <small>{policy.detail}</small>
                    </span>
                  </label>
                ))}
              </section>

              {saveError && (
                <p className="page-note" role="alert">
                  {saveError}
                </p>
              )}
              {saved && !saveError && (
                <p className="page-note" role="status">
                  Saved.
                </p>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
