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
import type { HostClient, JsonRpcNotification, PromptParams } from "../host";
import { SESSION_UPDATE } from "../host";
import { LiveThreadView } from "../views/ThreadView";
import {
  EMPTY_STREAM,
  applyAcpEvent,
  diffStat,
  hydrate,
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
 * A stand-in host that behaves like the real one on the two points this view
 * depends on: it persists before it answers (so a prompt shows up in the next
 * transcript read), and it refuses nothing that it queued.
 */
function stubHost() {
  const handlers = new Set<(n: JsonRpcNotification) => void>();
  const prompts: PromptParams[] = [];
  const cancel = vi.fn(async () => {});
  let busy = false;

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
    })),
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

  return {
    client,
    prompts,
    cancel,
    setBusy: (value: boolean) => {
      busy = value;
    },
    emit(acp: unknown, transcriptSeq: number) {
      act(() => {
        for (const handler of handlers) {
          handler({
            jsonrpc: "2.0",
            method: SESSION_UPDATE,
            params: {
              hostId: "h1",
              threadId: THREAD.id,
              seq: transcriptSeq,
              transcriptSeq,
              acp,
            },
          });
        }
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
