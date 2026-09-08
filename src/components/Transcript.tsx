//! The transcript: ACP-shaped items rendered in the prototype's grammar.
//!
//! Items arrive one tool call at a time because that is how `session/update`
//! reports them, but a consecutive run of them is drawn as a *single* toolblock.
//! One agent turn that read six files and ran the tests is one thing that
//! happened, and six stacked cards would read as six turns.

import { memo, useEffect, useId, useMemo, useRef, useState } from "react";

import { copyText } from "./copyText";
import {
  BranchIcon,
  CaretRightIcon,
  CheckIcon,
  CopyIcon,
  CrossIcon,
  DotIcon,
  RingIcon,
  SmileIcon,
  SparkIcon,
} from "./Icon";
import { renderMarkdown } from "./markdown";
import { REACTION_CHOICES, reactionName } from "./reactions";
import type { ToolCall, ToolKind, TranscriptItem } from "./types";

/**
 * How many grouped entries are rendered from the tail to begin with, and how
 * many more each time the reader reaches the top of what is drawn.
 *
 * Windowed from the *end*, growing upward, and never absolutely positioned.
 * Bubbles and toolblocks have unknown heights, so a virtual list would need a
 * measurement cache and would get every guess wrong the first time a code
 * block or a long tool run appeared. Growing a tail window costs a slice.
 */
const WINDOW = 80;

export function Transcript({
  items,
  onAction,
  onReact,
  onBranch,
  branchingSeq,
}: {
  items: readonly TranscriptItem[];
  /** A notice card's button — a fold offer today, a permission reply in #20. */
  onAction?: (itemId: string, actionId: string) => void;
  /** Toggle an emoji on an agent bubble (#265). */
  onReact?: (itemId: string, emoji: string) => void;
  /** Code chats only (#266): fork the conversation through this message. */
  onBranch?: (itemId: string, seq: number) => void;
  /** Seq currently being forked, so the control can show a loading state. */
  branchingSeq?: number | null;
}) {
  // Streaming is why this is memoized rather than recomputed. #14's reducer
  // returns a new array whose *other elements are the same objects*, so with
  // the grouping cached on `items` and the rows below memoized, appending a
  // chunk re-renders one bubble instead of the whole conversation.
  const groups = useMemo(() => groupToolRuns(items), [items]);
  const [window, setWindow] = useState(WINDOW);

  // The *grouped* array is sliced, never `items`.
  //
  // Be precise about what that buys, because it is less than it looks.
  // Slicing `items` first would hand `groupToolRuns` a new array on every
  // render and make it re-walk the window per streamed chunk — the work the
  // memo above exists to avoid. It would *not* cost the row memoization:
  // slicing copies the array, not the elements, so `entry.item` and the
  // individual `calls` stay the same objects either way and both
  // `memo(TranscriptRow)` and `sameCalls` still bail out.
  //
  // So this is a cost argument, not a correctness one, and the identity test
  // in transcript.test.tsx pins the rows rather than this choice.
  const visible =
    groups.length > window ? groups.slice(groups.length - window) : groups;
  const hidden = groups.length - visible.length;

  return (
    <div className="transcript">
      {hidden > 0 && (
        <button
          type="button"
          className="show-earlier"
          onClick={() => setWindow((size) => size + WINDOW)}
        >
          Show earlier — {hidden} more
        </button>
      )}
      {visible.map((entry) =>
        entry.type === "tools" ? (
          <ToolBlock key={entry.key} calls={entry.calls} />
        ) : (
          <TranscriptEntry
            key={entry.item.id}
            item={entry.item}
            onAction={onAction}
            onReact={onReact}
            onBranch={onBranch}
            branchingSeq={branchingSeq}
          />
        ),
      )}
    </div>
  );
}

type Grouped =
  | { type: "tools"; key: string; calls: ToolCall[] }
  | { type: "item"; item: Exclude<TranscriptItem, { kind: "tool" }> };

/** Exported for the test that pins the grouping rule. */
export function groupToolRuns(items: readonly TranscriptItem[]): Grouped[] {
  const out: Grouped[] = [];
  for (const item of items) {
    if (item.kind === "tool") {
      const last = out[out.length - 1];
      if (last?.type === "tools") {
        last.calls.push(item.call);
      } else {
        out.push({ type: "tools", key: item.id, calls: [item.call] });
      }
    } else {
      out.push({ type: "item", item });
    }
  }
  return out;
}

const ToolBlock = memo(ToolBlockRow, (before, after) =>
  sameCalls(before.calls, after.calls),
);

/**
 * An agent's reply, as markdown (#14).
 *
 * Its own component so the parse can be memoized on the text. Appending a
 * chunk mid-stream replaces this one item and leaves every sibling the same
 * object, which is what `memo(TranscriptRow)` above keys on — so a streamed
 * token reparses one bubble and re-renders nothing else. Parsing inline in the
 * switch would reparse the whole conversation on every chunk.
 */
function AgentBubble({
  item,
  onReact,
  onBranch,
  branchingSeq,
}: {
  item: Extract<TranscriptItem, { kind: "agent" }>;
  onReact?: (itemId: string, emoji: string) => void;
  onBranch?: (itemId: string, seq: number) => void;
  branchingSeq?: number | null;
}) {
  const nodes = useMemo(() => renderMarkdown(item.text), [item.text]);
  const reactions = item.reactions ?? [];
  const canReact = onReact !== undefined;
  const showBranch = canOfferBranch(onBranch, item.seq, item.streaming);
  return (
    <div className="msg bot">
      <div className="bot-turn">
        <div className="bubble" data-streaming={item.streaming || undefined}>
          {nodes}
        </div>
      </div>
      {(item.text.length > 0 ||
        showBranch ||
        canReact ||
        reactions.length > 0) && (
        <div className="msg-actions" role="group" aria-label="Message actions">
          {item.text.length > 0 && <CopyResponseButton text={item.text} />}
          {showBranch && onBranch && item.seq !== undefined && (
            <BranchButton
              itemId={item.id}
              seq={item.seq}
              onBranch={onBranch}
              branchingSeq={branchingSeq}
            />
          )}
          {(canReact || reactions.length > 0) && (
            <ReactionBar
              itemId={item.id}
              reactions={reactions}
              onReact={onReact}
            />
          )}
        </div>
      )}
    </div>
  );
}

const COPY_IDLE = "Copy response";
const COPY_OK = "Copied";
const COPY_FAIL = "Couldn't copy to the clipboard";
const COPY_FEEDBACK_MS = 2000;

/**
 * Copy this reply's source text — markdown, fences, line breaks — not the
 * rendered bubble and not the chrome around it. Confirmation lives on the
 * control so a success or a refusal is the same place the click was.
 */
function CopyResponseButton({ text }: { text: string }) {
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (status === "idle") return;
    const id = window.setTimeout(() => setStatus("idle"), COPY_FEEDBACK_MS);
    return () => window.clearTimeout(id);
  }, [status]);

  const tooltip =
    status === "copied" ? COPY_OK : status === "failed" ? COPY_FAIL : COPY_IDLE;

  function onCopy() {
    void copyText(text).then(
      () => setStatus("copied"),
      () => setStatus("failed"),
    );
  }

  return (
    <>
      <button
        type="button"
        className="msg-action"
        aria-label={COPY_IDLE}
        data-tooltip={tooltip}
        data-state={status === "idle" ? undefined : status}
        onClick={onCopy}
      >
        {status === "copied" ? <CheckIcon /> : <CopyIcon />}
      </button>
      <span
        className="msg-action-status"
        role={status === "failed" ? "alert" : "status"}
        aria-live={status === "failed" ? "assertive" : "polite"}
      >
        {status === "idle" ? "" : tooltip}
      </span>
    </>
  );
}

function ReactionBar({
  itemId,
  reactions,
  onReact,
}: {
  itemId: string;
  reactions: readonly string[];
  onReact?: (itemId: string, emoji: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
        addRef.current?.focus();
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    rootRef.current
      ?.querySelector<HTMLButtonElement>("[role='menuitem']")
      ?.focus();
  }, [open]);

  function choose(emoji: string) {
    setOpen(false);
    onReact?.(itemId, emoji);
    addRef.current?.focus();
  }

  return (
    <div className="react-bar" ref={rootRef}>
      {reactions.length > 0 && (
        <ul className="react-marks" aria-label="Reactions">
          {reactions.map((emoji) => {
            const name = reactionName(emoji);
            return (
              <li key={emoji}>
                {onReact ? (
                  <button
                    type="button"
                    className="react-badge"
                    aria-pressed="true"
                    aria-label={`Remove ${name} reaction`}
                    onClick={() => onReact(itemId, emoji)}
                  >
                    {emoji}
                  </button>
                ) : (
                  <span className="react-badge" aria-label={`${name} reaction`}>
                    {emoji}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {onReact && (
        <div className="react-add-wrap">
          <button
            ref={addRef}
            type="button"
            className="react-add"
            aria-haspopup="menu"
            aria-expanded={open}
            aria-controls={open ? menuId : undefined}
            aria-label="Add reaction"
            onClick={() => setOpen((was) => !was)}
          >
            <SmileIcon />
          </button>
          {open && (
            <div
              className="react-pick"
              id={menuId}
              role="menu"
              aria-label="Choose a reaction"
            >
              {REACTION_CHOICES.map((choice) => {
                const selected = reactions.includes(choice.emoji);
                return (
                  <button
                    key={choice.emoji}
                    type="button"
                    role="menuitem"
                    aria-label={
                      selected
                        ? `Remove ${choice.name} reaction`
                        : `React with ${choice.name}`
                    }
                    data-selected={selected || undefined}
                    onClick={() => choose(choice.emoji)}
                  >
                    {choice.emoji}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Identity, not deep equality: the reducer replaces exactly the call it
    changed, so a per-element `===` is both correct and O(n) on pointers. */
function sameCalls(a: readonly ToolCall[], b: readonly ToolCall[]): boolean {
  return a.length === b.length && a.every((call, i) => call === b[i]);
}

const TranscriptEntry = memo(TranscriptRow);

function TranscriptRow({
  item,
  onAction,
  onReact,
  onBranch,
  branchingSeq,
}: {
  item: Exclude<TranscriptItem, { kind: "tool" }>;
  onAction?: (itemId: string, actionId: string) => void;
  onReact?: (itemId: string, emoji: string) => void;
  onBranch?: (itemId: string, seq: number) => void;
  branchingSeq?: number | null;
}) {
  switch (item.kind) {
    case "stamp":
      return <div className="stamp">{item.text}</div>;
    case "sys":
      return (
        <div className="sys" role="status">
          {item.text}
        </div>
      );
    case "user":
      return (
        <div className="msg me">
          <div className="bubble">{item.text}</div>
          {canOfferBranch(onBranch, item.seq) &&
            onBranch &&
            item.seq !== undefined && (
              <div
                className="msg-actions"
                role="group"
                aria-label="Message actions"
              >
                <BranchButton
                  itemId={item.id}
                  seq={item.seq}
                  onBranch={onBranch}
                  branchingSeq={branchingSeq}
                />
              </div>
            )}
        </div>
      );
    case "agent":
      return (
        <AgentBubble
          item={item}
          onReact={onReact}
          onBranch={onBranch}
          branchingSeq={branchingSeq}
        />
      );
    case "notice":
      return <Notice item={item} onAction={onAction} />;
    // Unreachable through the reducer, which only ever builds the kinds above.
    // Present because a component that returns `undefined` is a React error,
    // and one unmapped item must not blank the conversation.
    default:
      return null;
  }
}

/** Fork through this bubble's last transcript seq (#266). Hidden while the
    reply is still streaming — a cut mid-token is not a conversation. */
function canOfferBranch(
  onBranch: ((itemId: string, seq: number) => void) | undefined,
  seq: number | undefined,
  streaming?: boolean,
): boolean {
  return Boolean(onBranch) && seq !== undefined && seq >= 1 && !streaming;
}

function BranchButton({
  itemId,
  seq,
  onBranch,
  branchingSeq,
}: {
  itemId: string;
  seq: number;
  onBranch: (itemId: string, seq: number) => void;
  branchingSeq?: number | null;
}) {
  const busy = branchingSeq === seq;
  const branchLabel = busy ? "Branching…" : "Branch in new chat";
  return (
    <button
      type="button"
      className="msg-action"
      aria-label={branchLabel}
      data-tooltip={branchLabel}
      aria-busy={busy || undefined}
      disabled={branchingSeq != null}
      onClick={() => onBranch(itemId, seq)}
    >
      <BranchIcon />
    </button>
  );
}

function Notice({
  item,
  onAction,
}: {
  item: Extract<TranscriptItem, { kind: "notice" }>;
  onAction?: (itemId: string, actionId: string) => void;
}) {
  return (
    <div className={item.resolved ? "notice leaving" : "notice"}>
      <div className="r1">
        <b>{item.title}</b>
        {item.pill && (
          <span className="pill">
            <SparkIcon />
            {item.pill}
          </span>
        )}
      </div>
      <p>{item.body}</p>
      <div className="acts">
        {item.actions.map((action) => (
          <button
            key={action.id}
            type="button"
            className={action.primary ? "btn primary" : "btn"}
            disabled={item.resolved}
            onClick={() => onAction?.(item.id, action.id)}
          >
            {action.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** ACP tool kinds in the prototype's verbs. `execute` has always read "bash".
 *
 * Read through [`verb`], never indexed directly: ACP adds kinds, adapters
 * invent them, and a `Record` lookup that misses returns `undefined` — which
 * `padEnd` then throws on, taking the whole transcript with it.
 */
const VERBS: Record<ToolKind, string> = {
  read: "read",
  edit: "edit",
  write: "write",
  execute: "bash",
  search: "grep",
  fetch: "fetch",
  think: "think",
  delete: "rm",
  move: "mv",
  other: "tool",
};

function ToolBlockRow({ calls }: { calls: readonly ToolCall[] }) {
  return (
    <pre className="toolblock">
      {calls.map((call) => (
        <div className="call" key={call.id}>
          <span className="verb">
            <CaretRightIcon /> {verb(call.kind).padEnd(5)}
          </span>{" "}
          {call.target}
          <ToolMarker call={call} />
        </div>
      ))}
    </pre>
  );
}

function verb(kind: ToolKind): string {
  return VERBS[kind] ?? "tool";
}

function ToolMarker({ call }: { call: ToolCall }) {
  switch (call.status) {
    case "pending":
      return (
        <span className="spin">
          {"  "}
          <RingIcon />
          {" waiting"}
        </span>
      );
    case "in_progress":
      return (
        <span className="spin">
          {"  "}
          <DotIcon />
          {` ${call.note ?? "running…"}`}
        </span>
      );
    case "completed":
      return call.note ? (
        <span className="tick">
          {"  "}
          <CheckIcon />
          {` ${call.note}`}
        </span>
      ) : null;
    case "failed":
      return (
        <span className="fail">
          {"  "}
          <CrossIcon />
          {` ${call.note ?? "failed"}`}
        </span>
      );
    case "cancelled":
      return (
        <span className="fail">
          {"  "}
          <CrossIcon />
          {` ${call.note ?? "cancelled"}`}
        </span>
      );
    // A status from an ACP version this build has never met. Returning
    // nothing at all from a component is a React error, so the line renders
    // without a marker rather than not rendering.
    default:
      return null;
  }
}
