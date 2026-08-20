//! ACP `session/update` → the prototype's chat grammar (#14).
//!
//! The whole file is one pure reducer plus the hook that feeds it. That split
//! is the point: the *same* function reduces a live notification and a row
//! replayed out of SQLite, so a reopened thread cannot look different from the
//! one you were watching a second ago. If the mapping lived in the hook, the
//! replay path would need a second copy of it and the two would drift.
//!
//! Two rules the shapes here exist to keep:
//!
//! **Nothing an adapter can say may take the transcript down.** A `kind` no
//! ACP version has defined, a status we have never heard of, a payload that is
//! not an object at all — each has a defined landing place. An agent is
//! third-party software; a transcript that dies on an unrecognised enum is a
//! chat that dies on its first unfamiliar tool.
//!
//! **Appending must not rebuild the world.** Every update returns a new array
//! with *the same element objects* except the one that changed, so
//! `React.memo` on the rows below turns a chunk into one re-render instead of
//! a thousand. The tests pin object identity, because that is the property —
//! not the render count of any particular component.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  HostClient,
  JsonRpcNotification,
  PromptMode,
  QueuedPromptView,
  SessionUpdateParams,
  ThreadTranscriptResult,
  TranscriptEventView,
} from "../host";
import { SESSION_UPDATE } from "../host";
import type { StatusTone, ThreadStatus } from "../components/status";
import type {
  ToolCall,
  ToolKind,
  ToolStatus,
  TranscriptItem,
} from "../components/types";

/** Where the agent has got to in its own plan — the header's "step 3/7". */
export interface PlanProgress {
  done: number;
  total: number;
  /** The entry it says it is on, when one is marked `in_progress`. */
  current?: string;
}

/**
 * Everything one thread's chat is drawn from.
 *
 * `busy` is the renderer's own reading of the stream (a turn started and has
 * not ended), deliberately not a second copy of the run ledger: the ledger is
 * the host's answer and arrives by request, this one arrives with the events
 * and is what the composer switches on between polls.
 */
export interface ThreadStream {
  items: readonly TranscriptItem[];
  plan: PlanProgress | null;
  lastStopReason: string | null;
  busy: boolean;
  /** The highest transcript `seq` folded in, live or replayed. */
  headSeq: number;
  /**
   * How far the *store replay* got. Live events at or below it are copies of
   * rows already drawn; above it, everything is applied.
   *
   * Deliberately not `headSeq`. De-duplicating against a running high-water
   * mark would discard a live event that arrived out of order behind a later
   * one — and out-of-order is a thing a host with two notification drainers
   * can do. A boundary only ever moves when a replay says it has.
   */
  hydratedSeq: number;
  /** Prompts the host is holding for the turn in flight, oldest first. */
  queued: readonly string[];
  /** Index of each live `toolCallId`, so an update lands on its own line. */
  toolIndex: Readonly<Record<string, number>>;
  /** The bubble a chunk is currently being appended to. */
  open: { kind: "user" | "agent"; index: number } | null;
  /** Fallback id source for a host that persists nothing (no `transcriptSeq`). */
  counter: number;
}

export const EMPTY_STREAM: ThreadStream = {
  items: [],
  plan: null,
  lastStopReason: null,
  busy: false,
  headSeq: 0,
  hydratedSeq: 0,
  queued: [],
  toolIndex: {},
  open: null,
  counter: 0,
};

const TOOL_KINDS: readonly ToolKind[] = [
  "read",
  "edit",
  "write",
  "execute",
  "search",
  "fetch",
  "think",
  "delete",
  "move",
  "other",
];

const TOOL_STATUSES: readonly ToolStatus[] = [
  "pending",
  "in_progress",
  "completed",
  "failed",
  "cancelled",
];

/** Replay a `thread/transcript` answer. Same reducer, same result. */
export function hydrate(
  result: ThreadTranscriptResult,
  from: ThreadStream = EMPTY_STREAM,
): ThreadStream {
  let stream = result.events.reduce(
    (acc: ThreadStream, event: TranscriptEventView) =>
      applyAcpEvent(acc, event.payload, event.seq),
    from,
  );
  // The head is the log's, not the last row we happened to be given: a window
  // (`limit`) leaves older rows behind and a caller that is up to date is
  // given none at all. Everything at or below it is now drawn, so that is the
  // boundary the live stream is de-duplicated against.
  const head = Math.max(stream.headSeq, result.headSeq);
  stream = {
    ...stream,
    headSeq: head,
    hydratedSeq: Math.max(stream.hydratedSeq, result.headSeq),
  };
  return withQueued(stream, result.queued);
}

/** The queue is host state, not a stream event — it replaces wholesale. */
function withQueued(
  stream: ThreadStream,
  queued: readonly QueuedPromptView[],
): ThreadStream {
  const texts = queued.map((prompt) => contentText(prompt.content));
  if (sameStrings(stream.queued, texts)) return stream;
  return { ...stream, queued: texts };
}

/**
 * Fold one ACP `session/update` payload into the stream.
 *
 * `seq` is the `transcript_events` row this event landed in, when it landed in
 * one. An event at or below the head is one we already have — that is the
 * whole of the de-duplication between the replay and the live stream, and it
 * is exact because the host stamps both from the same counter.
 */
export function applyAcpEvent(
  stream: ThreadStream,
  payload: unknown,
  seq?: number,
): ThreadStream {
  if (typeof seq === "number" && seq <= stream.hydratedSeq) return stream;
  const next =
    typeof seq === "number"
      ? { ...stream, headSeq: Math.max(stream.headSeq, seq) }
      : stream;
  const update = asRecord(payload);
  // Not an object, or an object with no `sessionUpdate`: nothing we can map,
  // and nothing worth taking the chat down for.
  if (!update) return next;

  switch (str(update.sessionUpdate)) {
    case "user_message_chunk":
      return chunk(next, "user", blockText(update.content));
    case "agent_message_chunk":
      return chunk(next, "agent", blockText(update.content));
    // Reasoning is not the transcript. ACP has a `think` tool kind for the
    // work an agent chooses to show; a raw thought stream would double the
    // length of every chat with text the prototype has no bubble for.
    case "agent_thought_chunk":
      return next;
    case "tool_call":
    case "tool_call_update":
      return toolCall(next, update);
    case "plan":
      return { ...closeBubble(next), plan: planProgress(update.entries) };
    case "state_update":
      return stateUpdate(next, update);
    // `available_commands_update`, `current_mode_update`, and whatever ACP
    // adds next. Ignored on purpose, and ignored *safely*.
    default:
      return next;
  }
}

/** What the thread header says, given the stream and the ledger's last word. */
export function streamStatus(
  stream: ThreadStream,
  fallback: ThreadStatus,
): ThreadStatus {
  if (stream.busy) {
    const label = stream.plan
      ? `running · step ${Math.min(stream.plan.done + 1, stream.plan.total)}/${stream.plan.total}`
      : "running";
    return { label, tone: "running" };
  }
  if (stream.lastStopReason) {
    return stopReasonStatus(stream.lastStopReason);
  }
  return fallback;
}

/** Stop reasons are the completion signal, and they are not all "done". */
function stopReasonStatus(reason: string): ThreadStatus {
  const tone: StatusTone =
    reason === "end_turn" ? "ok" : reason === "cancelled" ? "quiet" : "bad";
  return { label: stopReasonLabel(reason), tone };
}

function stopReasonLabel(reason: string): string {
  switch (reason) {
    case "end_turn":
      return "done";
    case "cancelled":
      return "cancelled";
    case "max_tokens":
      return "stopped: out of tokens";
    case "max_turn_requests":
      return "stopped: too many steps";
    case "refusal":
      return "refused";
    default:
      return `stopped: ${reason}`;
  }
}

// ---- reducer internals ----------------------------------------------------

function chunk(
  stream: ThreadStream,
  kind: "user" | "agent",
  text: string,
): ThreadStream {
  if (!text) return stream;
  const open = stream.open;
  if (open && open.kind === kind) {
    const previous = stream.items[open.index];
    if (previous && previous.kind === kind) {
      return {
        ...stream,
        items: replaceAt(stream.items, open.index, {
          ...previous,
          text: previous.text + text,
        }),
      };
    }
  }
  const item: TranscriptItem =
    kind === "user"
      ? { kind: "user", id: nextId(stream), text }
      : { kind: "agent", id: nextId(stream), text, streaming: stream.busy };
  return {
    ...stream,
    items: [...stream.items, item],
    open: { kind, index: stream.items.length },
    counter: stream.counter + 1,
  };
}

/**
 * One tool line, created or updated in place.
 *
 * An update for a call we never saw creates the line rather than being
 * dropped: `session/load` replays, a client that connected mid-turn, and an
 * adapter that only ever sends `tool_call_update` all produce exactly that,
 * and a missing line is a worse answer than a line with a late start.
 */
function toolCall(stream: ThreadStream, update: Record<string, unknown>): ThreadStream {
  const callId = str(update.toolCallId) ?? str(update.id);
  if (!callId) return stream;
  const at = stream.toolIndex[callId];
  const existing =
    at === undefined ? undefined : asToolItem(stream.items[at]);

  const call: ToolCall = {
    id: callId,
    kind: toolKind(update.kind) ?? existing?.kind ?? "other",
    target: toolTarget(update) ?? existing?.target ?? callId,
    status: toolStatus(update.status) ?? existing?.status ?? "pending",
    note: toolNote(update) ?? existing?.note,
  };

  if (at !== undefined && existing) {
    if (sameCall(existing, call)) return stream;
    const item = stream.items[at];
    return {
      ...stream,
      items: replaceAt(stream.items, at, { ...item, kind: "tool", call }),
    };
  }
  const closed = closeBubble(stream);
  return {
    ...closed,
    items: [...closed.items, { kind: "tool", id: `tool-${callId}`, call }],
    toolIndex: { ...closed.toolIndex, [callId]: closed.items.length },
  };
}

function stateUpdate(
  stream: ThreadStream,
  update: Record<string, unknown>,
): ThreadStream {
  const jabot = asRecord(update.jabot);
  // The host's own note that a queued prompt is never going to be sent. It is
  // the user's text, so it is said out loud rather than dropped in silence.
  if (jabot && str(jabot.event) === "prompt_dropped") {
    const text = contentText(jabot.content);
    const reason = str(jabot.reason) ?? "the session ended";
    const closed = closeBubble(stream);
    return {
      ...closed,
      items: [
        ...closed.items,
        {
          kind: "sys",
          id: nextId(closed),
          text: `Not sent — ${reason}: “${text}”`,
        },
      ],
      counter: closed.counter + 1,
      queued: [],
    };
  }

  const stopReason = str(update.stopReason);
  const idle = str(update.sessionState) === "idle";
  if (!stopReason && !idle) return stream;
  const closed = endTurn(stream);
  if (!stopReason) {
    // Idleness with no stop reason is a v2 adapter reporting it went quiet.
    // It ends the streaming bubble; it does not claim an outcome (#15/D-006).
    return closed;
  }
  return {
    ...closed,
    lastStopReason: stopReason,
    items: [
      ...closed.items,
      { kind: "sys", id: nextId(closed), text: sysLine(stopReason) },
    ],
    counter: closed.counter + 1,
  };
}

function sysLine(stopReason: string): string {
  switch (stopReason) {
    case "end_turn":
      return "Session finished.";
    case "cancelled":
      return "Cancelled.";
    default:
      return `Session stopped: ${stopReasonLabel(stopReason)}.`;
  }
}

/** A turn began: the composer is busy and the queue can start filling. */
export function markPromptSent(stream: ThreadStream): ThreadStream {
  return { ...stream, busy: true, lastStopReason: null };
}

/** A prompt the host is holding, shown before it has been sent to anyone. */
export function markPromptQueued(
  stream: ThreadStream,
  text: string,
): ThreadStream {
  return { ...stream, queued: [...stream.queued, text] };
}

function endTurn(stream: ThreadStream): ThreadStream {
  const closed = closeBubble(stream);
  return { ...closed, busy: false, plan: null };
}

/** Stop appending to the open bubble, and stop calling it streaming. */
function closeBubble(stream: ThreadStream): ThreadStream {
  const open = stream.open;
  if (!open) return stream;
  const item = stream.items[open.index];
  if (item && item.kind === "agent" && item.streaming) {
    return {
      ...stream,
      open: null,
      items: replaceAt(stream.items, open.index, {
        ...item,
        streaming: false,
      }),
    };
  }
  return { ...stream, open: null };
}

/**
 * A new array whose every other element is the *same object*.
 *
 * This is what makes streaming cheap: React sees one changed child and the
 * memoized rows around it skip rendering entirely.
 */
function replaceAt(
  items: readonly TranscriptItem[],
  index: number,
  item: TranscriptItem,
): TranscriptItem[] {
  const next = items.slice();
  next[index] = item;
  return next;
}

function nextId(stream: ThreadStream): string {
  return stream.headSeq > 0
    ? `e${stream.headSeq}-${stream.counter}`
    : `n${stream.counter}`;
}

function planProgress(entries: unknown): PlanProgress | null {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const rows = entries.map(asRecord);
  const done = rows.filter((row) => str(row?.status) === "completed").length;
  const current = rows.find((row) => str(row?.status) === "in_progress");
  return {
    done,
    total: rows.length,
    current: current ? (str(current.content) ?? undefined) : undefined,
  };
}

// ---- ACP shapes -----------------------------------------------------------

/** An unknown kind is `other`, never a crash and never a blank line. */
function toolKind(value: unknown): ToolKind | undefined {
  const raw = str(value);
  if (!raw) return undefined;
  return TOOL_KINDS.includes(raw as ToolKind) ? (raw as ToolKind) : "other";
}

function toolStatus(value: unknown): ToolStatus | undefined {
  const raw = str(value);
  if (!raw) return undefined;
  return TOOL_STATUSES.includes(raw as ToolStatus)
    ? (raw as ToolStatus)
    : "in_progress";
}

/** What the line says it acted on: the title, else a path, else the command. */
function toolTarget(update: Record<string, unknown>): string | undefined {
  const title = str(update.title);
  if (title) return title;
  const locations = Array.isArray(update.locations) ? update.locations : [];
  const first = asRecord(locations[0]);
  const path = first ? str(first.path) : undefined;
  if (path) return path;
  const raw = asRecord(update.rawInput);
  if (raw) {
    return (
      str(raw.command) ??
      str(raw.path) ??
      str(raw.file_path) ??
      str(raw.query) ??
      undefined
    );
  }
  return undefined;
}

/**
 * The trailing note: the prototype's "+18 −7" for an edit, or the last line of
 * output for anything that printed some.
 */
function toolNote(update: Record<string, unknown>): string | undefined {
  const content = Array.isArray(update.content) ? update.content : [];
  const diffs = content
    .map(asRecord)
    .filter(
      (block): block is Record<string, unknown> =>
        block !== undefined && str(block.type) === "diff",
    );
  if (diffs.length > 0) {
    let added = 0;
    let removed = 0;
    for (const diff of diffs) {
      const stat = diffStat(diff);
      added += stat.added;
      removed += stat.removed;
    }
    return `+${added} −${removed}`;
  }
  for (let i = content.length - 1; i >= 0; i -= 1) {
    const text = blockText(content[i]);
    const last = text.trimEnd().split("\n").filter(Boolean).pop();
    if (last) return last.length > 56 ? `${last.slice(0, 55)}…` : last;
  }
  return undefined;
}

/**
 * Lines added and removed.
 *
 * A `gitPatch` is counted exactly, because it is already a diff. Structured
 * `oldText`/`newText` are counted as a line multiset difference — exact for
 * pure insertions and deletions, and an honest approximation for a rewrite.
 * The alternative is an LCS over two whole files on every chunk, which is a
 * lot of work for a number in a toolblock.
 */
export function diffStat(diff: Record<string, unknown>): {
  added: number;
  removed: number;
} {
  const patch = str(diff.gitPatch) ?? str(diff.git_patch);
  if (patch) {
    let added = 0;
    let removed = 0;
    for (const line of patch.split("\n")) {
      if (line.startsWith("+++") || line.startsWith("---")) continue;
      if (line.startsWith("+")) added += 1;
      else if (line.startsWith("-")) removed += 1;
    }
    return { added, removed };
  }
  const oldLines = lines(str(diff.oldText) ?? str(diff.old_text));
  const newLines = lines(str(diff.newText) ?? str(diff.new_text));
  const counts = new Map<string, number>();
  for (const line of oldLines) counts.set(line, (counts.get(line) ?? 0) + 1);
  let added = 0;
  for (const line of newLines) {
    const seen = counts.get(line) ?? 0;
    if (seen > 0) counts.set(line, seen - 1);
    else added += 1;
  }
  let removed = 0;
  for (const remaining of counts.values()) removed += remaining;
  return { added, removed };
}

function lines(text: string | undefined): string[] {
  if (!text) return [];
  const split = text.split("\n");
  if (split[split.length - 1] === "") split.pop();
  return split;
}

/** ACP content: one block, an array of them, or a bare string. */
function blockText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(blockText).join("");
  const block = asRecord(content);
  if (!block) return "";
  const text = str(block.text);
  if (text) return text;
  const nested = block.content;
  if (nested !== undefined && nested !== content) return blockText(nested);
  return "";
}

/** A `session/prompt` content value as one line of text, for a queue chip. */
function contentText(content: unknown): string {
  if (typeof content === "string") return content;
  const text = blockText(content);
  return text || JSON.stringify(content ?? "");
}

function asToolItem(item: TranscriptItem | undefined): ToolCall | undefined {
  return item && item.kind === "tool" ? item.call : undefined;
}

function sameCall(a: ToolCall, b: ToolCall): boolean {
  return (
    a.kind === b.kind &&
    a.target === b.target &&
    a.status === b.status &&
    a.note === b.note
  );
}

function sameStrings(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((value, i) => value === b[i]);
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function str(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

// ---- the hook -------------------------------------------------------------

export interface LiveTranscript {
  stream: ThreadStream;
  /** Why the transcript could not be loaded, for the line that says so. */
  error: string | null;
  loading: boolean;
  /** Send a turn. Queues behind one in flight instead of being refused. */
  send: (text: string) => void;
  /**
   * Cancel the turn in flight.
   *
   * Also how the UI interrupts: anything the user typed meanwhile is already
   * queued, so ending the turn is what lets it go. `mode: "interrupt"` on the
   * wire does the same two things in one call, for a client that wants to
   * cancel and enqueue atomically; the composer does not need that, because it
   * has already enqueued by the time the button exists.
   */
  cancel: () => void;
}

/**
 * One thread's chat, hydrated from the store and kept up to date by
 * `session/update`.
 *
 * The order matters and is the reason this is a hook rather than two effects:
 * the subscription is installed *before* `thread/transcript` is asked for, and
 * events that arrive while the answer is in flight are buffered. Every one of
 * them carries the `seq` its row got, so replaying the buffer after hydrating
 * drops exactly the events the answer already contained. Subscribing after the
 * read would instead lose whatever arrived in between.
 */
export function useThreadTranscript(
  client: HostClient | null,
  threadId: string | null,
): LiveTranscript {
  const [stream, setStream] = useState<ThreadStream>(EMPTY_STREAM);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const streamRef = useRef(stream);
  streamRef.current = stream;

  useEffect(() => {
    setStream(EMPTY_STREAM);
    setError(null);
    setLoading(true);
    if (!client || !threadId) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    let hydrated = false;
    const buffered: SessionUpdateParams[] = [];

    const apply = (params: SessionUpdateParams) => {
      setStream((current) =>
        applyAcpEvent(current, params.acp, params.transcriptSeq),
      );
    };

    const unsubscribe = client.onNotification(
      (notification: JsonRpcNotification) => {
        if (notification.method !== SESSION_UPDATE) return;
        const params = notification.params as SessionUpdateParams | undefined;
        if (!params || params.threadId !== threadId) return;
        if (hydrated) apply(params);
        else buffered.push(params);
      },
    );

    client
      .threadTranscript({ threadId })
      .then((result) => {
        if (cancelled) return;
        setStream((current) => {
          let next = hydrate(result, current);
          for (const params of buffered) {
            next = applyAcpEvent(next, params.acp, params.transcriptSeq);
          }
          return next;
        });
        hydrated = true;
        buffered.length = 0;
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        // A thread the host cannot replay still has a live stream: keep the
        // subscription, say what went wrong, and let the turn draw itself.
        setError(err instanceof Error ? err.message : String(err));
        hydrated = true;
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [client, threadId]);

  const dispatch = useCallback(
    (text: string, mode: PromptMode) => {
      if (!client || !threadId) return;
      const busy = streamRef.current.busy;
      setStream((current) =>
        busy ? markPromptQueued(current, text) : markPromptSent(current),
      );
      client
        .prompt({ threadId, content: text, mode })
        .then((result) => {
          // The host is the authority on whether it queued or sent: our `busy`
          // can be one event stale, and a bubble that claims the agent was
          // told something it was not is the one lie this surface cannot tell.
          setStream((current) => {
            if (result.queued && !busy) return markPromptQueued(current, text);
            if (!result.queued && busy) {
              return {
                ...markPromptSent(current),
                queued: current.queued.filter((held) => held !== text),
              };
            }
            return current;
          });
        })
        .catch((err: unknown) => {
          setStream((current) => ({
            ...current,
            queued: current.queued.filter((held) => held !== text),
            busy: busy ? current.busy : false,
          }));
          setError(err instanceof Error ? err.message : String(err));
        });
    },
    [client, threadId],
  );

  const send = useCallback(
    // Always `queue`, never the default `reject`: the host refuses a bare
    // second prompt (#15) and a refusal is not something to show a user who
    // just typed a sentence at a thread that happens to be thinking.
    (text: string) => dispatch(text, "queue"),
    [dispatch],
  );

  const cancel = useCallback(() => {
    if (!client || !threadId) return;
    client.cancel({ threadId }).catch((err: unknown) => {
      setError(err instanceof Error ? err.message : String(err));
    });
  }, [client, threadId]);

  return useMemo(
    () => ({ stream, error, loading, send, cancel }),
    [stream, error, loading, send, cancel],
  );
}
