//! The schedule editor: who runs it, when, and what happens to a missed run.
//!
//! Three fields the prototype never had to think about, all of them here
//! because the host cannot guess them:
//!
//! - **the bot.** A scheduled job runs *as* someone, with that bot's persona
//!   and tool allowlist, on its standing thread (decision #6). There is no
//!   host-owned persona to fall back on, so this is required.
//! - **the cron.** Free text, because five fields is a real language and the
//!   host refuses anything it cannot parse with the field named. The presets
//!   are for the four schedules everybody actually writes.
//! - **what happens to a missed run.** JaBot is not running while the Mac is
//!   shut (decision #4), so this question has to be asked rather than assumed.

import { useId, useState } from "react";

import { FieldLabel, Modal } from "./Modal";
import type { Bot } from "./types";
import type { CatchUpPolicy } from "../host";
import { PRESETS, type ScheduleDraft } from "../views/schedules";

export interface ScheduleEditorValue extends ScheduleDraft {
  scheduleId: string | null;
}

export function ScheduleEditorModal({
  schedule,
  bots,
  error = null,
  onSave,
  onRemove,
  onCancel,
}: {
  /** `null` for a new one. */
  schedule: ScheduleEditorValue | null;
  bots: readonly Bot[];
  /** Why the last save was refused — an unparseable cron, most likely. The
      card stays open holding the draft, because that is something to fix. */
  error?: string | null;
  onSave: (draft: ScheduleDraft) => void;
  onRemove?: (scheduleId: string) => void;
  onCancel: () => void;
}) {
  const nameId = useId();
  const botId = useId();
  const cronId = useId();
  const promptId = useId();
  const catchUpId = useId();

  const [name, setName] = useState(schedule?.name ?? "");
  const [bot, setBot] = useState(schedule?.botId ?? bots[0]?.id ?? "");
  const [cron, setCron] = useState(schedule?.cron ?? PRESETS[0].cron);
  const [prompt, setPrompt] = useState(schedule?.prompt ?? "");
  const [catchUp, setCatchUp] = useState<CatchUpPolicy>(
    schedule?.catchUp ?? "once",
  );

  return (
    <Modal
      title={schedule ? "Edit schedule" : "New schedule"}
      onClose={onCancel}
    >
      <FieldLabel htmlFor={nameId}>NAME</FieldLabel>
      <input
        id={nameId}
        type="text"
        value={name}
        placeholder="e.g. Morning triage"
        onChange={(event) => setName(event.target.value)}
      />

      <FieldLabel htmlFor={botId}>RUNS AS</FieldLabel>
      <select
        id={botId}
        value={bot}
        onChange={(event) => setBot(event.target.value)}
      >
        {bots.map((candidate) => (
          <option key={candidate.id} value={candidate.id}>
            {candidate.name}
          </option>
        ))}
      </select>

      <FieldLabel htmlFor={cronId}>WHEN</FieldLabel>
      <div
        className="sched-presets"
        role="group"
        aria-label="Schedule presets"
      >
        {PRESETS.map((preset) => (
          <button
            key={preset.cron}
            type="button"
            className="minichip sched-preset"
            aria-pressed={cron === preset.cron}
            onClick={() => setCron(preset.cron)}
          >
            {preset.label}
          </button>
        ))}
      </div>
      <input
        id={cronId}
        type="text"
        value={cron}
        placeholder="0 9 * * 1-5"
        // The host refuses a schedule over its cron and nothing else, so an
        // error is always this field's — and an error 300px below the box it
        // is about is an error the user has to go hunting for.
        aria-invalid={error ? true : undefined}
        aria-describedby={
          error ? `${cronId}-hint ${cronId}-error` : `${cronId}-hint`
        }
        onChange={(event) => setCron(event.target.value)}
      />
      <p id={`${cronId}-hint`} className="sched-hint">
        Minute, hour, day, month, weekday — in this Mac’s local time.
      </p>

      <FieldLabel htmlFor={promptId}>WHAT SHOULD IT DO?</FieldLabel>
      <input
        id={promptId}
        type="text"
        value={prompt}
        placeholder="e.g. Summarise overnight mail, flag anything urgent"
        onChange={(event) => setPrompt(event.target.value)}
      />

      <FieldLabel htmlFor={catchUpId}>IF JABOT WAS CLOSED</FieldLabel>
      <select
        id={catchUpId}
        value={catchUp}
        aria-describedby={catchUp === "once" ? `${catchUpId}-hint` : undefined}
        onChange={(event) => setCatchUp(event.target.value as CatchUpPolicy)}
      >
        <option value="once">Run the most recent missed one, once</option>
        <option value="skip">Skip it — only run on time</option>
      </select>
      {/* Only under "once". Skipping has no catch-up run to bound, so the
          sentence was describing behaviour the user had just turned off. */}
      {catchUp === "once" && (
        <p id={`${catchUpId}-hint`} className="sched-hint">
          At most one catch-up run, however long this Mac was off.
        </p>
      )}

      {error && (
        <p id={`${cronId}-error`} className="modal-error" role="alert">
          {error}
        </p>
      )}

      <div className="macts">
        {schedule && onRemove && (
          <button
            type="button"
            className="btn danger"
            onClick={() => onRemove(schedule.scheduleId!)}
          >
            Remove
          </button>
        )}
        <button type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          onClick={() =>
            onSave({
              botId: bot,
              name: name.trim() || "Untitled schedule",
              cron: cron.trim(),
              prompt: prompt.trim(),
              catchUp,
            })
          }
        >
          Save
        </button>
      </div>
    </Modal>
  );
}
