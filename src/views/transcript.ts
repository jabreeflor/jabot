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
  PendingPermissionView,
  PermissionAskParams,
  PermissionResolvedParams,
  PromptMode,
  QueuedPromptView,
  SessionUpdateParams,
  ThreadTranscriptResult,
  TranscriptEventView,
} from "../host";
import { PERMISSION_ASK, PERMISSION_RESOLVED, SESSION_UPDATE } from "../host";
import type { StatusTone, ThreadStatus } from "../components/status";
import type {
  NoticeAction,
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
 * `busy` is "a turn is in flight", and it is fed from both ends: the events
 * raise it (anything the agent says, and any prompt the host dispatches, means
 * a turn is running — whoever started it) and a stop reason lowers it. It is
 * seeded once, from the run ledger the replay arrives with, because the events
 * alone cannot describe a turn that began before this view existed.
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
  /** Index of each permission card by `requestId` (#20). Keyed on the request
      rather than on position because the same ask reaches this reducer twice —
      once from `permission/pending` on hydrate, once from the live
      `permission/ask` — and two cards for one question is two questions. */
  permissions: Readonly<Record<string, number>>;
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
  permissions: {},
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
  // The ledger decides `busy`, not the replay, and the replayed events' own
  // reading of it is discarded here. A transcript that ends in an agent chunk
  // looks exactly the same whether the agent is still typing or the host died
  // under it a week ago; only the run says which. `from.busy` still wins,
  // because a prompt sent while this read was in flight started a turn the
  // host had not yet opened a run for when it answered.
  const busy = from.busy || isOpenRun(result.runState);
  // A replay over a run that has already ended holds no live bubble, however
  // the rows end. A thread whose host died mid-sentence replays exactly like
  // one still being written, and the caret would blink over it forever —
  // which is the same lie a stop reason exists to stop telling.
  const settled = busy ? stream : closeBubble(stream);
  stream = {
    ...settled,
    headSeq: head,
    hydratedSeq: Math.max(settled.hydratedSeq, result.headSeq),
    busy,
    // A turn in flight has no outcome yet, and the replay's last stop reason
    // belongs to the turn before it.
    lastStopReason: busy ? null : settled.lastStopReason,
  };
  return withQueued(stream, result.queued);
}

/** `runState` is reported only while the run is open, so any value means yes. */
function isOpenRun(runState: ThreadTranscriptResult["runState"]): boolean {
  return runState !== undefined && runState !== null;
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
    // The host writes this at *dispatch* and never at accept, so it is the
    // start of a turn whether we asked for it or another window did — and when
    // it is the queue draining, it is also the head of the queue leaving.
    case "user_message_chunk": {
      const started = turnInFlight(next);
      return chunk(
        promptDispatched(update) ? shiftQueued(started) : started,
        "user",
        blockText(update.content),
      );
    }
    case "agent_message_chunk":
      return chunk(turnInFlight(next), "agent", blockText(update.content));
    // Reasoning is not the transcript. ACP has a `think` tool kind for the
    // work an agent chooses to show; a raw thought stream would double the
    // length of every chat with text the prototype has no bubble for.
    case "agent_thought_chunk":
      return next;
    case "tool_call":
    case "tool_call_update":
      return toolCall(turnInFlight(next), update);
    case "plan":
      return {
        ...closeBubble(turnInFlight(next)),
        plan: planProgress(update.entries),
      };
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

/**
 * A turn is running, and this event is the evidence.
 *
 * Without this, `busy` is only ever raised by our own `send`, so a turn this
 * view did not start — the queue draining, or anything already in flight when
 * the view mounted — draws no Stop button, streams into a bubble marked
 * `streaming: false`, and leaves the header reporting the *previous* turn's
 * stop reason while the agent is mid-sentence.
 *
 * Returns the same object when nothing changes, because this runs on every
 * chunk and the identity of the stream is what the memoized rows below hang
 * off.
 */
function turnInFlight(stream: ThreadStream): ThreadStream {
  if (stream.busy && stream.lastStopReason === null) return stream;
  return { ...stream, busy: true, lastStopReason: null };
}

/**
 * One prompt left the host's queue, so drop the head of our mirror of it.
 *
 * The head and not a text match: the queue is FIFO on both sides, and two
 * identical follow-ups are two entries that have to clear one at a time. If
 * the mirror is somehow already out of step, shrinking it by one still gets
 * the count right, and the next `thread/transcript` replaces it wholesale.
 */
function shiftQueued(stream: ThreadStream): ThreadStream {
  if (stream.queued.length === 0) return stream;
  return { ...stream, queued: stream.queued.slice(1) };
}

/** The host's marker for "this bubble is the queue's head being sent". */
function promptDispatched(update: Record<string, unknown>): boolean {
  const jabot = asRecord(update.jabot);
  return jabot !== undefined && str(jabot.event) === "prompt_dispatched";
}

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

/**
 * A turn began: the composer is busy and the queue can start filling.
 *
 * The optimistic half of the same rule [`turnInFlight`] applies to events —
 * the composer must not wait a round trip to offer Stop.
 */
export function markPromptSent(stream: ThreadStream): ThreadStream {
  return turnInFlight(stream);
}

/** A prompt the host is holding, shown before it has been sent to anyone. */
export function markPromptQueued(
  stream: ThreadStream,
  text: string,
): ThreadStream {
  return { ...stream, queued: [...stream.queued, text] };
}

// ---- permission cards (#20) -----------------------------------------------

/** The prefix that makes a notice card's id reversible back to its request. */
const PERMISSION_ITEM = "perm-";

/**
 * The action id for "no" on a card whose agent offered no options at all.
 *
 * Never mixed in with the agent's own option ids — it is namespaced so an
 * adapter cannot accidentally ship an option that means cancel to us and
 * something else to itself.
 */
export const PERMISSION_CANCEL = "jabot:cancel";

export function permissionItemId(requestId: string): string {
  return `${PERMISSION_ITEM}${requestId}`;
}

/** The request a notice card belongs to, or `null` for any other notice. */
export function permissionRequestId(itemId: string): string | null {
  return itemId.startsWith(PERMISSION_ITEM)
    ? itemId.slice(PERMISSION_ITEM.length)
    : null;
}

/**
 * An agent is asking. Draw the card once, whichever way the ask arrived.
 *
 * A request already on screen is left exactly as it is — the same object, so
 * nothing re-renders. An ask does not change after it is made, and the live
 * notification and the hydrated `permission/pending` row are two views of one
 * question rather than two questions.
 */
export function applyPermissionAsk(
  stream: ThreadStream,
  ask: {
    requestId: string;
    threadId?: string;
    subject: unknown;
    options: unknown;
    stale?: boolean;
  },
): ThreadStream {
  if (!ask.requestId) return stream;
  if (stream.permissions[ask.requestId] !== undefined) return stream;
  const closed = closeBubble(stream);
  return {
    ...closed,
    items: [...closed.items, permissionNotice(ask)],
    permissions: {
      ...closed.permissions,
      [ask.requestId]: closed.items.length,
    },
  };
}

/**
 * Somebody answered: this window, another window, or the host cancelling the
 * turn. The card locks either way — the buttons are the only thing that could
 * send a second answer, and the host is not the only one who can resolve it.
 */
export function applyPermissionResolved(
  stream: ThreadStream,
  requestId: string,
): ThreadStream {
  const at = stream.permissions[requestId];
  if (at === undefined) return stream;
  const item = stream.items[at];
  if (!item || item.kind !== "notice" || item.resolved) return stream;
  return {
    ...stream,
    items: replaceAt(stream.items, at, { ...item, resolved: true }),
  };
}

/** Every ask the host is still holding for this thread, oldest first. */
export function hydratePermissions(
  stream: ThreadStream,
  requests: readonly PendingPermissionView[],
): ThreadStream {
  return requests.reduce(
    (acc: ThreadStream, request: PendingPermissionView) =>
      applyPermissionAsk(acc, request),
    stream,
  );
}

/**
 * Say out loud that an answer was recorded but never reached anyone.
 *
 * The alternative is a card that fades on click exactly as it would have if
 * the agent had acted on it, over a session that is not going to do anything.
 */
export function noteUndelivered(stream: ThreadStream): ThreadStream {
  return {
    ...stream,
    items: [
      ...stream.items,
      {
        kind: "sys",
        id: nextId(stream),
        text: "Recorded. The agent that asked is gone — message the thread to pick the work back up.",
      },
    ],
    counter: stream.counter + 1,
  };
}

function permissionNotice(ask: {
  requestId: string;
  threadId?: string;
  subject: unknown;
  options: unknown;
  stale?: boolean;
}): Extract<TranscriptItem, { kind: "notice" }> {
  const subject = asRecord(ask.subject);
  const toolCall = subject ? asRecord(subject.toolCall) : undefined;
  const detail = toolCall ?? subject;
  return {
    kind: "notice",
    id: permissionItemId(ask.requestId),
    title: (detail && str(detail.title)) ?? "Permission needed",
    pill: detail ? str(detail.kind) : undefined,
    body: permissionBody(detail, ask.stale === true),
    actions: permissionActions(ask.options),
    threadId: ask.threadId,
  };
}

function permissionBody(
  detail: Record<string, unknown> | undefined,
  stale: boolean,
): string {
  const what = detail ? (permissionTarget(detail) ?? "") : "";
  const base = what
    ? `The agent wants to go ahead with ${what}.`
    : "The agent is asking before it goes ahead.";
  if (!stale) return base;
  // The honest half. #21 brings this thread back as Needs you after a restart;
  // what it cannot bring back is the ACP call, so a click here records the
  // decision and reaches nobody.
  return `${base} JaBot restarted while it was waiting, so your answer is recorded rather than delivered.`;
}

/**
 * What the card says the agent is about to do.
 *
 * The *opposite* priority to a tool line's target: a toolblock leads with the
 * agent's own title because it is a log of what happened, while this is a
 * decision, and "Run ls" over a command of `rm -rf /` is the summary hiding
 * the thing being agreed to. The concrete argument wins; the title is the
 * fallback, and it is already the heading anyway.
 */
function permissionTarget(
  detail: Record<string, unknown>,
): string | undefined {
  const raw = asRecord(detail.rawInput);
  const argument = raw
    ? (str(raw.command) ??
      str(raw.path) ??
      str(raw.file_path) ??
      str(raw.query))
    : undefined;
  if (argument) return argument;
  const locations = Array.isArray(detail.locations) ? detail.locations : [];
  const first = asRecord(locations[0]);
  return (first ? str(first.path) : undefined) ?? str(detail.title);
}

/**
 * The agent's own options, in the agent's own words.
 *
 * Nothing is invented: an option the adapter did not offer is one the host
 * cannot send back. The only exception is an ask with no options at all, which
 * would otherwise be a question with no way to answer it.
 */
function permissionActions(options: unknown): NoticeAction[] {
  const offered = Array.isArray(options) ? options : [];
  const actions: NoticeAction[] = [];
  for (const raw of offered) {
    const option = asRecord(raw);
    const id = option ? str(option.optionId) : undefined;
    if (!id) continue;
    const kind = option ? (str(option.kind) ?? "") : "";
    actions.push({
      id,
      label: (option && str(option.name)) ?? id,
      primary: kind.startsWith("allow") || id.startsWith("allow"),
    });
  }
  if (actions.length === 0) {
    actions.push({ id: PERMISSION_CANCEL, label: "Cancel" });
  }
  return actions;
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
  /**
   * Answer a permission card: the notice item's id, and the id of the button
   * that was pressed — which is one of the agent's own ACP option ids, or
   * [`PERMISSION_CANCEL`] (#20).
   */
  answer: (itemId: string, actionId: string) => void;
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
    // Permission notifications are buffered with the updates and for the same
    // reason: the subscription is installed before either read goes out, and
    // an ask that arrives in between belongs on the screen, not in the gap.
    const buffered: LiveEvent[] = [];

    const apply = (event: LiveEvent) => {
      setStream((current) => applyLive(current, event));
    };

    const unsubscribe = client.onNotification(
      (notification: JsonRpcNotification) => {
        const event = liveEvent(notification, threadId);
        if (!event) return;
        if (hydrated) apply(event);
        else buffered.push(event);
      },
    );

    let pendingError: unknown = null;
    Promise.all([
      client.threadTranscript({ threadId }),
      // An ask outlives the host that took it (#20), so the outstanding ones
      // are read at the same time as the transcript rather than waited for:
      // a thread reopened after a quit has to show the question again.
      client.pendingPermissions({ threadId }).catch((err: unknown) => {
        pendingError = err;
        return { requests: [] as PendingPermissionView[] };
      }),
    ])
      .then(([result, pending]) => {
        if (cancelled) return;
        setStream((current) => {
          let next = hydrate(result, current);
          next = hydratePermissions(next, pending.requests);
          for (const event of buffered) next = applyLive(next, event);
          return next;
        });
        hydrated = true;
        buffered.length = 0;
        if (pendingError) setError(message(pendingError));
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        // A thread the host cannot replay still has a live stream: keep the
        // subscription, say what went wrong, and let the turn draw itself.
        setError(message(err));
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
          setError(message(err));
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
      setError(message(err));
    });
  }, [client, threadId]);

  const answer = useCallback(
    (itemId: string, actionId: string) => {
      const requestId = permissionRequestId(itemId);
      if (!client || !threadId || !requestId) return;
      const deviceId = client.deviceId;
      if (!deviceId) {
        setError("Not connected to the host yet.");
        return;
      }
      // Optimistic, and load-bearing: locking the card *is* what stops a
      // second click becoming a second answer while the first is in flight.
      // The host is idempotent underneath (#20) — this is the half that keeps
      // the user from having to find that out.
      setStream((current) => applyPermissionResolved(current, requestId));
      client
        .replyPermission({
          requestId,
          deviceId,
          ...(actionId === PERMISSION_CANCEL
            ? { cancelled: true }
            : { optionId: actionId }),
        })
        .then((result) => {
          if (!result.delivered) {
            setStream((current) => noteUndelivered(current));
          }
        })
        .catch((err: unknown) => setError(message(err)));
    },
    [client, threadId],
  );

  return useMemo(
    () => ({ stream, error, loading, send, cancel, answer }),
    [stream, error, loading, send, cancel, answer],
  );
}

/** One notification, as something the reducer understands — or nothing. */
type LiveEvent =
  | { kind: "update"; params: SessionUpdateParams }
  | { kind: "ask"; params: PermissionAskParams }
  | { kind: "resolved"; params: PermissionResolvedParams };

function liveEvent(
  notification: JsonRpcNotification,
  threadId: string,
): LiveEvent | null {
  const params = notification.params as
    | { threadId?: string }
    | undefined;
  if (!params || params.threadId !== threadId) return null;
  switch (notification.method) {
    case SESSION_UPDATE:
      return { kind: "update", params: params as SessionUpdateParams };
    case PERMISSION_ASK:
      return { kind: "ask", params: params as PermissionAskParams };
    case PERMISSION_RESOLVED:
      return { kind: "resolved", params: params as PermissionResolvedParams };
    default:
      return null;
  }
}

function applyLive(stream: ThreadStream, event: LiveEvent): ThreadStream {
  switch (event.kind) {
    case "update":
      return applyAcpEvent(stream, event.params.acp, event.params.transcriptSeq);
    case "ask":
      return applyPermissionAsk(stream, {
        requestId: event.params.requestId,
        threadId: event.params.threadId,
        subject: event.params.subject,
        options: event.params.options,
      });
    case "resolved":
      return applyPermissionResolved(stream, event.params.requestId);
  }
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
