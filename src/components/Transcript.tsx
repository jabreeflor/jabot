/**
 * The transcript: ACP-shaped items rendered in the prototype's grammar.
 *
 * Items arrive one tool call at a time because that is how `session/update`
 * reports them, but a consecutive run of them is drawn as a *single* toolblock.
 * One agent turn that read six files and ran the tests is one thing that
 * happened, and six stacked cards would read as six turns.
 */

import type { ToolCall, ToolKind, TranscriptItem } from "./types";

export function Transcript({
  items,
  onAction,
}: {
  items: readonly TranscriptItem[];
  /** A notice card's button — a fold offer today, a permission reply in #20. */
  onAction?: (itemId: string, actionId: string) => void;
}) {
  return (
    <div className="transcript">
      {groupToolRuns(items).map((entry) =>
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

function TranscriptEntry({
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
      return (
        <div className="msg bot">
          <div className="bubble" data-streaming={item.streaming || undefined}>
            {item.text}
          </div>
        </div>
      );
    case "notice":
      return <Notice item={item} onAction={onAction} />;
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
        {item.pill && <span className="pill">{item.pill}</span>}
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

/** ACP tool kinds in the prototype's verbs. `execute` has always read "bash". */
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

function ToolBlock({ calls }: { calls: readonly ToolCall[] }) {
  return (
    <pre className="toolblock">
      {calls.map((call) => (
        <div className="call" key={call.id}>
          <span className="verb">▸ {VERBS[call.kind].padEnd(5)}</span>{" "}
          {call.target}
          <ToolMarker call={call} />
        </div>
      ))}
    </pre>
  );
}

function ToolMarker({ call }: { call: ToolCall }) {
  switch (call.status) {
    case "pending":
      return <span className="spin">{"  ◌ waiting"}</span>;
    case "in_progress":
      return <span className="spin">{`  ● ${call.note ?? "running…"}`}</span>;
    case "completed":
      return call.note ? <span className="tick">{`  ✓ ${call.note}`}</span> : null;
    case "failed":
      return <span className="fail">{`  ✗ ${call.note ?? "failed"}`}</span>;
    case "cancelled":
      return <span className="fail">{`  ✗ ${call.note ?? "cancelled"}`}</span>;
  }
}
