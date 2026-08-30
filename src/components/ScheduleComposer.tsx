//! Creating a schedule, as a prompt rather than as a form (#25).
//!
//! A schedule is one sentence — *do this, then, as them* — and the form asked
//! for it as five separate answers, three of which the sentence had already
//! given. So the sentence is the field. You type "summarise overnight mail
//! every weekday at 9am" and the WHEN chip fills itself in from the words that
//! were already there; everything else has a default worth keeping.
//!
//! What it does not do is guess silently. Whatever `parseWhen` finds is shown
//! back in a control that overrides it, in the same words the list will use, so
//! a wrong reading costs one click. The chips are the whole form — the details
//! nobody restates (a name, what a missed run should do) fold away underneath.
//!
//! It is a *screen*, not a modal: the list slides out and this slides in, which
//! is the difference between "the app moved" and "a box appeared over it".

import { useId, useMemo, useState, type FormEvent } from "react";

import { ArrowLeftIcon, ArrowUpIcon, ClockIcon } from "./Icon";
import { FieldLabel } from "./Modal";
import type { Bot } from "./types";
import type { CatchUpPolicy } from "../host";
import {
  PRESETS,
  describeCron,
  parseWhen,
  suggestName,
  type ScheduleDraft,
} from "../views/schedules";

const CUSTOM = "__custom__";

/** Three sentences that show the shape of the thing rather than describing it:
    each one says a job and a when, and clicking one fills the box *and* moves
    the WHEN chip, which is the whole trick demonstrated in one click. */
const STARTERS: readonly string[] = [
  "Summarise overnight mail every weekday at 9am",
  "Check the build every hour and tell me only if it broke",
  "Every Friday at 4pm, write up what shipped this week",
];

/** What a suggestion on the list hands over: the sentence, and the when. */
export interface ScheduleSeed {
  prompt: string;
  cron: string;
}

export function ScheduleComposer({
  bots,
  seed,
  error = null,
  busy = false,
  onCreate,
  onCancel,
}: {
  bots: readonly Bot[];
  /** Pre-filled from a suggestion, or `null` for an empty prompt. */
  seed?: ScheduleSeed | null;
  /** Why the host refused the last attempt. The draft stays on screen. */
  error?: string | null;
  busy?: boolean;
  onCreate: (draft: ScheduleDraft) => void;
  onCancel: () => void;
}) {
  const promptId = useId();
  const nameId = useId();
  const cronId = useId();
  const catchUpId = useId();

  const [prompt, setPrompt] = useState(seed?.prompt ?? "");
  const [bot, setBot] = useState(bots[0]?.id ?? "");
  const [cron, setCron] = useState(seed?.cron ?? PRESETS[0].cron);
  const [name, setName] = useState("");
  const [catchUp, setCatchUp] = useState<CatchUpPolicy>("once");
  // Whether the WHEN chip is currently the sentence's answer or the user's.
  // Once they touch it, the parser stops moving it under them — a control that
  // springs back while you are typing is worse than one that never helped.
  const [readFromPrompt, setReadFromPrompt] = useState(false);
  // Set by any deliberate pick, including "Custom…". Separate from `showCron`
  // because choosing a preset also has to stop the parser, and that closes no
  // cron box.
  const [pinned, setPinned] = useState(false);
  const [showCron, setShowCron] = useState(false);

  const options = useMemo(() => {
    const known = PRESETS.map((preset) => ({ ...preset }));
    if (!known.some((preset) => preset.cron === cron) && cron.trim()) {
      known.push({ label: describeCron(cron), cron });
    }
    return known;
  }, [cron]);

  function onPromptChange(next: string) {
    setPrompt(next);
    if (pinned) return;
    const found = parseWhen(next);
    // Only ever *adds* a reading. Clearing the chip because a half-typed
    // sentence stopped parsing would make the control flicker as you type,
    // and a sentence that agrees with the default still said it out loud.
    if (found) {
      setCron(found);
      setReadFromPrompt(true);
    }
  }

  function pickWhen(value: string) {
    setPinned(true);
    setReadFromPrompt(false);
    if (value === CUSTOM) {
      setShowCron(true);
      return;
    }
    setCron(value);
    setShowCron(false);
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    const said = prompt.trim();
    if (!said || busy) return;
    onCreate({
      botId: bot,
      name: name.trim() || suggestName(said) || "Untitled schedule",
      cron: cron.trim(),
      prompt: said,
      catchUp,
    });
  }

  const runsAs = bots.find((candidate) => candidate.id === bot);

  return (
    <div className="view sched-compose">
      <div className="compose-bar">
        <button type="button" className="compose-back" onClick={onCancel}>
          <ArrowLeftIcon />
          Schedules
        </button>
      </div>
      <div className="compose-scroll">
        <div className="compose-stage">
          <h1 className="compose-title">What should run on a timer?</h1>
          <p className="compose-sub">
            Say what should happen, and say when. Anything the sentence leaves
            out is a chip under it.
          </p>

          <form className="prompt-box" onSubmit={submit}>
            <textarea
              id={promptId}
              aria-label="What should it do?"
              className="prompt-field"
              value={prompt}
              rows={3}
              autoFocus
              placeholder="Summarise overnight mail every weekday at 9am, and flag anything urgent"
              onChange={(event) => onPromptChange(event.target.value)}
              onKeyDown={(event) => {
                // Return sends, as it does in the message box next door;
                // Shift+Return is the one that means "another line". Escape
                // leaves, which is what Escape does to every other surface in
                // the app that can be left.
                if (event.key === "Enter" && !event.shiftKey) submit(event);
                if (event.key === "Escape") onCancel();
              }}
            />

            <div className="prompt-bar">
              <span className="prompt-chip">
                <span className="chip-key">as</span>
                <select
                  aria-label="Runs as"
                  value={bot}
                  onChange={(event) => setBot(event.target.value)}
                >
                  {bots.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>
                      {candidate.name}
                    </option>
                  ))}
                </select>
              </span>

              <span
                className={
                  readFromPrompt ? "prompt-chip read-off" : "prompt-chip"
                }
              >
                <ClockIcon />
                <select
                  aria-label="When"
                  value={showCron ? CUSTOM : cron}
                  onChange={(event) => pickWhen(event.target.value)}
                >
                  {options.map((option) => (
                    <option key={option.cron} value={option.cron}>
                      {option.label}
                    </option>
                  ))}
                  <option value={CUSTOM}>Custom…</option>
                </select>
              </span>

              <button
                type="submit"
                className="send-btn"
                aria-label="Create schedule"
                disabled={!prompt.trim() || busy}
              >
                <ArrowUpIcon />
              </button>
            </div>
          </form>

          {!prompt.trim() && (
            <div className="compose-starters">
              {STARTERS.map((starter) => (
                <button
                  key={starter}
                  type="button"
                  className="compose-starter"
                  onClick={() => onPromptChange(starter)}
                >
                  {starter}
                </button>
              ))}
            </div>
          )}

          <p className="compose-read" aria-live="polite">
            {readFromPrompt ? (
              <>
                <span className="read-mark">Read from your words</span>
                {describeCron(cron)}, as {runsAs?.name ?? "your bot"}.
              </>
            ) : (
              <>
                {describeCron(cron)}, as {runsAs?.name ?? "your bot"} — on that
                bot’s own thread, landing in your Inbox.
              </>
            )}
          </p>

          {showCron && (
            <div className="compose-cron">
              <FieldLabel htmlFor={cronId}>CRON</FieldLabel>
              <input
                id={cronId}
                className="mfield"
                type="text"
                value={cron}
                autoFocus
                placeholder="0 9 * * 1-5"
                aria-invalid={error ? true : undefined}
                aria-describedby={`${cronId}-hint`}
                onChange={(event) => setCron(event.target.value)}
              />
              <p id={`${cronId}-hint`} className="sched-hint">
                Minute, hour, day, month, weekday — in this Mac’s local time.
              </p>
            </div>
          )}

          <details className="compose-more">
            <summary>Name it, and say what a missed run should do</summary>
            <div className="compose-more-body">
              <FieldLabel htmlFor={nameId}>NAME</FieldLabel>
              <input
                id={nameId}
                className="mfield"
                type="text"
                value={name}
                placeholder={suggestName(prompt) || "Morning triage"}
                onChange={(event) => setName(event.target.value)}
              />
              <p className="sched-hint">
                Left empty, it takes the opening words of the prompt.
              </p>

              <FieldLabel htmlFor={catchUpId}>IF JABOT WAS CLOSED</FieldLabel>
              <select
                id={catchUpId}
                className="mfield"
                value={catchUp}
                onChange={(event) =>
                  setCatchUp(event.target.value as CatchUpPolicy)
                }
              >
                <option value="once">Run the most recent missed one, once</option>
                <option value="skip">Skip it — only run on time</option>
              </select>
            </div>
          </details>

          {error && (
            <p className="modal-error" role="alert">
              {error}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
