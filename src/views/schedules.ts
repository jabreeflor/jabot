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

/** The four schedules a person writes without looking anything up. Anything
    else is typed as cron in the same place — this is a shortcut, not the
    vocabulary. Lives here rather than in either editor because both of them
    offer the same four, and two lists that drift apart is how a preset ends up
    meaning one thing on the way in and another on the way back out. */
export const PRESETS: ReadonlyArray<{ label: string; cron: string }> = [
  { label: "Every weekday, 9am", cron: "0 9 * * 1-5" },
  { label: "Every day, 8am", cron: "0 8 * * *" },
  { label: "Every hour", cron: "0 * * * *" },
  { label: "Mondays, 9am", cron: "0 9 * * 1" },
];

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

/** How far off the next run is, in the words the list uses: "in 13 minutes".
 *
 *  Relative rather than absolute because that is the question the row is
 *  answering — "is this about to happen?" — and a person reading "Mar 5, 09:00"
 *  has to work out the answer themselves. The absolute time is still one hover
 *  away on the row, so nothing is lost.
 *
 *  `null` for anything that cannot be read as a time, so the caller can fall
 *  back rather than print "in NaN minutes". */
export function relativeWhen(
  stamp: string | undefined,
  now: number = Date.now(),
): string | null {
  if (!stamp) return null;
  const at = new Date(stamp).getTime();
  if (Number.isNaN(at)) return null;

  const seconds = Math.round((at - now) / 1000);
  // A schedule the host still owes is "due now", not "1 minute ago": the host
  // fires it, and until it does, past-due is a statement about the queue.
  if (seconds <= 30) return "due now";
  if (seconds < 90) return "in a minute";

  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `in ${minutes} minutes`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `in ${hours} hour${hours === 1 ? "" : "s"}`;
  const days = Math.round(hours / 24);
  return `in ${days} day${days === 1 ? "" : "s"}`;
}

const DAY_WORDS: ReadonlyArray<readonly [RegExp, string]> = [
  [/\b(weekday|weekdays|working day|working days|week days)\b/, "1-5"],
  [/\b(weekend|weekends)\b/, "0,6"],
  [/\b(sunday|sundays)\b/, "0"],
  [/\b(monday|mondays)\b/, "1"],
  [/\b(tuesday|tuesdays)\b/, "2"],
  [/\b(wednesday|wednesdays)\b/, "3"],
  [/\b(thursday|thursdays)\b/, "4"],
  [/\b(friday|fridays)\b/, "5"],
  [/\b(saturday|saturdays)\b/, "6"],
];

/** The vague times, resolved out loud. Only used when no clock was given, and
    the answer is shown back as "Every day at 08:00" — a guess the user can see
    is a guess they can correct, which is the whole reason to risk one. */
const VAGUE_HOURS: ReadonlyArray<readonly [RegExp, number]> = [
  [/\bmorning\b/, 8],
  [/\bmidday\b|\bnoon\b/, 12],
  [/\bafternoon\b/, 14],
  [/\bevening\b/, 18],
  [/\bnight\b|\btonight\b/, 21],
  [/\bmidnight\b/, 0],
];

/** The cron hiding in a sentence, or `null`.
 *
 *  This is what makes the composer a prompt rather than a form: you say "every
 *  weekday at 9am, summarise overnight mail" and the WHEN chip fills itself in.
 *  It is deliberately narrow and deliberately visible — whatever it finds is
 *  shown back as a sentence beside a control that overrides it, so a wrong
 *  guess costs one click and a right one costs nothing.
 *
 *  Nothing is inferred from a bare time: "the 9am standup" is a subject, not a
 *  schedule. A recurrence has to be said — "every", "each", "daily", a named
 *  day — before a clock in the same sentence means anything. */
export function parseWhen(text: string): string | null {
  const said = text.toLowerCase();
  const clock = readClock(said);
  const day = DAY_WORDS.find(([pattern]) => pattern.test(said))?.[1] ?? null;
  const recurs = /\bevery\b|\beach\b|\bdaily\b|\bhourly\b|\bweekly\b/.test(said);

  if (/\b(every hour|hourly|each hour)\b/.test(said)) {
    return `${clock?.minute ?? 0} * * * *`;
  }

  const hour = clock?.hour ?? vagueHour(said);
  if (hour === null) return null;
  const minute = clock?.minute ?? 0;

  if (day) return `${minute} ${hour} * * ${day}`;
  if (recurs || /\bday\b/.test(said)) return `${minute} ${hour} * * *`;
  return null;
}

function vagueHour(said: string): number | null {
  if (readClock(said)) return null;
  return VAGUE_HOURS.find(([pattern]) => pattern.test(said))?.[1] ?? null;
}

/** A clock time out of a sentence: `9am`, `9:30pm`, `at 17:00`, `noon`. */
function readClock(said: string): { hour: number; minute: number } | null {
  if (/\bnoon\b|\bmidday\b/.test(said)) return { hour: 12, minute: 0 };
  if (/\bmidnight\b/.test(said)) return { hour: 0, minute: 0 };

  const meridiem = said.match(/\b(\d{1,2})(?::(\d{2}))?\s*(am|pm)\b/);
  if (meridiem) {
    const raw = Number(meridiem[1]) % 12;
    const hour = meridiem[3] === "pm" ? raw + 12 : raw;
    return { hour, minute: Number(meridiem[2] ?? 0) };
  }

  // 24-hour only after "at", so a "15 minutes" or a "3 PRs" is not a time.
  const spelled = said.match(/\bat\s+(\d{1,2}):(\d{2})\b/);
  if (spelled) {
    const hour = Number(spelled[1]);
    const minute = Number(spelled[2]);
    if (hour < 24 && minute < 60) return { hour, minute };
  }
  return null;
}

/** The words that are the *when*, not the *what*. Stripped out of a prompt to
    get at the name hiding in it — "summarise overnight mail every weekday at
    9am" is a schedule called "Summarise overnight mail". Each pattern needs a
    time word next to it, so the "every" in "check every open pull request"
    survives and the one in "every weekday" does not. */
const WHEN_WORDS = [
  /\b(every|each)\s+(day|weekday|weekdays|week|hour|morning|afternoon|evening|night|monday|tuesday|wednesday|thursday|friday|saturday|sunday|mondays|tuesdays|wednesdays|thursdays|fridays|saturdays|sundays|weekend|weekends)\b/gi,
  /\b(daily|hourly|weekly|nightly)\b/gi,
  /\bat\s+\d{1,2}(:\d{2})?\s*(am|pm)?\b/gi,
  /\b\d{1,2}(:\d{2})?\s*(am|pm)\b/gi,
  /\b(on\s+)?(mondays?|tuesdays?|wednesdays?|thursdays?|fridays?|saturdays?|sundays?|weekdays?|weekends?)\b/gi,
  /\b(in the\s+)?(morning|afternoon|evening|overnight tonight)\b/gi,
  /\b(noon|midday|midnight)\b/gi,
];

/** A word that cannot be the last one in a title. */
const DANGLING = new Set([
  "and", "or", "the", "a", "an", "to", "of", "for", "with", "on", "in", "at",
  "by", "from", "into", "that", "this", "my", "your", "its", "then",
]);

/** A name for a schedule nobody named: what it does, with the when taken out.
 *
 *  A schedule created from a prompt still needs a title in the list, and
 *  "Untitled schedule" three times over is a list you cannot read. The prompt's
 *  own words are the closest thing to what the user would have typed — minus
 *  the half of the sentence the WHEN chip is already showing. */
export function suggestName(prompt: string): string {
  const segments = prompt
    .split(/[.,;\n]/)
    .map((segment) => withoutWhen(segment))
    .filter(Boolean);
  const said = firstClause(segments[0] ?? "");
  if (!said) return "";

  const words = said.split(/\s+/).slice(0, 6);
  while (words.length > 1 && DANGLING.has(words[words.length - 1].toLowerCase())) {
    words.pop();
  }
  const name = words.join(" ");
  const clipped = name.length > 42 ? `${name.slice(0, 42).trimEnd()}\u2026` : name;
  return clipped.charAt(0).toUpperCase() + clipped.slice(1);
}

/** The first thing a schedule does, when it does two. "Summarise overnight
    mail and flag anything urgent" is called "Summarise overnight mail" — the
    second clause is what the prompt is for, not what the row is called. Only
    when something real precedes the conjunction, so "and flag anything" as a
    whole prompt keeps its words. */
function firstClause(said: string): string {
  const cut = said.split(/\s+(?:and|then|but|also|plus)\s+/i)[0];
  return cut.split(/\s+/).length >= 2 ? cut : said;
}

function withoutWhen(segment: string): string {
  const stripped = WHEN_WORDS.reduce(
    (said, pattern) => said.replace(pattern, " "),
    segment,
  );
  // What the stripping leaves behind: a leading "and", doubled spaces, a lone
  // preposition where a time used to be.
  return stripped
    .replace(/\s+/g, " ")
    .replace(/^\s*(and|then|also|,)\s+/i, "")
    .trim();
}
