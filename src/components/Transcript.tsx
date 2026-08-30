//! The transcript: ACP-shaped items rendered in the prototype's grammar.
//!
//! Items arrive one tool call at a time because that is how `session/update`
//! reports them, but a consecutive run of them is drawn as a *single* toolblock.
//! One agent turn that read six files and ran the tests is one thing that
//! happened, and six stacked cards would read as six turns.

import { memo, useMemo } from "react";

import {
  CaretRightIcon,
  CheckIcon,
  CrossIcon,
  DotIcon,
  RingIcon,
  SparkIcon,
} from "./Icon";
import { renderMarkdown } from "./markdown";
import type { ToolCall, ToolKind, TranscriptItem } from "./types";

export function Transcript({
  items,
  onAction,
}: {
  items: readonly TranscriptItem[];
  /** A notice card's button — a fold offer today, a permission reply in #20. */
  onAction?: (itemId: string, actionId: string) => void;
}) {
  // Streaming is why this is memoized rather than recomputed. #14's reducer
  // returns a new array whose *other elements are the same objects*, so with
  // the grouping cached on `items` and the rows below memoized, appending a
  // chunk re-renders one bubble instead of the whole conversation.
  const groups = useMemo(() => groupToolRuns(items), [items]);
  return (
    <div className="transcript">
      {groups.map((entry) =>
        entry.type === "tools" ? (
          <ToolBlock key={entry.key} calls={entry.calls} />
        ) : (
          <TranscriptEntry
            key={entry.item.id}
            item={entry.item}
            onAction={onAction}
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
function AgentBubble({ item }: { item: Extract<TranscriptItem, { kind: "agent" }> }) {
  const nodes = useMemo(() => renderMarkdown(item.text), [item.text]);
  return (
    <div className="msg bot">
      <div className="bubble" data-streaming={item.streaming || undefined}>
        {nodes}
      </div>
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
}: {
  item: Exclude<TranscriptItem, { kind: "tool" }>;
  onAction?: (itemId: string, actionId: string) => void;
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
        </div>
      );
    case "agent":
      return <AgentBubble item={item} />;
    case "notice":
      return <Notice item={item} onAction={onAction} />;
    // Unreachable through the reducer, which only ever builds the kinds above.
    // Present because a component that returns `undefined` is a React error,
    // and one unmapped item must not blank the conversation.
    default:
      return null;
  }
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
