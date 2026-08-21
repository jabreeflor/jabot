//! Schedules, live from the host (#25).
//!
//! The shape of `crew.ts`, for the same reason: `schedule/list` already returns
//! each job with its recent fires beside it, so this is a rename from wire
//! shape to prop shape and nothing more. No reducer, no second idea of what a
//! schedule is, and no client-side cron — what a schedule owes next is the
//! host's answer, because the host is the only thing that can still be wrong
//! about it after the app has been closed for a week.
//!
//! `schedules` stays `null` until the host answers. Unlike the crew, an empty
//! answer is entirely legitimate — a fresh install has no jobs — so `null` and
//! `[]` are genuinely different and the view draws different things for them.

import { useCallback, useEffect, useState } from "react";

import type {
  CatchUpPolicy,
  HostClient,
  ScheduleFireView,
  ScheduleView,
} from "../host";

/** What the editor hands back. The id is the caller's business, not the form's. */
export interface ScheduleDraft {
  botId: string;
  name: string;
  cron: string;
  prompt: string;
  catchUp: CatchUpPolicy;
}

export interface Schedules {
  /** `null` until the host answers; `[]` is a real, and common, answer. */
  schedules: ScheduleView[] | null;
  error: string | null;
  reload: () => void;
  /** Create when `scheduleId` is null, else patch. Resolves with the saved
      record or throws the host's error — the editor has to be able to say
      *why*, and "that cron has no hour 99 in it" is a fixable thing to say. */
  save: (scheduleId: string | null, draft: ScheduleDraft) => Promise<ScheduleView>;
  setEnabled: (scheduleId: string, enabled: boolean) => Promise<void>;
  remove: (scheduleId: string) => Promise<void>;
  /** Run now. Its own occurrence — it does not consume the next due time. */
  runNow: (scheduleId: string) => Promise<void>;
}

export function useSchedules(client: HostClient | null): Schedules {
  const [schedules, setSchedules] = useState<ScheduleView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped to re-run the load. Every mutation below changes the list, and a
  // fire changes it without anyone asking — see the poll in the view.
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `schedule/list` — a unit test's stub, an older host — should leave the
    // screen empty rather than take the render down.
    (async () => client.listSchedules())()
      .then((listed) => {
        if (cancelled) return;
        setSchedules(listed.schedules);
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

  const save = useCallback(
    async (scheduleId: string | null, draft: ScheduleDraft) => {
      if (!client) throw new Error("No host connection.");
      const saved = scheduleId
        ? await client.updateSchedule({ scheduleId, ...draft })
        : await client.createSchedule(draft);
      reload();
      return saved;
    },
    [client, reload],
  );

  const setEnabled = useCallback(
    async (scheduleId: string, enabled: boolean) => {
      if (!client) throw new Error("No host connection.");
      await client.updateSchedule({ scheduleId, enabled });
      reload();
    },
    [client, reload],
  );

  const remove = useCallback(
    async (scheduleId: string) => {
      if (!client) throw new Error("No host connection.");
      await client.removeSchedule({ scheduleId });
      reload();
    },
    [client, reload],
  );

  const runNow = useCallback(
    async (scheduleId: string) => {
      if (!client) throw new Error("No host connection.");
      await client.runSchedule({ scheduleId });
      reload();
    },
    [client, reload],
  );

  return { schedules, error, reload, save, setEnabled, remove, runNow };
}

/** A cron string in words, for the cases a person actually writes.
 *
 *  Deliberately partial: anything this does not recognise is shown verbatim
 *  rather than described wrongly. "Every 1 5 * * 2 minutes" would be worse
 *  than the cron itself, because the user can at least look that up. */
export function describeCron(cron: string): string {
  const raw = cron.trim();
  const shorthand: Record<string, string> = {
    "@hourly": "Every hour",
    "@daily": "Every day at midnight",
    "@midnight": "Every day at midnight",
    "@weekly": "Every Sunday at midnight",
    "@monthly": "On the 1st of each month",
    "@yearly": "Every 1 January",
    "@annually": "Every 1 January",
  };
  const named = shorthand[raw.toLowerCase()];
  if (named) return named;

  const fields = raw.split(/\s+/);
  if (fields.length !== 5) return raw;
  const [minute, hour, dom, month, dow] = fields;
  if (dom !== "*" || month !== "*") return raw;
  if (!/^\d+$/.test(minute)) return raw;

  const at = /^\d+$/.test(hour) ? clock(Number(hour), Number(minute)) : null;
  if (at && dow === "*") return `Every day at ${at}`;
  if (at && dow === "1-5") return `Weekdays at ${at}`;
  if (at && /^\d$/.test(dow)) return `Every ${DAYS[Number(dow) % 7]} at ${at}`;
  if (hour === "*") return `Every hour at :${pad(Number(minute))}`;
  return raw;
}

const DAYS = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}

function clock(hour: number, minute: number): string {
  return `${pad(hour)}:${pad(minute)}`;
}

/** One line about the last time this job ran, or why it did not.
 *
 *  The catch-up cases get their own words on purpose: "ran 4 hours late,
 *  5 earlier runs skipped" is the only place the policy in `host/schedule` is
 *  visible to the person it was applied to. */
export function describeFire(fire: ScheduleFireView | undefined): string {
  if (!fire) return "Has not run yet";
  const when = shortTime(fire.firedAt);
  const missed =
    fire.skippedCount > 0
      ? `, ${fire.skippedCount} missed run${fire.skippedCount === 1 ? "" : "s"} skipped`
      : "";
  switch (fire.state) {
    case "skipped":
      return `Skipped ${when}${missed}`;
    case "failed":
      return `Failed ${when}${missed}`;
    case "dispatched":
      return `Running since ${when}${missed}`;
    default:
      return `${fire.caughtUp ? "Caught up" : "Ran"} ${when}${missed}`;
  }
}

/** A timestamp as a person reads it. Invalid input is shown raw rather than as
    "Invalid Date" — a wrong-looking string is at least traceable. */
export function shortTime(stamp: string | undefined): string {
  if (!stamp) return "never";
  const at = new Date(stamp);
  if (Number.isNaN(at.getTime())) return stamp;
  return at.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
