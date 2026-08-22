//! Recurring jobs (#25).
//!
//! One row per schedule, and every row says three things a cron UI usually
//! leaves out: which bot it runs as, when it is next owed, and what happened
//! the last time — including the case the host works hardest at, which is a
//! run that was missed while the Mac was shut.
//!
//! The list polls. A fire is the one thing on this screen that happens without
//! the user doing anything, and a row still saying "next: 09:00" ten minutes
//! after nine would be the screen lying about the only thing it is for. The
//! poll is deliberately slow — the host is authoritative and a schedule is
//! measured in hours, not frames.

import { useEffect } from "react";

import type { ScheduleView } from "../host";
import type { Bot } from "../components/types";
import { PlusIcon } from "../components/Icon";
import { describeCron, describeFire, shortTime } from "./schedules";

/** Slow on purpose: `schedule/list` is a couple of SQLite reads, and nothing
    on this screen is worth a tighter loop than the job it describes. */
const POLL_MS = 10_000;

export function SchedulesView({
  schedules,
  bots,
  error,
  onReload,
  onAdd,
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
  onAdd: () => void;
  onEdit: (scheduleId: string) => void;
  onToggle: (scheduleId: string, enabled: boolean) => void;
  onRunNow: (scheduleId: string) => void;
  onOpenThread: (threadId: string) => void;
}) {
  useEffect(() => {
    const timer = window.setInterval(onReload, POLL_MS);
    return () => window.clearInterval(timer);
  }, [onReload]);

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
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

          {schedules === null && !error && (
            <div className="page-empty">Asking the host…</div>
          )}

          {schedules?.length === 0 && (
            <div className="page-empty">
              No schedules yet. Add one to have a bot do something on a timer.
            </div>
          )}

          <div className="sched-list">
            {(schedules ?? []).map((schedule) => (
              <ScheduleRow
                key={schedule.scheduleId}
                schedule={schedule}
                bots={bots}
                onEdit={onEdit}
                onToggle={onToggle}
                onRunNow={onRunNow}
                onOpenThread={onOpenThread}
              />
            ))}

            <button type="button" className="add-card" onClick={onAdd}>
              <span className="big" aria-hidden="true">
                <PlusIcon />
              </span>
              Add a schedule
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ScheduleRow({
  schedule,
  bots,
  onEdit,
  onToggle,
  onRunNow,
  onOpenThread,
}: {
  schedule: ScheduleView;
  bots: readonly Bot[];
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
  return (
    <div className="crew-card sched-card">
      <div className="r1">
        <div>
          <div className="nm">{schedule.name}</div>
          <div className="role">
            {describeCron(schedule.cron)} · as {bot?.name ?? schedule.botName}
          </div>
        </div>
        <label className="sched-switch">
          <input
            type="checkbox"
            checked={schedule.enabled}
            onChange={(event) =>
              onToggle(schedule.scheduleId, event.target.checked)
            }
          />
          {schedule.enabled ? "On" : "Off"}
        </label>
      </div>

      <div className="role sched-prompt">{schedule.prompt}</div>

      <div className="tools">
        <span className="minichip">
          {schedule.enabled
            ? `Next ${shortTime(schedule.nextRunAt)}`
            : "Paused"}
        </span>
        <span className={caughtUp ? "minichip sched-caught" : "minichip"}>
          {describeFire(fire)}
        </span>
        {schedule.catchUp === "skip" && (
          <span className="minichip">No catch-up</span>
        )}
      </div>

      {fire?.detail && <div className="role sched-detail">{fire.detail}</div>}

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
  );
}
