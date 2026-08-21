/**
 * End-to-end: the session supervisor (#21) over the wire.
 *
 * `src-tauri/tests/supervisor.rs` drives `HostSession` in-process, where a
 * test can pump the adapter itself and reach into the store. This file makes
 * the durability claims the way the product does — a `jabot-hostd` process is
 * killed and a new one is started on the same data directory, and the question
 * is what the production `HostClient` is told afterwards.
 *
 * Stopping the host really does end its adapters (`shutdown_adapters` on the
 * way out), so "restart" here is Cmd-Q and relaunch, not a reconnect.
 */
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  RPC_ERROR,
  SUPERVISOR_STATUS,
  THREAD_RESUME,
  type InboxListResult,
  type ThreadStateResult,
} from "../../src/host/protocol";
import { fakeAcpRuntime, HostdProcess, type HostdOptions } from "../support/hostd";

/** The copy `state-machine.md` specifies. Hard-coded so that changing the
    sentence in the host fails here rather than silently changing what a user
    reads on the launch after a quit. */
const WAS_WAITING_ON_YOU = "the agent was waiting on you; reopen to continue";

const running: HostdProcess[] = [];
const dataDirs: string[] = [];

async function connected(options: HostdOptions = { persistent: true }) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

/** A data dir this file owns, so a host can be stopped and started on it. */
function ownDataDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-supervisor-"));
  dataDirs.push(dir);
  return dir;
}

async function openThread(
  client: HostClient,
  threadId: string,
  mode?: string,
  cwd = tmpdir(),
) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd,
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

async function settle(
  client: HostClient,
  threadId: string,
  predicate: (state: ThreadStateResult) => boolean,
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

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("boot reconciliation", () => {
  it("advertises the supervisor methods it implements", async () => {
    const { hello } = await connected();
    // The drift guard: a method the TS client can call has to be one the Rust
    // host admits to, or the two halves of the protocol have already parted.
    for (const method of [THREAD_RESUME, SUPERVISOR_STATUS]) {
      expect(hello.methods).toContain(method);
    }
  });

  it("resurfaces a thread we quit on with the ask still outstanding", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-perm", "permission");
    await first.client.fold({ threadId: "t-perm" });
    await first.client.prompt({ threadId: "t-perm", content: "rm -rf" });
    await settle(first.client, "t-perm", (s) => s.state === "resurfaced");
    // Before the quit, the card is about the request the agent made.
    expect((await first.client.inbox()).events[0].summary).toBe("Run ls");
    await first.host.stop();

    const second = await connected({ dataDir });
    const state = await second.client.threadState({ threadId: "t-perm" });

    // The permission RPC died with the process, so the run is `lost` — the
    // ledger's word for "we stopped being able to find out". What the human
    // is told is a separate axis, and it is the sentence the state machine
    // specifies rather than a stale description of a live request.
    expect(state.latestRun?.state).toBe("lost");
    expect(state.process.connected).toBe(false);
    const inbox = await second.client.inbox();
    expect(kinds(inbox)).toEqual(["needs_you"]);
    expect(inbox.events[0].summary).toBe(WAS_WAITING_ON_YOU);
    expect(inbox.unread).toBe(1);

    const status = await second.client.supervisorStatus();
    expect(status.liveAdapters).toEqual([]);
    expect(status.boot).toMatchObject([
      { threadId: "t-perm", was: "needs_you", now: "lost" },
    ]);
  });

  it("reports a turn the restart interrupted as stuck, not failed", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-hang", "hang");
    await first.client.prompt({ threadId: "t-hang", content: "hi" });
    await settle(first.client, "t-hang", (s) => s.latestRun?.state === "running");
    await first.client.fold({ threadId: "t-hang" });
    await first.host.stop();

    const second = await connected({ dataDir });
    const state = await second.client.threadState({ threadId: "t-hang" });

    // `failed` invites a retry of work we have no evidence went wrong. The
    // ask is to reopen, and the conversation is still there to reopen into.
    expect(state.state).toBe("resurfaced");
    expect(state.resurfacedReason).toBe("stuck");
    expect(state.latestRun?.state).toBe("lost");
    expect(state.latestRun?.error).toBe("interrupted by restart");
    expect(kinds(await second.client.inbox())).toEqual(["stuck"]);
  });
});

describe("resume", () => {
  it("resumes the stored session instead of orphaning the conversation", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-resume", "resumable");
    await first.client.prompt({ threadId: "t-resume", content: "hi" });
    await settle(first.client, "t-resume", (s) => s.latestRun?.state === "succeeded");
    await first.host.stop();

    const second = await connected({ dataDir });
    const cold = await second.client.threadState({ threadId: "t-resume" });
    expect(cold.process.connected).toBe(false);
    expect(cold.process.resumable).toBe(true);

    const resumed = await second.client.resumeThread({ threadId: "t-resume" });
    expect(resumed.outcome).toBe("resumed");
    expect(resumed.acpSessionId).toBe("sess-fake-1");
    expect(resumed.state.process.connected).toBe(true);

    // The adapter's own side of the wire is the only place `session/resume`
    // and `session/new` are distinguishable, so that is where it is asserted.
    const log = readFileSync(
      path.join(dataDir, "adapter-logs", "t-resume.stderr.log"),
      "utf8",
    );
    expect(log).toContain("session_resume=");
    expect(log).not.toContain("session_new=");
  });

  it("refuses to resume a session whose job has changed", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-drift", "resumable");
    await first.client.prompt({ threadId: "t-drift", content: "hi" });
    await settle(first.client, "t-drift", (s) => s.latestRun?.state === "succeeded");
    // Wait for Inbox is a different permission mode from the one the receipt
    // was stamped under (#15's fingerprint).
    await first.client.fold({ threadId: "t-drift", policy: "wait_for_inbox" });
    await first.host.stop();

    const second = await connected({ dataDir });
    const state = await second.client.threadState({ threadId: "t-drift" });
    expect(state.process.resumable).toBe(false);
    expect(state.process.drift).toEqual(["permissionMode"]);

    const refused = await second.client.resumeThread({ threadId: "t-drift" });
    expect(refused.outcome).toBe("drifted");
    expect(refused.resumed).toBe(false);
    expect(refused.drift).toEqual(["permissionMode"]);
    // Nothing was spawned to hold a session it must not have been given.
    expect((await second.client.supervisorStatus()).liveAdapters).toEqual([]);
  });

  it("refuses a prompt into a folder that is gone rather than starting over", async () => {
    const dataDir = ownDataDir();
    const cwd = path.join(dataDir, "checkout");
    mkdirSync(cwd);
    const first = await connected({ dataDir });
    await openThread(first.client, "t-gone", "resumable", cwd);
    await first.client.prompt({ threadId: "t-gone", content: "hi" });
    await settle(first.client, "t-gone", (s) => s.latestRun?.state === "succeeded");
    await first.client.fold({ threadId: "t-gone" });
    await first.host.stop();

    rmSync(cwd, { recursive: true, force: true });

    // Reopening the thread and typing is the path a user actually takes after
    // a restart, and it has to refuse for the same reason `thread/resume`
    // does: an adapter spawned here inherits JaBot's own directory, and the
    // `session/new` that follows overwrites the receipt.
    const second = await connected({ dataDir });
    const failure = await second.client
      .prompt({ threadId: "t-gone", content: "carry on" })
      .catch((err: unknown) => err);
    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.CWD_MISSING);

    const state = await second.client.threadState({ threadId: "t-gone" });
    // Still the conversation that is really there, so the thread comes back
    // the moment the checkout does.
    expect(state.acpSessionId).toBe("sess-fake-1");
    expect(state.resurfacedReason).toBe("failed");
    expect((await second.client.supervisorStatus()).liveAdapters).toEqual([]);
  });

  it("says so plainly when the adapter can neither resume nor load", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-plain");
    await first.client.prompt({ threadId: "t-plain", content: "hi" });
    await settle(first.client, "t-plain", (s) => s.latestRun?.state === "succeeded");
    await first.host.stop();

    const second = await connected({ dataDir });
    const answer = await second.client.resumeThread({ threadId: "t-plain" });
    expect(answer.outcome).toBe("unsupported");
    expect(answer.resumed).toBe(false);
    expect(answer.detail).toBeTruthy();
    // And the user is not stuck with it: prompting starts an honest new one.
    await second.client.prompt({ threadId: "t-plain", content: "again" });
    const state = await settle(
      second.client,
      "t-plain",
      (s) => s.latestRun?.seq === 2 && s.latestRun?.state === "succeeded",
    );
    expect(state.acpSessionId).toBe("sess-fake-1");
  });
});

describe("keep-alive", () => {
  it("notices an adapter that died without closing its stdout", async () => {
    const { client } = await connected();
    await openThread(client, "t-orphan", "orphan-stdout");
    await client.prompt({ threadId: "t-orphan", content: "hi" });

    // The adapter exits while a grandchild holds the same stdout, so the read
    // loop never sees EOF. Only reaping the pid can tell the host, and without
    // that this thread reports a live session for as long as the app runs.
    const state = await settle(client, "t-orphan", (s) => !s.process.connected);
    expect(state.latestRun?.state).toBe("failed");
    expect(state.latestRun?.error).toBe("the adapter process exited");
    expect((await client.supervisorStatus()).liveAdapters).toEqual([]);
  });

  it("reports the live adapter it is holding for a folded, working thread", async () => {
    const { client } = await connected();
    await openThread(client, "t-live", "hang");
    await client.prompt({ threadId: "t-live", content: "hi" });
    await settle(client, "t-live", (s) => s.latestRun?.state === "running");
    await client.fold({ threadId: "t-live" });

    // Folded and still working: the process is kept, which is the whole
    // feature. `session/close` here would be a cancel wearing a tidier name.
    const status = await client.supervisorStatus();
    expect(status.liveAdapters).toHaveLength(1);
    expect(status.liveAdapters[0]).toMatchObject({
      threadId: "t-live",
      acpSessionId: "sess-fake-1",
      // Thread-scoped harnesses key on the thread, which is what says out loud
      // that two Claude chats could never have shared one process (#13).
      profileKey: "claude:t-live",
    });
    expect(status.liveAdapters[0].pid).toBeGreaterThan(0);
    expect((await client.threadState({ threadId: "t-live" })).process.pid).toBe(
      status.liveAdapters[0].pid,
    );
  });
});
