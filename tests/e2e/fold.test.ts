/**
 * End-to-end: Fold and Wait for Inbox on a session that is actually running (#26).
 *
 * Every other lifecycle case folds a thread that is idle, or one whose turn has
 * already ended. Those prove the transition table. They do not prove the
 * product's premise, which is a claim about a *live* subprocess: the row leaves
 * the sidebar, the agent keeps working, the Inbox says it is still asleep, and
 * the thread comes back on its own with the right card when the work it was
 * already doing finishes, fails, or asks for something the host may not answer.
 *
 * So every case here folds mid-turn. The fake agent's `gated` mode holds the
 * turn open until the test writes a gate file, which turns "fold while it is
 * still running" from a race against a sleep into an ordering the test
 * controls. The client is the production `HostClient`, the host is a real
 * `jabot-hostd` with a real SQLite store, and the agent is a real ACP
 * subprocess — nothing here is stubbed.
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  INBOX_RESURFACE,
  type FoldPolicy,
  type InboxListResult,
  type InboxResurfaceParams,
  type JsonRpcNotification,
  type ThreadStateResult,
} from "../../src/host/protocol";
import {
  gatedAcpRuntime,
  HostdProcess,
  openGate,
  type HostdOptions,
} from "../support/hostd";

const running: HostdProcess[] = [];

async function connected(options: HostdOptions = { persistent: true }) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  await client.hello();
  return { host, client };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

const kinds = (inbox: InboxListResult) => inbox.events.map((event) => event.kind);

/** Poll `thread/state` until the host has settled where the test expects. */
async function settle(
  client: HostClient,
  threadId: string,
  predicate: (state: ThreadStateResult) => boolean,
  timeoutMs = 15_000,
): Promise<ThreadStateResult> {
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

/**
 * A registered folder with one thread in it, prompted, and left mid-turn.
 *
 * The folder is the point: "the row leaves the sidebar" is a claim about
 * `folder/list`, and a thread with no folder is in nobody's sidebar to begin
 * with. It is deliberately not a git checkout — a worktree would be a second
 * moving part in a test about the overlay.
 */
async function liveThread(client: HostClient, threadId: string) {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-fold-"));
  const gate = path.join(dir, `${threadId}.gate`);
  const folder = await client.registerFolder({ path: dir, name: "globnet-sync" });
  await client.openThread({
    threadId,
    title: "Auth migration",
    cwd: folder.cwd,
    folderId: folder.folderId,
    harnessId: "claude",
    runtime: gatedAcpRuntime(gate),
  });
  await client.prompt({ threadId, content: "migrate the auth middleware" });

  // Fold has to land on a turn that is genuinely in flight; folding an idle
  // thread and watching it stay idle would prove none of this.
  const state = await settle(
    client,
    threadId,
    (s) => s.latestRun?.state === "running" && s.process.acpState === "running",
  );
  return { folder, gate, state };
}

/** The sidebar's own question: is this row in the folder it belongs to? */
async function inSidebar(client: HostClient, folderId: string, threadId: string) {
  const { folders } = await client.listFolders();
  const folder = folders.find((row) => row.folderId === folderId);
  return (folder?.threads ?? []).some((thread) => thread.threadId === threadId);
}

async function fold(client: HostClient, threadId: string, policy?: FoldPolicy) {
  return client.fold(policy ? { threadId, policy } : { threadId });
}

describe("folding a live session", () => {
  it("hides the row, keeps the adapter working, and brings it back when it finishes", async () => {
    const { host, client } = await connected();
    const { folder, gate, state } = await liveThread(client, "t-live");

    expect(state.state).toBe("active");
    expect(await inSidebar(client, folder.folderId, "t-live")).toBe(true);

    const folded = await fold(client, "t-live");
    expect(folded.state).toBe("folded");
    // Visibility only. Same process, same run, same turn: the overlay moved
    // and nothing else did.
    expect(folded.process.connected).toBe(true);
    expect(folded.process.acpState).toBe("running");
    expect(folded.latestRun?.state).toBe("running");
    expect(folded.latestRun?.id).toBe(state.latestRun?.id);

    // The row is out of the sidebar — the whole point of folding it.
    expect(await inSidebar(client, folder.folderId, "t-live")).toBe(false);

    // And the Inbox says it is asleep and still working, without a card:
    // folding is not news, so Still Sleeping is the thread row, not an event.
    const asleep = await client.inbox();
    expect(kinds(asleep)).toEqual([]);
    expect(asleep.sleeping).toHaveLength(1);
    expect(asleep.sleeping[0]).toMatchObject({
      threadId: "t-live",
      title: "Auth migration",
      foldPolicy: "default",
      runState: "running",
      acpState: "running",
    });
    expect(asleep.unread).toBe(0);

    // Ask for the Inbox the instant the host says the thread came back — the
    // same race a renderer runs. Persist-then-notify is what makes it safe.
    const readOnNotify = host
      .waitFor(
        (n: JsonRpcNotification) =>
          n.method === INBOX_RESURFACE &&
          (n.params as InboxResurfaceParams).threadId === "t-live",
      )
      .then(() => client.inbox());

    // The work that was already under way finishes with nobody watching.
    openGate(gate, "end_turn");

    const inbox = await readOnNotify;
    expect(kinds(inbox)).toEqual(["done"]);
    expect(inbox.events[0]).toMatchObject({
      threadId: "t-live",
      title: "Auth migration finished",
      runId: state.latestRun?.id,
    });
    expect(inbox.sleeping).toEqual([]);

    const done = await client.threadState({ threadId: "t-live" });
    expect(done.state).toBe("resurfaced");
    expect(done.resurfacedReason).toBe("done");
    expect(done.latestRun?.state).toBe("succeeded");
    // Back in the sidebar, because a card the user has to act on needs a row
    // to open.
    expect(await inSidebar(client, folder.folderId, "t-live")).toBe(true);
  });

  it("comes back failed, not stuck, when the live turn ends badly", async () => {
    const { client } = await connected();
    const { gate } = await liveThread(client, "t-live-fail");
    await fold(client, "t-live-fail");

    openGate(gate, "max_tokens");
    const state = await settle(client, "t-live-fail", (s) => s.state === "resurfaced");

    // Failed and stuck are different asks of the human — a failure wants a
    // retry, silence wants patience — so a turn that really ended must never
    // be filed as one that merely went quiet.
    expect(state.resurfacedReason).toBe("failed");
    expect(state.latestRun?.state).toBe("failed");
    expect(state.lastStopReason).toBe("max_tokens");
    expect(state.process.acpState).toBe("idle");
    expect(kinds(await client.inbox())).toEqual(["failed"]);
  });

  it("reports a live folded session that goes quiet as stuck, with the process still up", async () => {
    // The backstop is a real timeout; this shortens it so the test can watch
    // it fire. Until the gate opens the agent says nothing at all, so the card
    // can only come from the silence — no other signal is on its way.
    const { client } = await connected({
      persistent: true,
      env: { JABOT_IDLE_TIMEOUT_MS: "150" },
    });
    const { gate } = await liveThread(client, "t-live-quiet");
    await fold(client, "t-live-quiet");

    const stuck = await settle(client, "t-live-quiet", (s) => s.state === "resurfaced");
    expect(stuck.resurfacedReason).toBe("stuck");
    // Stuck keeps everything alive: the run is still open and so is the
    // adapter, so waiting is a real option and the work is not thrown away.
    expect(stuck.latestRun?.state).toBe("running");
    expect(stuck.process.connected).toBe(true);
    expect(kinds(await client.inbox())).toEqual(["stuck"]);

    // It was slow, not wedged. Once the turn really ends the card has to
    // become the outcome, or finished work sits under "has gone quiet".
    openGate(gate, "end_turn");
    const done = await settle(
      client,
      "t-live-quiet",
      (s) => s.latestRun?.state === "succeeded",
    );
    expect(done.resurfacedReason).toBe("done");
    expect(kinds(await client.inbox())).toEqual(["done"]);
  });
});

describe("Wait for Inbox on a live session", () => {
  it("answers a read while it sleeps and still asks before a delete", async () => {
    const { client } = await connected();
    const { gate } = await liveThread(client, "t-live-policy");

    // The user changes their mind about a turn that is already in flight. The
    // policy is written before the fold precisely so an ask arriving in the
    // same breath is judged by the policy they just chose.
    const folded = await fold(client, "t-live-policy", "wait_for_inbox");
    expect(folded.state).toBe("folded");
    expect(folded.foldPolicy).toBe("wait_for_inbox");

    // Two asks in one turn: the read the host may answer for them, and the
    // delete it may not. Locked policy — folding never auto-allows a
    // destructive tool, however quiet the user asked for it to be.
    openGate(gate, "read,delete");

    const state = await settle(client, "t-live-policy", (s) => s.state === "resurfaced");
    expect(state.resurfacedReason).toBe("needs_you");
    expect(state.process.pendingPermissions).toBe(1);
    expect(state.latestRun?.state).toBe("needs_you");

    const inbox = await client.inbox();
    expect(kinds(inbox)).toContain("needs_you");
    const away = inbox.events.find((event) => event.kind === "judgment_call");
    expect(away?.title).toBe("Allowed Read src/auth.ts");
    // A receipt, not something still owed: only the delete is unread.
    expect(away?.readAt).toBeTruthy();
    expect(inbox.unread).toBe(1);
  });

  it("asks for everything once the thread is awake again", async () => {
    const { client } = await connected();
    const { gate } = await liveThread(client, "t-live-awake");
    await fold(client, "t-live-awake", "wait_for_inbox");
    await client.reopenThread({ threadId: "t-live-awake" });

    // Wait for Inbox is a policy for a thread nobody is looking at. The
    // moment the user is back in the chat, the read is theirs to answer —
    // auto-allowing it here would answer a question in front of them.
    openGate(gate, "read");
    const state = await settle(
      client,
      "t-live-awake",
      (s) => s.process.pendingPermissions === 1,
    );
    expect(state.state).toBe("active");
    expect(state.foldPolicy).toBe("wait_for_inbox");
    expect(
      (await client.inbox()).events.some((event) => event.kind === "judgment_call"),
    ).toBe(false);
  });
});
