/**
 * End-to-end: the thread state machine and run ledger (#15) over the wire.
 *
 * `src-tauri/tests/lifecycle.rs` drives `HostSession` in-process, where a test
 * can pump the adapter itself. This file makes the same claims from the client
 * side — the production `HostClient`, a `jabot-hostd` process, a real SQLite
 * store, and a real ACP subprocess under it — so "the lifecycle works" means
 * the transitions, the ledger and the Inbox survive the protocol, not that a
 * Rust helper was polled.
 *
 * The ordering case is the one worth reading twice: it waits for the
 * `inbox/resurface` notification and then asks the host for the Inbox, which
 * is exactly what a renderer does. The card has to already be there.
 */
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  INBOX_RESURFACE,
  RPC_ERROR,
  THREAD_FOLD,
  THREAD_OPEN,
  THREAD_STATE,
  type InboxListResult,
  type InboxResurfaceParams,
  type JsonRpcNotification,
} from "../../src/host/protocol";
import { fakeAcpRuntime, HostdProcess, type HostdOptions } from "../support/hostd";

const running: HostdProcess[] = [];

async function connected(options: HostdOptions = { persistent: true }) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

/** A store-backed thread whose harness is the fake ACP agent. */
async function openThread(client: HostClient, threadId: string, mode?: string) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

/** Poll `thread/state` until the host has settled where the test expects. */
async function settle(
  client: HostClient,
  threadId: string,
  predicate: (state: Awaited<ReturnType<HostClient["threadState"]>>) => boolean,
  timeoutMs = 15_000,
) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const state = await client.threadState({ threadId });
    if (predicate(state)) return state;
    if (Date.now() > deadline) {
      throw new Error(`${threadId} never settled; last state: ${JSON.stringify(state)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
}

const kinds = (inbox: InboxListResult) => inbox.events.map((event) => event.kind);

describe("overlay transitions", () => {
  it("advertises every lifecycle method it implements", async () => {
    const { hello } = await connected();

    // The drift guard: a method the TS client can call must be one the host
    // admits to, or the renderer and the host have already diverged.
    for (const method of [THREAD_OPEN, THREAD_FOLD, THREAD_STATE]) {
      expect(hello.methods).toContain(method);
    }
  });

  it("walks active → folded → active and keeps the policy on disk", async () => {
    const { client } = await connected();
    const opened = await openThread(client, "t-walk");
    expect(opened.state).toBe("active");

    const folded = await client.fold({
      threadId: "t-walk",
      policy: "wait_for_inbox",
    });
    expect(folded.state).toBe("folded");
    expect(folded.foldPolicy).toBe("wait_for_inbox");

    // Folding writes no card: Still Sleeping is the thread row, not an event.
    const inbox = await client.inbox();
    expect(kinds(inbox)).toEqual([]);
    expect(inbox.sleeping.map((row) => row.threadId)).toEqual(["t-walk"]);

    const reopened = await client.reopenThread({ threadId: "t-walk" });
    expect(reopened.state).toBe("active");
    expect(reopened.foldPolicy).toBe("wait_for_inbox");
  });

  it("refuses an illegal transition instead of quietly doing nothing", async () => {
    const { client } = await connected();
    await openThread(client, "t-illegal");
    await client.fold({ threadId: "t-illegal" });

    const failure = await client
      .fold({ threadId: "t-illegal" })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.ILLEGAL_TRANSITION);
    expect((failure as HostRpcError).data).toMatchObject({
      from: "folded",
      action: "fold",
    });
    // A refused transition must leave the thread exactly where it was.
    expect((await client.threadState({ threadId: "t-illegal" })).state).toBe("folded");
  });

  it("tombstones a deleted thread and takes its Inbox cards with it", async () => {
    const { client } = await connected();
    await openThread(client, "t-gone");
    await client.prompt({ threadId: "t-gone", content: "hi" });
    await settle(client, "t-gone", (s) => s.latestRun?.state === "succeeded");
    await client.fold({ threadId: "t-gone" });
    expect(kinds(await client.inbox())).toEqual(["done"]);

    const deleted = await client.deleteThread({ threadId: "t-gone" });

    expect(deleted.state).toBe("deleted");
    expect(deleted.deletedAt).toBeTruthy();
    expect(kinds(await client.inbox())).toEqual([]);
  });
});

describe("run ledger", () => {
  it("opens a run on prompt and closes it on the stop reason", async () => {
    const { client } = await connected();
    await openThread(client, "t-run");
    await client.prompt({ threadId: "t-run", content: "hi" });

    const state = await settle(client, "t-run", (s) => s.latestRun?.state === "succeeded");
    expect(state.latestRun).toMatchObject({
      seq: 1,
      kind: "prompt",
      state: "succeeded",
      acpSessionId: "sess-fake-1",
    });
    expect(state.lastStopReason).toBe("end_turn");
    // The process axis is reported next to the overlay, never merged into it.
    expect(state.state).toBe("active");
    expect(state.process.acpState).toBe("idle");
    expect(state.process.connected).toBe(true);

    await client.prompt({ threadId: "t-run", content: "again" });
    const second = await settle(client, "t-run", (s) => s.latestRun?.seq === 2);
    // Many sequential runs, one ACP session (#5).
    expect(second.runs).toHaveLength(2);
    expect(second.runs.every((run) => run.acpSessionId === "sess-fake-1")).toBe(true);
  });

  it("refuses a second prompt while the first turn is still in flight", async () => {
    const { client } = await connected();
    await openThread(client, "t-overlap", "permission");
    await client.fold({ threadId: "t-overlap" });
    await client.prompt({ threadId: "t-overlap", content: "rm -rf" });
    await settle(client, "t-overlap", (s) => s.latestRun?.state === "needs_you");

    // ACP runs one turn per session, and the stop reason that comes back names
    // no prompt — so a second run would be handed the first turn's outcome and
    // the run that did the work would be retired holding nothing.
    const failure = await client
      .prompt({ threadId: "t-overlap", content: "and also" })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.RUN_IN_FLIGHT);
    expect((failure as HostRpcError).data).toMatchObject({
      threadId: "t-overlap",
      runState: "needs_you",
    });
    const state = await client.threadState({ threadId: "t-overlap" });
    expect(state.runs).toHaveLength(1);
  });

  it("writes a session receipt a resume can check for drift", async () => {
    const { client } = await connected();
    await openThread(client, "t-receipt");
    await client.prompt({ threadId: "t-receipt", content: "hi" });

    const state = await settle(client, "t-receipt", (s) => Boolean(s.receipt));
    expect(state.receipt).toMatchObject({
      acpSessionId: "sess-fake-1",
      harnessId: "claude",
      cwd: tmpdir(),
      permissionMode: "default",
      tools: [],
    });
    expect(state.receipt?.fingerprint).toMatch(/^[0-9a-f]{16}$/);
  });
});

describe("resurface", () => {
  /**
   * What a renderer gets: the frame arrives, it asks for the Inbox, and the
   * card the frame named is there with the same copy on it.
   *
   * Deliberately *not* claiming to pin persist-then-notify. The host answers
   * one request at a time, so `inbox/list` cannot be served until the
   * `thread/fold` handler has returned and both halves have happened — this
   * race is one a notify-then-persist host would also win, and this case
   * passes against one. The order is pinned where it is observable, in
   * `src-tauri/tests/lifecycle.rs::a_resurface_whose_write_fails_notifies_nobody`,
   * by making the write fail and asserting the silence.
   */
  it("hands a client a card that is already readable, with matching copy", async () => {
    const { host, client } = await connected();
    await openThread(client, "t-order");
    await client.prompt({ threadId: "t-order", content: "hi" });
    await settle(client, "t-order", (s) => s.latestRun?.state === "succeeded");

    // Ask for the Inbox the instant the notification lands — the same race a
    // renderer runs.
    const readOnNotify = new Promise<{
      announced: InboxResurfaceParams;
      inbox: InboxListResult;
    }>((resolve, reject) => {
      host
        .waitFor(
          (n: JsonRpcNotification) =>
            n.method === INBOX_RESURFACE &&
            (n.params as InboxResurfaceParams).threadId === "t-order",
        )
        .then(
          (n) =>
            client.inbox().then(
              (inbox) =>
                resolve({ announced: n.params as InboxResurfaceParams, inbox }),
              reject,
            ),
          reject,
        );
    });

    const folded = await client.fold({ threadId: "t-order" });
    // The agent had already stopped, so this resurfaces rather than sleeping.
    expect(folded.state).toBe("resurfaced");
    expect(folded.resurfacedReason).toBe("done");

    const { announced, inbox } = await readOnNotify;
    expect(kinds(inbox)).toEqual(["done"]);
    expect(inbox.events[0]).toMatchObject({
      threadId: "t-order",
      threadTitle: "Auth migration",
      title: "Auth migration finished",
      runId: folded.latestRun?.id,
    });
    expect(inbox.unread).toBe(1);
    // The frame and the row are one card, not two sources that can disagree.
    expect(announced.reason).toBe(inbox.events[0].kind);
    expect(announced.title).toBe(inbox.events[0].title);
    expect(announced.summary).toBe(inbox.events[0].summary);
  });

  it("comes back as needs_you while a permission is outstanding", async () => {
    const { client } = await connected();
    await openThread(client, "t-perm", "permission");
    await client.fold({ threadId: "t-perm" });
    await client.prompt({ threadId: "t-perm", content: "rm -rf" });

    const state = await settle(client, "t-perm", (s) => s.state === "resurfaced");
    expect(state.resurfacedReason).toBe("needs_you");
    // A paused run, not a finished one — answering resumes this same run.
    expect(state.latestRun?.state).toBe("needs_you");
    expect(state.process).toMatchObject({
      acpState: "requires_action",
      pendingPermissions: 1,
    });
    expect(kinds(await client.inbox())).toEqual(["needs_you"]);
  });

  it("distinguishes stuck from failed", async () => {
    const failed = await connected();
    await openThread(failed.client, "t-failed", "fail");
    await failed.client.fold({ threadId: "t-failed" });
    await failed.client.prompt({ threadId: "t-failed", content: "hi" });
    const failedState = await settle(
      failed.client,
      "t-failed",
      (s) => s.state === "resurfaced",
    );
    expect(failedState.resurfacedReason).toBe("failed");
    expect(failedState.latestRun?.state).toBe("failed");

    // Same overlay state, different reason, different card, and — the part
    // the prototype conflated — a process that is still alive and running.
    const stuck = await connected({
      persistent: true,
      env: { JABOT_IDLE_TIMEOUT_MS: "150" },
    });
    await openThread(stuck.client, "t-stuck", "hang");
    await stuck.client.fold({ threadId: "t-stuck" });
    await stuck.client.prompt({ threadId: "t-stuck", content: "hi" });
    const stuckState = await settle(stuck.client, "t-stuck", (s) => s.state === "resurfaced");
    expect(stuckState.resurfacedReason).toBe("stuck");
    expect(stuckState.latestRun?.state).toBe("running");
    expect(stuckState.process.connected).toBe(true);
    expect(kinds(await stuck.client.inbox())).toEqual(["stuck"]);
  });

  it("answers a read itself under Wait for Inbox and logs it instead of asking", async () => {
    const { client } = await connected();
    await openThread(client, "t-read", "read-permission");
    await client.fold({ threadId: "t-read", policy: "wait_for_inbox" });
    await client.prompt({ threadId: "t-read", content: "hi" });

    // The turn completed without the human, which is only possible if the host
    // answered the read on their behalf.
    const state = await settle(client, "t-read", (s) => s.latestRun?.state === "succeeded");
    expect(state.resurfacedReason).toBe("done");

    const inbox = await client.inbox();
    const away = inbox.events.find((event) => event.kind === "judgment_call");
    expect(away?.title).toBe("Allowed Read src/auth.ts");
    expect(away?.readAt).toBeTruthy();
    expect(inbox.unread).toBe(1);
  });
});
