/**
 * ACP → chat, and the live thread that draws it (#14).
 *
 * The reducer cases are written against payloads an adapter really sends,
 * including the ones no adapter should: an unknown tool kind, an unknown
 * `sessionUpdate`, a payload that is not an object. Those are the interesting
 * tests — a transcript that dies on an unfamiliar enum is a chat that dies on
 * its first unfamiliar tool, and #11's review found that class of bug once
 * already.
 *
 * The identity assertions look fussy and are the streaming budget: appending a
 * chunk has to leave every other item the same object, or the memoized rows
 * below re-render the whole conversation on every token.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Transcript } from "../components/Transcript";
import type {
  HandoffView,
  HostClient,
  ProcessView,
  JsonRpcNotification,
  PendingPermissionView,
  PermissionReplyParams,
  PromptParams,
  ThreadTranscriptResult,
} from "../host";
import { PERMISSION_ASK, PERMISSION_RESOLVED, SESSION_UPDATE } from "../host";
import { LiveThreadView } from "../views/ThreadView";
import {
  EMPTY_STREAM,
  applyAcpEvent,
  diffStat,
  hydrate,
  markPromptQueued,
  markPromptSent,
  streamStatus,
  type ThreadStream,
} from "../views/transcript";
import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";

function feed(payloads: unknown[], from: ThreadStream = EMPTY_STREAM): ThreadStream {
  return payloads.reduce<ThreadStream>(
    (stream, payload) => applyAcpEvent(stream, payload),
    from,
  );
}

const last = (items: readonly TranscriptItem[]) => items[items.length - 1];

const text = (value: string) => ({
  sessionUpdate: "agent_message_chunk",
  content: { type: "text", text: value },
});

describe("ACP → transcript", () => {
  it("streams agent chunks into one bubble", () => {
    const stream = feed([text("Reading "), text("the guard"), text(".")]);
    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toMatchObject({
      kind: "agent",
      text: "Reading the guard.",
    });
  });

  it("keeps every untouched item the same object while a chunk streams", () => {
    const before = feed([
      { sessionUpdate: "user_message_chunk", content: { type: "text", text: "go" } },
      {
        sessionUpdate: "tool_call",
        toolCallId: "c1",
        kind: "read",
        title: "src/auth.ts",
        status: "completed",
      },
      text("Reading "),
    ]);
    const after = applyAcpEvent(before, text("the guard."));

    expect(after.items).not.toBe(before.items);
    expect(after.items[0]).toBe(before.items[0]);
    expect(after.items[1]).toBe(before.items[1]);
    expect(after.items[2]).not.toBe(before.items[2]);
  });

  it("updates a tool call in place rather than stacking a second line", () => {
    const before = feed([
      {
        sessionUpdate: "tool_call",
        toolCallId: "c1",
        kind: "execute",
        title: "npm test",
        status: "pending",
      },
      text("working"),
    ]);
    const after = applyAcpEvent(before, {
      sessionUpdate: "tool_call_update",
      toolCallId: "c1",
      status: "completed",
    });

    expect(after.items).toHaveLength(before.items.length);
    expect(after.items[0]).toMatchObject({
      kind: "tool",
      call: { target: "npm test", kind: "execute", status: "completed" },
    });
    // The bubble beside it is untouched — one status flip, one re-render.
    expect(after.items[1]).toBe(before.items[1]);
  });

  it("creates the line for an update whose tool call it never saw", () => {
    // What a client that connected mid-turn, or a `session/load` replay, sends.
    const stream = feed([
      {
        sessionUpdate: "tool_call_update",
        toolCallId: "c9",
        kind: "edit",
        title: "src/auth.ts",
        status: "completed",
      },
    ]);
    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toMatchObject({ call: { kind: "edit" } });
  });

  it("survives an unknown tool kind, an unknown status, and an unknown update", () => {
    const stream = feed([
      {
        sessionUpdate: "tool_call",
        toolCallId: "c1",
        kind: "sorcery",
        title: "summon",
        status: "levitating",
      },
      { sessionUpdate: "quantum_update", payload: 7 },
      "not an object at all",
      null,
      { noSessionUpdateAtAll: true },
    ]);

    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toMatchObject({
      call: { kind: "other", status: "in_progress" },
    });

    // And it renders: the verb falls back rather than throwing on `padEnd`.
    const { container } = render(<Transcript items={stream.items} />);
    expect(container.querySelector(".verb")?.textContent).toContain("tool");
    expect(container.querySelector(".call")?.textContent).toContain("summon");
  });

  it("turns a diff into the prototype's +/− note", () => {
    const stream = feed([
      {
        sessionUpdate: "tool_call",
        toolCallId: "c1",
        kind: "edit",
        title: "src/auth.ts",
        status: "completed",
        content: [
          {
            type: "diff",
            path: "/repo/src/auth.ts",
            oldText: "a\ngone\n",
            newText: "a\nadded\nalso\n",
          },
        ],
      },
    ]);
    expect(stream.items[0]).toMatchObject({ call: { note: "+2 −1" } });
  });

  it("counts a git patch exactly when the adapter sends one", () => {
    expect(
      diffStat({
        gitPatch: "--- a/x\n+++ b/x\n@@\n-old\n+new\n+more\n context\n",
      }),
    ).toEqual({ added: 2, removed: 1 });
  });

  it("reads a plan as the header's step counter", () => {
    const stream = markPromptSent(
      feed([
        {
          sessionUpdate: "plan",
          entries: [
            { content: "Read", status: "completed" },
            { content: "Patch", status: "in_progress" },
            { content: "Test", status: "pending" },
          ],
        },
      ]),
    );
    expect(stream.plan).toEqual({ done: 1, total: 3, current: "Patch" });
    expect(streamStatus(stream, { label: "idle", tone: "quiet" })).toEqual({
      label: "running · step 2/3",
      tone: "running",
    });
  });

  it("ends the turn on a stop reason and says which one", () => {
    const running = markPromptSent(feed([text("done here")]));
    expect(running.busy).toBe(true);

    const ended = applyAcpEvent(running, {
      sessionUpdate: "state_update",
      sessionState: "idle",
      stopReason: "max_tokens",
    });
    expect(ended.busy).toBe(false);
    expect(ended.lastStopReason).toBe("max_tokens");
    expect(last(ended.items)).toMatchObject({ kind: "sys" });
    expect(streamStatus(ended, { label: "idle", tone: "quiet" })).toEqual({
      label: "stopped: out of tokens",
      tone: "bad",
    });
    // The bubble stops claiming to be streaming, or it renders mid-type
    // forever after the agent has stopped talking.
    expect(ended.items[0]).toMatchObject({ kind: "agent", streaming: false });
  });

  /**
   * Idle with no stop reason is a v2 adapter reporting it went quiet. #15
   * (D-006) is explicit that idleness alone is not an outcome, and the chat
   * must not announce one either.
   */
  it("does not call an idle report an outcome", () => {
    const ended = applyAcpEvent(markPromptSent(feed([text("hi")])), {
      sessionUpdate: "state_update",
      sessionState: "idle",
    });
    expect(ended.lastStopReason).toBeNull();
    expect(ended.items.every((item) => item.kind !== "sys")).toBe(true);
  });

  /**
   * The turn the renderer did not start. `busy` used to be raised only by our
   * own `send`, so a queue drain — or anything already running when the view
   * mounted — drew no Stop button, streamed into a bubble marked
   * `streaming: false`, and left the header reporting the *previous* turn's
   * outcome while the agent was mid-sentence.
   */
  it("treats fresh agent output after a stop reason as a new turn", () => {
    const ended = applyAcpEvent(markPromptSent(feed([text("first turn")])), {
      sessionUpdate: "state_update",
      sessionState: "idle",
      stopReason: "end_turn",
    });
    expect(streamStatus(ended, { label: "idle", tone: "quiet" }).label).toBe("done");

    const again = applyAcpEvent(ended, text("actually, one more thing"));
    expect(again.busy).toBe(true);
    expect(again.lastStopReason).toBeNull();
    expect(streamStatus(again, { label: "idle", tone: "quiet" })).toEqual({
      label: "running",
      tone: "running",
    });
    expect(last(again.items)).toMatchObject({ kind: "agent", streaming: true });
  });

  /** A tool call is agent output too, and so is a plan. */
  it("reopens the turn on a tool call the renderer did not ask for", () => {
    const idle = applyAcpEvent(EMPTY_STREAM, {
      sessionUpdate: "state_update",
      sessionState: "idle",
      stopReason: "end_turn",
    });
    const working = applyAcpEvent(idle, {
      sessionUpdate: "tool_call",
      toolCallId: "c1",
      kind: "execute",
      title: "npm test",
      status: "in_progress",
    });
    expect(working.busy).toBe(true);
    expect(working.lastStopReason).toBeNull();
  });

  /**
   * The host drains its queue as an ordinary `user_message_chunk`, so without
   * a marker the "N messages waiting" strip has nothing to shrink on: it
   * stayed up over a prompt that had in fact been delivered, and its "Send
   * now" button then cancelled whatever unrelated turn was in flight.
   */
  it("shortens the waiting strip when the host dispatches a queued prompt", () => {
    const waiting = markPromptQueued(
      markPromptQueued(EMPTY_STREAM, "first follow-up"),
      "second follow-up",
    );
    expect(waiting.queued).toEqual(["first follow-up", "second follow-up"]);

    const sent = applyAcpEvent(waiting, {
      sessionUpdate: "user_message_chunk",
      content: { type: "text", text: "first follow-up" },
      jabot: { event: "prompt_dispatched" },
    });
    expect(sent.queued).toEqual(["second follow-up"]);
    expect(sent.busy).toBe(true);
    expect(last(sent.items)).toMatchObject({
      kind: "user",
      text: "first follow-up",
    });

    // An unmarked prompt is someone typing, not the queue moving.
    const typed = applyAcpEvent(sent, {
      sessionUpdate: "user_message_chunk",
      content: { type: "text", text: "and one more" },
    });
    expect(typed.queued).toEqual(["second follow-up"]);
  });

  it("says so when a queued prompt is never going to be sent", () => {
    const stream = applyAcpEvent(EMPTY_STREAM, {
      sessionUpdate: "state_update",
      jabot: {
        event: "prompt_dropped",
        reason: "the adapter stopped",
        content: "and also fix the tests",
      },
    });
    expect(last(stream.items)).toMatchObject({
      kind: "sys",
      text: expect.stringContaining("and also fix the tests") as unknown as string,
    });
  });
});

describe("hydrate", () => {
  /**
   * The claim the whole overlay rests on: a replay of our rows and the live
   * stream produce the same chat, and an event that is in both is applied
   * once. The host stamps a `session/update` with the `seq` of the row it
   * wrote, which is what makes the second half exact rather than a guess.
   */
  it("replays rows and then ignores the live copies of the same events", () => {
    const events = [
      { seq: 1, method: SESSION_UPDATE, createdAt: "", payload: { sessionUpdate: "user_message_chunk", content: { type: "text", text: "go" } } },
      { seq: 2, method: SESSION_UPDATE, createdAt: "", payload: text("on it") },
    ];
    const hydrated = hydrate({
      threadId: "t1",
      headSeq: 2,
      events,
      truncated: false,
      queued: [],
      // What the real host reports for a thread whose turn is still going —
      // which is the only situation in which live copies of replayed events
      // can still be arriving.
      runState: "running",
    });
    expect(hydrated.items).toHaveLength(2);
    expect(hydrated.headSeq).toBe(2);

    // The same event arriving as a notification while we were hydrating.
    const again = applyAcpEvent(hydrated, events[1].payload, 2);
    expect(again).toBe(hydrated);

    const newer = applyAcpEvent(hydrated, text(" — done."), 3);
    expect(newer.items).toHaveLength(2);
    expect(newer.items[1]).toMatchObject({ text: "on it — done." });
  });

  it("takes the log's head even when the window left rows behind", () => {
    const hydrated = hydrate({
      threadId: "t1",
      headSeq: 900,
      events: [
        { seq: 899, method: SESSION_UPDATE, createdAt: "", payload: text("tail") },
        { seq: 900, method: SESSION_UPDATE, createdAt: "", payload: text(" end") },
      ],
      truncated: true,
      queued: [{ position: 1, content: "and then deploy", queuedAt: "" }],
    });
    expect(hydrated.headSeq).toBe(900);
    expect(hydrated.queued).toEqual(["and then deploy"]);
  });

  /**
   * The replay cannot answer "is this turn still going?" — the last row of a
   * live turn and the last row of one whose host died under it are the same
   * row. So the run ledger comes with it, and it is the ledger that decides.
   */
  it("seeds the turn in flight from the run the replay arrives with", () => {
    const events = [
      { seq: 1, method: SESSION_UPDATE, createdAt: "", payload: text("half a sen") },
    ];
    const mid = hydrate({
      threadId: "t1",
      headSeq: 1,
      events,
      truncated: false,
      queued: [],
      runState: "running",
    });
    expect(mid.busy).toBe(true);
    expect(streamStatus(mid, { label: "idle", tone: "quiet" }).label).toBe("running");
    expect(mid.items[0]).toMatchObject({ kind: "agent", streaming: true });
  });

  /**
   * The same rows over a run that has ended. The bubble must stop claiming to
   * be mid-type, or a thread the host died under blinks a caret forever.
   */
  it("does not call a dead turn a running one", () => {
    const dead = hydrate({
      threadId: "t1",
      headSeq: 2,
      events: [
        { seq: 1, method: SESSION_UPDATE, createdAt: "", payload: { sessionUpdate: "user_message_chunk", content: { type: "text", text: "go" } } },
        { seq: 2, method: SESSION_UPDATE, createdAt: "", payload: text("half a sen") },
      ],
      truncated: false,
      queued: [],
    });
    expect(dead.busy).toBe(false);
    expect(dead.items[1]).toMatchObject({ kind: "agent", streaming: false });
  });

  /**
   * A prompt sent while the read was in flight started a turn the host had
   * not opened a run for when it answered. The optimistic `busy` wins, or the
   * Stop button the user is looking at disappears under them.
   */
  it("does not undo a turn started while it was loading", () => {
    const sending = markPromptSent(EMPTY_STREAM);
    const hydrated = hydrate(
      { threadId: "t1", headSeq: 0, events: [], truncated: false, queued: [] },
      sending,
    );
    expect(hydrated.busy).toBe(true);
  });
});

// ---- the live view --------------------------------------------------------

const THREAD: ThreadSummary = {
  id: "t-auth",
  folderId: "f1",
  botId: null,
  harnessId: "claude",
  title: "Auth migration",
  state: "active",
  foldPolicy: "default",
  runState: null,
};

const HARNESSES: HarnessCard[] = [
  { id: "claude", label: "Claude Code", blurb: "", accent: "var(--h-claude)" },
];

const HOST: HostTarget = { hostId: "h1", name: "This Mac", reachable: true };

/**
 * A stand-in host that behaves like the real one on the points this view
 * depends on: it persists before it answers (so a prompt shows up in the next
 * transcript read), it refuses nothing that it queued, and its permission
 * broker resolves each request exactly once — a second answer comes back
 * saying what the first one decided rather than reaching the agent twice
 * (#20). A mock that answered twice would let a bug through that the real host
 * refuses to have.
 */
function stubHost(
  replay: Partial<ThreadTranscriptResult> = {},
  pending: PendingPermissionView[] = [],
  handoff?: HandoffView,
  process?: Partial<ProcessView>,
  /** Whatever else `thread/state` should answer with — the worktree fields
      (#23) are read from the same call the handoff and the drift are. */
  state: Record<string, unknown> = {},
) {
  const handlers = new Set<(n: JsonRpcNotification) => void>();
  const prompts: PromptParams[] = [];
  const cancel = vi.fn(async () => {});
  let busy = false;
  const resolved = new Map<string, { optionId?: string; delivered: boolean }>();
  const stale = new Set(
    pending.filter((request) => request.stale).map((r) => r.requestId),
  );

  const client = {
    onNotification: (handler: (n: JsonRpcNotification) => void) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    threadTranscript: vi.fn(async () => ({
      threadId: THREAD.id,
      headSeq: 1,
      events: [
        {
          seq: 1,
          method: SESSION_UPDATE,
          createdAt: "",
          payload: {
            sessionUpdate: "user_message_chunk",
            content: { type: "text", text: "start the migration" },
          },
        },
      ],
      truncated: false,
      queued: [],
      // Like the host: an open run is reported while there is one, and a
      // thread whose last turn ended reports nothing at all.
      ...replay,
    })),
    deviceId: "dev-1",
    threadState: vi.fn(async () => ({
      threadId: THREAD.id,
      handoff,
      process,
      ...state,
    })),
    pendingPermissions: vi.fn(async () => ({
      requests: pending.filter((request) => !resolved.has(request.requestId)),
    })),
    replyPermission: vi.fn(async (params: PermissionReplyParams) => {
      const already = resolved.get(params.requestId);
      if (already) {
        return {
          requestId: params.requestId,
          delivered: already.delivered,
          alreadyAnswered: true,
          optionId: already.optionId,
          cancelled: already.optionId === undefined,
        };
      }
      // An ask whose adapter is gone is answerable and undeliverable — the
      // whole reason the record outlives the process.
      const delivered = !stale.has(params.requestId);
      resolved.set(params.requestId, { optionId: params.optionId, delivered });
      notify(PERMISSION_RESOLVED, {
        hostId: "h1",
        threadId: THREAD.id,
        seq: 99,
        requestId: params.requestId,
        deviceId: params.deviceId,
        optionId: params.optionId,
        cancelled: params.cancelled,
      });
      return {
        requestId: params.requestId,
        delivered,
        alreadyAnswered: false,
        optionId: params.optionId,
        cancelled: params.cancelled === true,
      };
    }),
    prompt: vi.fn(async (params: PromptParams) => {
      prompts.push(params);
      const queued = busy;
      busy = true;
      return {
        threadId: THREAD.id,
        acpSessionId: "sess-1",
        accepted: !queued,
        queued,
        queuePosition: queued ? 1 : undefined,
      };
    }),
    cancel,
  } as unknown as HostClient;

  function notify(method: string, params: unknown) {
    act(() => {
      for (const handler of handlers) {
        handler({ jsonrpc: "2.0", method, params });
      }
    });
  }

  return {
    client,
    prompts,
    cancel,
    replyPermission: client.replyPermission,
    setBusy: (value: boolean) => {
      busy = value;
    },
    /** Somebody else answered — another window, or the turn being cancelled. */
    resolve(requestId: string) {
      resolved.set(requestId, { optionId: undefined, delivered: true });
      notify(PERMISSION_RESOLVED, {
        hostId: "h1",
        threadId: THREAD.id,
        seq: 98,
        requestId,
        deviceId: "dev-2",
        cancelled: true,
      });
    },
    /** The host's `permission/ask`, as the broker emits it. */
    ask(requestId: string, subject: unknown, options: unknown) {
      notify(PERMISSION_ASK, {
        hostId: "h1",
        threadId: THREAD.id,
        seq: 42,
        requestId,
        subject,
        options,
      });
    },
    emit(acp: unknown, transcriptSeq: number) {
      notify(SESSION_UPDATE, {
        hostId: "h1",
        threadId: THREAD.id,
        seq: transcriptSeq,
        transcriptSeq,
        acp,
      });
    },
  };
}

describe("LiveThreadView", () => {
  it("hydrates from the store and then follows the live stream", async () => {
    const host = stubHost();
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );

    // From SQLite, not from any harness log.
    expect(await screen.findByText("start the migration")).toBeInTheDocument();

    host.emit(text("Rewriting the middleware"), 2);
    expect(await screen.findByText("Rewriting the middleware")).toBeInTheDocument();

    host.emit(
      {
        sessionUpdate: "tool_call",
        toolCallId: "c1",
        kind: "execute",
        title: "npm test",
        status: "in_progress",
      },
      3,
    );
    expect(await screen.findByText(/npm test/)).toBeInTheDocument();
  });

  /**
   * Steer vs redispatch, from the user's side. ACP cannot inject a message
   * into a running turn, so a follow-up is *queued* — and the UI has to say
   * so, because "we took your text and sent it" and "we took your text and
   * are holding it" look identical otherwise.
   */
  it("queues a follow-up sent during a turn and offers to interrupt", async () => {
    const host = stubHost();
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    await screen.findByText("start the migration");

    const box = screen.getByRole("textbox");
    await userEvent.type(box, "use the new guard{Enter}");
    await waitFor(() => expect(host.prompts).toHaveLength(1));
    expect(host.prompts[0]).toMatchObject({
      threadId: THREAD.id,
      content: "use the new guard",
      // Queue, never reject: the host refuses an unqualified second prompt and
      // a refusal is not something to show a user who just typed a sentence.
      mode: "queue",
    });

    await userEvent.type(box, "and roll it back if the tests fail{Enter}");
    expect(
      await screen.findByText("1 message waiting"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("and roll it back if the tests fail"),
    ).toBeInTheDocument();

    // "Send now" ends the turn in flight, which is what lets the queue go.
    await userEvent.click(screen.getByRole("button", { name: "Send now" }));
    expect(host.cancel).toHaveBeenCalledWith({ threadId: THREAD.id });

    // And then the turn really ends and the host sends what it was holding.
    // The strip has to come down here: left up, it describes a message that
    // has already been delivered, and its "Send now" button goes on cancelling
    // turns that have nothing to do with it.
    host.emit(
      { sessionUpdate: "state_update", sessionState: "idle", stopReason: "cancelled" },
      2,
    );
    host.emit(
      {
        sessionUpdate: "user_message_chunk",
        content: { type: "text", text: "and roll it back if the tests fail" },
        jabot: { event: "prompt_dispatched" },
      },
      3,
    );

    await waitFor(() =>
      expect(screen.queryByText("1 message waiting")).toBeNull(),
    );
    // It started a turn, so the header says so and Stop is back.
    expect(screen.getByText("running")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
  });

  /**
   * Reopening a thread remounts this hook with an empty stream, so nothing in
   * the events can say a turn is already in flight — the replay ends the same
   * way whether the agent is still typing or stopped last week. The run the
   * transcript arrives with is what makes Stop reachable at all here.
   */
  it("offers Stop for a turn that was already running when it mounted", async () => {
    const host = stubHost({
      headSeq: 3,
      runState: "running",
      events: [
        {
          seq: 1,
          method: SESSION_UPDATE,
          createdAt: "",
          payload: {
            sessionUpdate: "user_message_chunk",
            content: { type: "text", text: "start the migration" },
          },
        },
        {
          seq: 2,
          method: SESSION_UPDATE,
          createdAt: "",
          payload: {
            sessionUpdate: "state_update",
            sessionState: "idle",
            stopReason: "end_turn",
          },
        },
        { seq: 3, method: SESSION_UPDATE, createdAt: "", payload: text("on the guard now") },
      ],
    });
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    await screen.findByText("on the guard now");

    // Not "done" — that was the turn before this one.
    expect(screen.getByText("running")).toBeInTheDocument();
    const stop = screen.getByRole("button", { name: "Stop" });
    await userEvent.click(stop);
    expect(host.cancel).toHaveBeenCalledWith({ threadId: THREAD.id });
  });

  it("offers Stop while a turn is running and not before", async () => {
    const host = stubHost();
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    await screen.findByText("start the migration");
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();

    await userEvent.type(screen.getByRole("textbox"), "go{Enter}");
    const stop = await screen.findByRole("button", { name: "Stop" });
    await userEvent.click(stop);
    expect(host.cancel).toHaveBeenCalledWith({ threadId: THREAD.id });

    host.emit(
      { sessionUpdate: "state_update", sessionState: "idle", stopReason: "cancelled" },
      2,
    );
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Stop" })).toBeNull(),
    );
    expect(screen.getByText("cancelled")).toBeInTheDocument();
  });
});

// ---- permission cards (#20) -----------------------------------------------

const EXECUTE = {
  toolCallId: "call-1",
  title: "Run ls",
  kind: "execute",
  rawInput: { command: "ls -la" },
};

const OPTIONS = [
  { optionId: "allow_once", name: "Allow", kind: "allow_once" },
  { optionId: "reject_once", name: "Deny", kind: "reject_once" },
];

const askedBefore = (overrides: Partial<PendingPermissionView> = {}): PendingPermissionView => ({
  requestId: "req-1",
  threadId: THREAD.id,
  title: "Run ls",
  kind: "execute",
  subject: EXECUTE,
  options: OPTIONS,
  createdAt: "2026-08-20T09:00:00.000Z",
  stale: true,
  ...overrides,
});

function renderThread(host: ReturnType<typeof stubHost>) {
  render(
    <LiveThreadView
      client={host.client}
      thread={THREAD}
      harnesses={HARNESSES}
      host={HOST}
    />,
  );
}

describe("permission prompts", () => {
  it("draws the agent's own options and sends back the one that was pressed", async () => {
    const host = stubHost();
    renderThread(host);
    await screen.findByText("start the migration");

    host.ask("req-1", EXECUTE, OPTIONS);

    expect(await screen.findByText("Run ls")).toBeInTheDocument();
    // The command, not a paraphrase: what the user is agreeing to has to be
    // on the card.
    expect(screen.getByText(/ls -la/)).toBeInTheDocument();
    expect(screen.getByText("execute")).toBeInTheDocument();

    const allow = screen.getByRole("button", { name: "Allow" });
    // And nothing the agent did not offer.
    expect(screen.getByRole("button", { name: "Deny" })).toBeInTheDocument();
    await userEvent.click(allow);

    expect(host.replyPermission).toHaveBeenCalledWith({
      requestId: "req-1",
      deviceId: "dev-1",
      optionId: "allow_once",
    });
    // Answered means answered: the buttons stop being a way to send a second
    // decision the moment the first one is in flight.
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Allow" })).toBeDisabled(),
    );
  });

  it("does not answer twice when the card is clicked twice", async () => {
    const host = stubHost();
    renderThread(host);
    await screen.findByText("start the migration");
    host.ask("req-1", EXECUTE, OPTIONS);
    const allow = await screen.findByRole("button", { name: "Allow" });

    await userEvent.click(allow);
    // The second click lands on the same button before anything re-renders it
    // away. Nothing may reach the host a second time.
    await userEvent.click(allow);

    expect(host.replyPermission).toHaveBeenCalledTimes(1);
  });

  it("locks the card when the answer came from somewhere else", async () => {
    const host = stubHost();
    renderThread(host);
    await screen.findByText("start the migration");
    host.ask("req-1", EXECUTE, OPTIONS);
    await screen.findByRole("button", { name: "Allow" });

    // Another window, another device, or the turn being cancelled under it —
    // the host says so with `permission/resolved` and this card is history.
    act(() => {
      host.resolve("req-1");
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Allow" })).toBeDisabled(),
    );
    expect(host.replyPermission).not.toHaveBeenCalled();
  });

  it("asks again after a restart, and says the answer is only recorded", async () => {
    const host = stubHost({}, [askedBefore()]);
    renderThread(host);

    // Nothing in the transcript replay carries this: the ask is a row in the
    // broker's ledger, and reopening the thread is what puts it back on screen.
    expect(await screen.findByText("Run ls")).toBeInTheDocument();
    expect(screen.getByText(/JaBot restarted/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Allow" }));

    expect(host.replyPermission).toHaveBeenCalledWith({
      requestId: "req-1",
      deviceId: "dev-1",
      optionId: "allow_once",
    });
    // The agent that asked is gone, and the card must not pretend otherwise.
    expect(
      await screen.findByText(/message the thread to pick the work back up/),
    ).toBeInTheDocument();
  });

  it("draws one card when the live ask repeats the one it hydrated", async () => {
    const host = stubHost({}, [askedBefore({ stale: false })]);
    renderThread(host);
    await screen.findByText("Run ls");

    // The same request arriving both ways is one question, not two.
    host.ask("req-1", EXECUTE, OPTIONS);

    await waitFor(() =>
      expect(screen.getAllByRole("button", { name: "Allow" })).toHaveLength(1),
    );
  });
});

/**
 * Provenance: who asked for this thread, when it was not the person reading
 * it.
 *
 * A thread Chief spawned looks exactly like one the human started — same
 * header, same transcript — and the human coming back tomorrow has no way to
 * tell which. The host has resolved this all along and nothing drew it.
 */
describe("LiveThreadView provenance", () => {
  const handoff = (over: Partial<HandoffView> = {}): HandoffView => ({
    handoffId: "h-1",
    kind: "handoff",
    task: "Chase the failing migration test",
    fromBotId: "chief",
    fromBotName: "Chief",
    dispatched: true,
    createdAt: "2026-08-20T10:00:00Z",
    ...over,
  });

  function draw(over: Partial<HandoffView> | null) {
    const host = stubHost({}, [], over === null ? undefined : handoff(over));
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    return host;
  }

  it("says which bot handed the work over, and what it asked for", async () => {
    draw({});
    expect(await screen.findByText(/Handed off by/)).toBeInTheDocument();
    expect(screen.getByText(/Chief/)).toBeInTheDocument();
    expect(
      screen.getByText(/Chase the failing migration test/),
    ).toBeInTheDocument();
  });

  /** A spawned coding thread is a different sentence: Chief did not hand this
      to a colleague, it opened a job. */
  it("calls a spawned code thread a coding job", async () => {
    draw({ kind: "code_session" });
    expect(await screen.findByText(/Coding job from/)).toBeInTheDocument();
  });

  /**
   * The case worth a different tone. A handoff to a bot whose harness is not
   * installed is still a real handoff — the task was sent, the thread is here
   * — but nobody heard it, and a line saying only "Handed off by Chief" would
   * be describing work that is not happening.
   */
  it("says so when the handoff never reached an agent", async () => {
    draw({ dispatched: false, detail: "Writer's harness is not installed" });
    expect(
      await screen.findByText(/Writer's harness is not installed/),
    ).toBeInTheDocument();
    const line = document.querySelector(".chat-handoff");
    expect(line).toHaveAttribute("data-tone", "warn");
  });

  /** The ordinary case is the person starting their own thread. Nothing to
      say, and a header that said "started by you" would be noise on every
      thread in the app. */
  it("draws nothing for a thread the human started", async () => {
    draw(null);
    await screen.findByText("start the migration");
    expect(document.querySelector(".chat-handoff")).toBeNull();
  });

  /** A host too old to answer `thread/state` costs the caption and nothing
      else — the conversation is entirely readable without it. */
  it("still renders the chat when the host cannot answer", async () => {
    const host = stubHost();
    const client = { ...host.client, threadState: undefined } as unknown as HostClient;
    render(
      <LiveThreadView
        client={client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    expect(await screen.findByText("start the migration")).toBeInTheDocument();
    expect(document.querySelector(".chat-handoff")).toBeNull();
  });
});

/**
 * Receipt drift: the stored session no longer matches the job that would be
 * spawned, so the next prompt starts a fresh conversation.
 *
 * The host has computed this on every `thread/state` since #21 — it is what
 * `resume_readiness` is for — and it is the one thing on this screen a user
 * cannot possibly infer. Everything looks normal: the transcript is there, the
 * composer works, and the next message silently opens a new conversation the
 * agent has no memory of.
 */
describe("LiveThreadView drift", () => {
  function draw(drift?: string[]) {
    const host = stubHost({}, [], undefined, {
      connected: false,
      acpState: "idle",
      pendingPermissions: 0,
      resumable: false,
      drift,
    });
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    return host;
  }

  it("names the fields that moved, in the words the rest of the UI uses", async () => {
    draw(["harnessId", "cwd"]);

    // Queried by container, not by the phrase: the phrase lives in a <b>
    // inside the notice, and the field names sit beside it.
    await screen.findByText(/setup has changed/);
    const notice = document.querySelector(".chat-drift");
    // Reads as a sentence, not a comma-joined fragment.
    expect(notice).toHaveTextContent("the engine and the folder are not");
    // The wire names themselves are not what a user reads.
    expect(notice).not.toHaveTextContent("harnessId");
  });

  it("joins three moved fields into a sentence", async () => {
    draw(["harnessId", "model", "cwd"]);

    await screen.findByText(/setup has changed/);
    expect(document.querySelector(".chat-drift")).toHaveTextContent(
      "the engine, the model and the folder are not",
    );
  });

  it("says what the next message will actually do", async () => {
    draw(["model"]);

    expect(
      await screen.findByText(/next message begins a new one/),
    ).toBeInTheDocument();
  });

  /** A field the host learns to report before this list does is still worth
      naming — printing it raw beats dropping it silently. */
  it("prints an unknown field rather than swallowing it", async () => {
    draw(["someNewField"]);

    expect(await screen.findByText(/someNewField/)).toBeInTheDocument();
  });

  /** The overwhelmingly common case. A banner on every thread would train the
      user to ignore the one that matters. */
  it("draws nothing when nothing has drifted", async () => {
    draw([]);
    await screen.findByText("start the migration");

    expect(document.querySelector(".chat-drift")).toBeNull();
  });

  it("draws nothing when the host reports no drift field at all", async () => {
    draw(undefined);
    await screen.findByText("start the migration");

    expect(document.querySelector(".chat-drift")).toBeNull();
  });
});

/**
 * Where a code thread is actually editing (#23).
 *
 * A thread opened in a git folder runs in a host-owned worktree under the app
 * data directory on a `jabot/<id>` branch, not in the user's checkout. The
 * host has stamped both onto the thread row since #23 and served them on
 * `thread/state`; nothing drew them, so someone looking at a running thread
 * could not tell which directory or which branch the agent was changing.
 */
describe("LiveThreadView, where the work is happening", () => {
  function draw(state: Record<string, unknown>) {
    const host = stubHost({}, [], undefined, undefined, state);
    render(
      <LiveThreadView
        client={host.client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );
    return host;
  }

  it("names the branch, and keeps the whole path for the tooltip", async () => {
    draw({
      worktreePath: "/Users/j/Library/Application Support/jabot/worktrees/t-1",
      branch: "jabot/t-1",
    });

    const chip = await screen.findByTitle(
      "/Users/j/Library/Application Support/jabot/worktrees/t-1",
    );
    // The branch is the visible half: it is what identifies the work, and the
    // path is long, machine-generated and the same prefix on every thread.
    expect(chip).toHaveTextContent("jabot/t-1");
  });

  /** A tree the host recorded without a branch is still somewhere else, and
      saying so with the directory beats saying nothing. */
  it("falls back to the tree's own directory when there is no branch", async () => {
    draw({ worktreePath: "/data/jabot/worktrees/t-9" });

    expect(
      await screen.findByTitle("/data/jabot/worktrees/t-9"),
    ).toHaveTextContent("t-9");
  });

  /**
   * The common case for everything that is not a code thread in a checkout: a
   * bot's standing thread, a non-git folder, the "use my own checkout"
   * opt-out. A chip on all of those would say nothing about any of them.
   */
  it("draws nothing for a thread that works in place", async () => {
    draw({ branch: "main" });

    await screen.findByText("start the migration");
    expect(document.querySelector(".worktree-chip")).toBeNull();
  });

  /** Same trade as the handoff caption: a host that will not answer costs the
      chip and nothing else. */
  it("costs nothing on a host that cannot answer thread/state", async () => {
    const host = stubHost();
    const client = { ...host.client, threadState: undefined } as unknown as HostClient;
    render(
      <LiveThreadView
        client={client}
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
      />,
    );

    expect(await screen.findByText("start the migration")).toBeInTheDocument();
    expect(document.querySelector(".worktree-chip")).toBeNull();
  });
});
