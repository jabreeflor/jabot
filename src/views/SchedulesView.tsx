//! Recurring jobs (#25).
//!
//! One row per schedule, and every row says three things a cron UI usually
//! leaves out: which bot it runs as, when it is next owed, and what happened
//! the last time — including the case the host works hardest at, which is a
//! run that was missed while the Mac was shut.
//!
//! The rows are a *list*, not a wall of cards. A schedule is a one-line fact —
//! this, then, as them — and the second a screen has six of them the question
//! stops being "what does this one say" and becomes "which of these is about to
//! happen". So: a status dot you can scan down, the next run in the words you
//! would use out loud ("in 13 minutes"), and everything else — the prompt, the
//! last run, the buttons — folded into the row until you open it.
//!
//! Above them, the two controls a list earns as soon as it is a list: a search
//! box and All/Active/Paused. Below them, suggestions, because an empty
//! schedules screen is the one screen in the app that cannot demonstrate
//! itself. Under all of it, docked, the prompt that writes a new one — the
//! list is what you write a schedule *against*, so it stays on screen while
//! you do.
//!
//! The list polls. A fire is the one thing on this screen that happens without
//! the user doing anything, and a row still saying "next: 09:00" ten minutes
//! after nine would be the screen lying about the only thing it is for. The
//! poll is deliberately slow — the host is authoritative and a schedule is
//! measured in hours, not frames.

import { useEffect, useMemo, useRef, useState } from "react";

import type { ScheduleView } from "../host";
import type { Bot } from "../components/types";
import {
  CaretRightIcon,
  ClockIcon,
  InboxIcon,
  PullRequestIcon,
  SearchIcon,
  SparkIcon,
} from "../components/Icon";
import {
  SchedulePrompt,
  type SchedulePromptHandle,
  type ScheduleSeed,
} from "../components/SchedulePrompt";
import type { ScheduleDraft } from "./schedules";
import { describeCron, describeFire, relativeWhen, shortTime } from "./schedules";

/** Slow on purpose: `schedule/list` is a couple of SQLite reads, and nothing
    on this screen is worth a tighter loop than the job it describes. */
const POLL_MS = 10_000;

type Filter = "all" | "active" | "paused";

/** What a fresh install has instead of a list. Each one is a whole schedule —
    a sentence and a cron — so accepting it is one click and correcting it is
    the same screen a blank prompt would have opened. */
const SUGGESTIONS: ReadonlyArray<{
  title: string;
  cron: string;
  prompt: string;
  icon: "inbox" | "prs" | "spark";
}> = [
  {
    title: "Morning brief",
    cron: "0 8 * * 1-5",
    prompt:
      "Summarise overnight mail and today’s calendar every weekday at 8am, and flag anything urgent",
    icon: "inbox",
  },
  {
    title: "Pull request sweep",
    cron: "0 17 * * 1-5",
    prompt:
      "Every weekday at 5pm, check every open pull request for review comments and failing CI, and say which ones need me",
    icon: "prs",
  },
  {
    title: "Weekly review",
    cron: "0 16 * * 5",
    prompt:
      "Every Friday at 4pm, turn this week’s threads into a short status update",
    icon: "spark",
  },
];

export function SchedulesView({
  schedules,
  bots,
  error,
  onReload,
  onCreate,
  onEdit,
  onToggle,
  onRunNow,
  onOpenThread,
}: {
  /** `null` means the host has not answered; `[]` means a fresh install. They
      are different pictures, and only one of them is an empty state. */
  schedules: readonly ScheduleView[] | null;
  bots: readonly Bot[];
  error: string | null;
  onReload: () => void;
  /** Resolves when the host has taken it, rejects with what it objected to —
      the prompt stays open holding the draft, because a refused cron is a
      thing to fix and not a thing to retype. */
  onCreate: (draft: ScheduleDraft) => Promise<unknown>;
  onEdit: (scheduleId: string) => void;
  onToggle: (scheduleId: string, enabled: boolean) => void;
  onRunNow: (scheduleId: string) => void;
  onOpenThread: (threadId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [openRow, setOpenRow] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // The prompt owns its draft. This is how the list writes into it — a
  // suggestion is a sentence handed over, "New schedule" is a cursor put in
  // the box — without the whole draft living up here to be passed back down.
  const prompt = useRef<SchedulePromptHandle>(null);

  useEffect(() => {
    const timer = window.setInterval(onReload, POLL_MS);
    return () => window.clearInterval(timer);
  }, [onReload]);

  const rows = schedules ?? [];
  const counts = {
    all: rows.length,
    active: rows.filter((row) => row.enabled).length,
    paused: rows.filter((row) => !row.enabled).length,
  };

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return rows.filter((row) => {
      if (filter === "active" && !row.enabled) return false;
      if (filter === "paused" && row.enabled) return false;
      if (!needle) return true;
      return [row.name, row.prompt, row.botName, describeCron(row.cron)]
        .join(" ")
        .toLowerCase()
        .includes(needle);
    });
  }, [rows, query, filter]);

  function write(seed: ScheduleSeed) {
    setSaveError(null);
    prompt.current?.write(seed);
  }

  function create(draft: ScheduleDraft) {
    setSaving(true);
    setSaveError(null);
    onCreate(draft)
      .then(() => {
        // The new row has to be somewhere the user can see it, or a create
        // under a filter looks like a create that did nothing.
        prompt.current?.clear();
        setFilter("all");
        setQuery("");
      })
      .catch((err: unknown) =>
        setSaveError(err instanceof Error ? err.message : String(err)),
      )
      .finally(() => setSaving(false));
  }

  const empty = schedules?.length === 0;

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page sched-page">
          <div className="page-top">
            <h1>Schedules</h1>
            <p>
              Recurring jobs. Each one runs as a bot, on that bot’s thread, and
              lands in your Inbox when it is done.
            </p>
          </div>

          {error && (
            <p className="modal-error" role="alert">
              {error}
            </p>
          )}

          {rows.length > 0 && (
            <div className="sched-controls">
              <div className="sched-search">
                <SearchIcon />
                <input
                  type="search"
                  value={query}
                  aria-label="Search schedules"
                  placeholder="Search schedules"
                  onChange={(event) => setQuery(event.target.value)}
                />
              </div>
              <div className="sched-filters" role="group" aria-label="Filter">
                {(["all", "active", "paused"] as const).map((option) => (
                  <button
                    key={option}
                    type="button"
                    className="sched-filter"
                    aria-pressed={filter === option}
                    onClick={() => setFilter(option)}
                  >
                    {option === "all"
                      ? "All"
                      : option === "active"
                        ? "Active"
                        : "Paused"}
                    <span className="count">{counts[option]}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          {schedules === null && !error && (
            <div className="page-empty">Asking the host…</div>
          )}

          {empty && (
            <div className="sched-blank">
              <span className="sched-blank-mark" aria-hidden="true">
                <ClockIcon />
              </span>
              <h2>No schedules yet</h2>
              {/* Pointing at the box rather than repeating it: the prompt is
                  docked at the bottom of this very screen, and a button that
                  only puts a cursor in a field the user can already see is one
                  affordance too many. */}
              <p>
                Say what a bot should do and when, in the box below —
                “summarise overnight mail every weekday at 9am”.
              </p>
            </div>
          )}

          {rows.length > 0 && shown.length === 0 && (
            <div className="page-empty">
              Nothing matches. {counts.all} schedule
              {counts.all === 1 ? "" : "s"} in total.
            </div>
          )}

          {shown.length > 0 && (
            <ul className="sched-list">
              {shown.map((schedule) => (
                <ScheduleRow
                  key={schedule.scheduleId}
                  schedule={schedule}
                  bots={bots}
                  open={openRow === schedule.scheduleId}
                  onOpen={() =>
                    setOpenRow((current) =>
                      current === schedule.scheduleId
                        ? null
                        : schedule.scheduleId,
                    )
                  }
                  onEdit={onEdit}
                  onToggle={onToggle}
                  onRunNow={onRunNow}
                  onOpenThread={onOpenThread}
                />
              ))}
            </ul>
          )}

          <div className="sched-suggest">
            <div className="page-section">SUGGESTIONS</div>
            <ul className="sched-suggest-list">
              {SUGGESTIONS.map((suggestion) => (
                <li key={suggestion.title}>
                  <button
                    type="button"
                    className="sched-suggest-row"
                    onClick={() =>
                      write({
                        prompt: suggestion.prompt,
                        cron: suggestion.cron,
                      })
                    }
                  >
                    <span
                      className={`mark is-${suggestion.icon}`}
                      aria-hidden="true"
                    >
                      {suggestion.icon === "inbox" ? (
                        <InboxIcon />
                      ) : suggestion.icon === "prs" ? (
                        <PullRequestIcon />
                      ) : (
                        <SparkIcon />
                      )}
                    </span>
                    <span className="body">
                      <span className="head">
                        <span className="ttl">{suggestion.title}</span>
                        <span className="when">
                          {describeCron(suggestion.cron)}
                        </span>
                      </span>
                      <span className="say">{suggestion.prompt}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>

      <SchedulePrompt
        handleRef={prompt}
        bots={bots}
        error={saveError}
        busy={saving}
        onCreate={create}
      />
    </div>
  );
}

function ScheduleRow({
  schedule,
  bots,
  open,
  onOpen,
  onEdit,
  onToggle,
  onRunNow,
  onOpenThread,
}: {
  schedule: ScheduleView;
  bots: readonly Bot[];
  open: boolean;
  onOpen: () => void;
  onEdit: (scheduleId: string) => void;
  onToggle: (scheduleId: string, enabled: boolean) => void;
  onRunNow: (scheduleId: string) => void;
  onOpenThread: (threadId: string) => void;
}) {
  const bot = bots.find((candidate) => candidate.id === schedule.botId);
  const fire = schedule.lastFire;
  // The catch-up cases get their own line rather than a badge: "5 missed runs
  // skipped" is the whole of what the policy did, and a dot would not say it.
  const caughtUp = fire?.caughtUp === true;
  const state = !schedule.enabled
    ? "paused"
    : fire?.state === "dispatched"
      ? "running"
      : fire?.state === "failed"
        ? "failed"
        : caughtUp
          ? "late"
          : "ready";
  const next = schedule.enabled ? relativeWhen(schedule.nextRunAt) : null;

  return (
    <li className={open ? "sched-row is-open" : "sched-row"}>
      <div className="sched-row-top">
        <button
          type="button"
          className="sched-row-main"
          aria-expanded={open}
          onClick={onOpen}
        >
          <span
            className={`sched-dot is-${state}`}
            aria-hidden="true"
            title={state}
          />
          <span className="sched-row-text">
            <span className="sched-row-name">{schedule.name}</span>
            <span className="sched-row-meta">
              <span>{describeCron(schedule.cron)}</span>
              <span className="sep" aria-hidden="true">
                ·
              </span>
              <span
                className={next === "due now" ? "sched-next is-due" : "sched-next"}
                title={schedule.enabled ? shortTime(schedule.nextRunAt) : undefined}
              >
                {schedule.enabled ? `Next run ${next ?? "unscheduled"}` : "Paused"}
              </span>
              <span className="sep" aria-hidden="true">
                ·
              </span>
              <span>as {bot?.name ?? schedule.botName}</span>
            </span>
          </span>
          <CaretRightIcon className="sched-caret" />
        </button>

        <label className="sched-switch">
          <input
            type="checkbox"
            checked={schedule.enabled}
            aria-label={`${schedule.name} enabled`}
            onChange={(event) =>
              onToggle(schedule.scheduleId, event.target.checked)
            }
          />
          <span className="track" aria-hidden="true">
            <span className="knob" />
          </span>
        </label>
      </div>

      {open && (
        <div className="sched-row-body">
          <p className="sched-prompt">{schedule.prompt}</p>

          <div className="tools">
            <span className={caughtUp ? "minichip sched-caught" : "minichip"}>
              {describeFire(fire)}
            </span>
            {schedule.catchUp === "skip" && (
              <span className="minichip">No catch-up</span>
            )}
            <span className="minichip">{schedule.cron}</span>
          </div>

          {fire?.detail && <p className="sched-detail">{fire.detail}</p>}

          <div className="acts">
            <button
              type="button"
              className="btn"
              onClick={() => onRunNow(schedule.scheduleId)}
            >
              Run now
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => onEdit(schedule.scheduleId)}
            >
              Edit
            </button>
            {schedule.threadId && (
              <button
                type="button"
                className="btn"
                onClick={() => onOpenThread(schedule.threadId!)}
              >
                Open thread
              </button>
            )}
          </div>
        </div>
      )}
    </li>
  );
}
