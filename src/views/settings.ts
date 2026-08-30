//! App-wide preferences, live from the host (#26).
//!
//! `null` until the host answers, the way every other host read in this app
//! treats "not asked yet" — a preview build and a unit test both have no host,
//! and a pane that drew zeros while it waited would be showing settings nobody
//! chose.

import { useCallback, useEffect, useState } from "react";

import type { HostClient, SettingsSetParams, SettingsView } from "../host";

export interface Settings {
  /** `null` until the host answers. */
  settings: SettingsView | null;
  /** Why the last load or save failed, for the pane to say. */
  error: string | null;
  /**
   * Write what changed and take the host's whole answer as the new state.
   *
   * Resolves with it, or throws the host's error — the pane keeps what was
   * typed on a refusal, because a rejected value is one edit away from a
   * good one and retyping it is not.
   */
  save: (params: SettingsSetParams) => Promise<SettingsView>;
}

export function useSettings(client: HostClient | null): Settings {
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `settings/get` should leave the pane saying it cannot ask rather than
    // take the render down.
    (async () => client.settings())()
      .then((answer) => {
        if (cancelled) return;
        setSettings(answer);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  const save = useCallback(
    async (params: SettingsSetParams) => {
      if (!client) throw new Error("No host connection.");
      const saved = await client.saveSettings(params);
      // The host's answer is the state. Merging the patch into what we had
      // would drift the moment the host declines to apply something — which
      // it does, for the idle timeout, whenever the env var is in force.
      setSettings(saved);
      setError(null);
      return saved;
    },
    [client],
  );

  return { settings, error, save };
}
