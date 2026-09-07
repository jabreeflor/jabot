//! Settings (#26, #19, #29): the knobs that already decide something, and
//! the devices this Mac has admitted.
//!
//! Three records parked a preference here before the pane existed — the stuck
//! backstop's threshold, a remembered permission scope, the cron interval —
//! and D-018 said plainly that naming #26 for it had been optimistic, since
//! nothing in that issue's scope created a place to put one. So the threshold
//! was an env var on the host process, which a bundled app gives nobody.
//!
//! Host knobs stay scoped to what the host actually decides. Appearance (#178)
//! is a renderer preference parked on this pane because Settings is where a
//! person looks — it never goes over the wire. A remembered permission scope
//! still has no host support, and a pane offering a control that decides
//! nothing is worse than no pane.
//!
//! Minutes on screen, milliseconds on the wire. Nobody thinks about a
//! backstop in milliseconds, and the wire keeps them because that is what
//! every other duration on the protocol uses.
//!
//! Devices sit on a tab rather than a CODE row because pairing is a fact
//! about this Mac, not about a thread. Revoke is the answer to "my phone was
//! stolen"; burying it under Schedules made it look like a daily surface.

import { useEffect, useState } from "react";

import { Tabs, tabButtonId, type TabSpec } from "../components/Tabs";
import type {
  FoldPolicy,
  PairedDeviceView,
  SettingsView as HostSettings,
} from "../host";
import type { HarnessCard } from "../components/types";
import { type ThemePreference, useTheme } from "../theme";
import { DevicesView } from "./DevicesView";

type SettingsTab = "general" | "devices";

const TABS: readonly TabSpec<SettingsTab>[] = [
  { id: "general", label: "General" },
  { id: "devices", label: "Devices" },
];

/** Dark / Light / Match system. The stored value is the preference, not the
    resolved palette — "system" still has to mean something on the next launch. */
const APPEARANCES: ReadonlyArray<{
  id: ThemePreference;
  label: string;
  detail: string;
}> = [
  {
    id: "dark",
    label: "Dark",
    detail: "The shipped graphite palette.",
  },
  {
    id: "light",
    label: "Light",
    detail: "A cream-paper reading surface.",
  },
  {
    id: "system",
    label: "Match system",
    detail:
      "Follows this Mac's appearance. Changes when the OS does.",
  },
];

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
  harnesses = [],
  error,
  onSave,
  devices,
  devicesError,
  onReloadDevices,
  onRevokeDevice,
}: {
  /** `null` until the host answers — a preview build has no settings. */
  settings: HostSettings | null;
  harnesses?: readonly HarnessCard[];
  error?: string | null;
  onSave: (patch: {
    disabledHarnessIds?: string[];
    idleTimeoutMs?: number;
    defaultFoldPolicy?: FoldPolicy;
  }) => Promise<unknown>;
  /** `null` until the host answers. The console is always in a real list. */
  devices: readonly PairedDeviceView[] | null;
  devicesError: string | null;
  onReloadDevices: () => void;
  /** Rejects with the host's own sentence, which the row shows verbatim. */
  onRevokeDevice: (deviceId: string) => Promise<unknown>;
}) {
  const [tab, setTab] = useState<SettingsTab>("general");

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Settings</h1>
            <p>
              {tab === "devices"
                ? "Everything paired with this Mac. A device can answer permission prompts and read your Inbox — revoking one cuts it off immediately, including a connection it already has open."
                : "How JaBot looks, and what it does when you are not watching"}
            </p>
          </div>

          <Tabs
            label="Settings section"
            panelId="settings-panel"
            tabs={TABS}
            value={tab}
            onChange={setTab}
          />

          <div
            id="settings-panel"
            role="tabpanel"
            aria-labelledby={tabButtonId("settings-panel", tab)}
          >
            {tab === "general" ? (
              <GeneralSettings
                settings={settings}
                harnesses={harnesses}
                error={error}
                onSave={onSave}
              />
            ) : (
              <DevicesView
                embedded
                devices={devices}
                error={devicesError}
                onReload={onReloadDevices}
                onRevoke={onRevokeDevice}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function GeneralSettings({
  settings,
  harnesses,
  error,
  onSave,
}: {
  settings: HostSettings | null;
  harnesses: readonly HarnessCard[];
  error?: string | null;
  onSave: (patch: {
    disabledHarnessIds?: string[];
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
    disabledHarnessIds?: string[];
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
    <>
      <AppearanceSettings />

      <section className="settings-section" aria-label="Harnesses">
        <h2>Harnesses</h2>
        <p>Choose which harnesses appear when starting a chat or choosing a bot’s engine.</p>
        {harnesses.map((harness) => (
          <label className="settings-harness" key={harness.id}>
            <input type="checkbox" checked={!settings?.disabledHarnessIds?.includes(harness.id)}
              disabled={!settings || saving}
              onChange={(event) => {
                const ids = settings?.disabledHarnessIds ?? [];
                void send({ disabledHarnessIds: event.target.checked
                  ? ids.filter((id) => id !== harness.id) : [...ids, harness.id] });
              }} />
            <span><b>{harness.label}</b><small>{harness.available === false
              ? harness.installHint ?? "Not installed" : harness.blurb}
              {harness.capabilityNotes ? ` ${harness.capabilityNotes}` : ""}</small></span>
          </label>
        ))}
      </section>
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
    </>
  );
}

function AppearanceSettings() {
  const { preference, setPreference } = useTheme();

  return (
    <section className="setting">
      <h2>Appearance</h2>
      <p className="setting-note">
        Dark is the default so an existing install does not flip on
        upgrade. Match system follows this Mac&apos;s appearance via the
        OS color-scheme preference.
      </p>
      {APPEARANCES.map((choice) => (
        <label className="checkline" key={choice.id}>
          <input
            type="radio"
            name="appearance"
            checked={preference === choice.id}
            onChange={() => setPreference(choice.id)}
          />
          <span>
            {choice.label}
            <small>{choice.detail}</small>
          </span>
        </label>
      ))}
    </section>
  );
}
