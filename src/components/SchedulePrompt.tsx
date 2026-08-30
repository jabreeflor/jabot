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
//! a wrong reading costs one click.
//!
//! It sits *under the list*, docked, the way the message box sits under a
//! transcript — not on a screen of its own. Writing a schedule is a thing you
//! do while looking at the ones you already have: to see that 8am is taken, to
//! copy how you phrased the last one, to notice you are about to write the
//! same job twice. A screen that replaced the list took all of that away for a
//! form that fits in a bar.

import {
  useEffect,
  useId,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type Ref,
} from "react";

import { ArrowUpIcon, ClockIcon } from "./Icon";
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

/** What a suggestion on the list hands over: the sentence, and the when. */
export interface ScheduleSeed {
  prompt: string;
  cron: string;
}

/** The list writes into the box rather than owning its state: a suggestion is
    a sentence handed to the prompt, and "New schedule" is a cursor put in it. */
export interface SchedulePromptHandle {
  write: (seed: ScheduleSeed) => void;
  /** The host has taken it and the row exists: the box is stale. */
  clear: () => void;
}

export function SchedulePrompt({
  bots,
  error = null,
  busy = false,
  onCreate,
  handleRef,
}: {
  bots: readonly Bot[];
  /** Why the host refused the last attempt. The draft stays in the box. */
  error?: string | null;
  busy?: boolean;
  onCreate: (draft: ScheduleDraft) => void;
  handleRef?: Ref<SchedulePromptHandle>;
}) {
  const promptId = useId();
  const cronId = useId();

  const [prompt, setPrompt] = useState("");
  const [bot, setBot] = useState(bots[0]?.id ?? "");
  const [cron, setCron] = useState(PRESETS[0].cron);
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

  const box = useRef<HTMLTextAreaElement>(null);

  useImperativeHandle(handleRef, () => ({
    write(seed: ScheduleSeed) {
      setPrompt(seed.prompt);
      setCron(seed.cron);
      setPinned(true);
      setReadFromPrompt(false);
      setShowCron(false);
      box.current?.focus();
    },
    clear,
  }));

  // A bot list that arrives after the first render — the crew is host-owned —
  // should fill the chip rather than leave it empty.
  useEffect(() => {
    if (!bot && bots[0]) setBot(bots[0].id);
  }, [bots, bot]);

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
      name: suggestName(said) || "Untitled schedule",
      cron: cron.trim(),
      prompt: said,
      catchUp,
    });
  }

  /** Cleared once the host has taken it — the caller says so by remounting
      nothing; this is the only place that knows the box is now stale. */
  function clear() {
    setPrompt("");
    setPinned(false);
    setReadFromPrompt(false);
    setShowCron(false);
  }

  const runsAs = bots.find((candidate) => candidate.id === bot);
  const name = suggestName(prompt);
  const written = prompt.trim().length > 0;

  return (
    <div className="sched-dock">
      <form
        className={written ? "dock-box is-live" : "dock-box"}
        onSubmit={(event) => {
          submit(event);
        }}
      >
        {/* What the bar is about to create, in the words the list will use. It
            reads back rather than previews: same sentence, same vocabulary —
            and it is the only place the derived name is visible before the row
            exists. */}
        {written && (
          <p className="dock-read" aria-live="polite">
            {readFromPrompt && (
              <span className="read-mark">Read from your words</span>
            )}
            <span className="dock-name">{name || "Untitled schedule"}</span>
            <span className="dock-when">
              {describeCron(cron)}, as {runsAs?.name ?? "your bot"}
            </span>
          </p>
        )}

        {/* The host refuses a schedule over its cron and nothing else, so the
            objection sits against the chips rather than somewhere else. */}
        {error && (
          <p className="modal-error dock-error" role="alert">
            {error}
          </p>
        )}

        <textarea
          id={promptId}
          ref={box}
          className="dock-field"
          value={prompt}
          rows={2}
          aria-label="What should it do?"
          placeholder="Describe a job, and when it should run…"
          onChange={(event) => onPromptChange(event.target.value)}
          onKeyDown={(event) => {
            // Return sends, as it does in the message box next door;
            // Shift+Return is the one that means "another line".
            if (event.key === "Enter" && !event.shiftKey) submit(event);
            // Escape empties the box rather than leaving the screen: there is
            // no screen to leave any more, and a draft you cannot abandon is
            // a draft you have to select-all to be rid of.
            if (event.key === "Escape" && written) {
              event.stopPropagation();
              clear();
            }
          }}
        />

        {/* A refusal is always about the cron, and a preset cannot be refused
            — so on one, whatever is in there is worth showing raw. Its own
            row: five fields is a field, and a field in among the chips is what
            pushes the send button onto a line of its own. */}
        {(showCron || error) && (
          <label className="dock-cron">
            <span>CRON</span>
            <input
              id={cronId}
              type="text"
              value={cron}
              autoFocus={showCron}
              aria-label="CRON"
              aria-invalid={error ? true : undefined}
              placeholder="0 9 * * 1-5"
              onChange={(event) => setCron(event.target.value)}
            />
          </label>
        )}

        <div className="dock-bar">
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
            className={[
              "prompt-chip",
              error ? "is-bad" : readFromPrompt ? "read-off" : "",
            ]
              .join(" ")
              .trim()}
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

          {/* The question decision #4 forces — this Mac is not always on — as
              a chip rather than a fold, because a bar has room for it and a
              question nobody sees is a default nobody chose. */}
          <span className="prompt-chip">
            <span className="chip-key">if closed</span>
            <select
              aria-label="If JaBot was closed"
              value={catchUp}
              onChange={(event) =>
                setCatchUp(event.target.value as CatchUpPolicy)
              }
            >
              <option value="once">run the missed one</option>
              <option value="skip">skip it</option>
            </select>
          </span>

          <button
            type="submit"
            className="send-btn"
            aria-label="Create schedule"
            disabled={!written || busy}
          >
            <ArrowUpIcon />
          </button>
        </div>
      </form>
    </div>
  );
}
