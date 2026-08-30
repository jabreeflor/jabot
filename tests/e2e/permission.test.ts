/**
 * End-to-end: the permission broker (#20) over the wire.
 *
 * `tests/e2e/acp-adapter.test.ts` already drives the happy path — an agent
 * asks, a human answers, the agent is told. What is asserted here is the part
 * that has no adapter to lean on: the ask is a durable record, so quitting
 * with a question on the screen does not throw the question away, and
 * answering is idempotent whether or not anyone is still listening.
 *
 * `src-tauri/tests/permission.rs` makes the same claims in-process. This file
 * makes them through the production `HostClient`, a real `jabot-hostd`, real
 * SQLite and a real ACP subprocess — so "the broker works" means it works
 * across the protocol, not that a Rust helper was polled.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  PERMISSION_ASK,
  PERMISSION_PENDING,
  type InboxListResult,
  type PermissionAskParams,
} from "../../src/host/protocol";
import { fakeAcpRuntime, HostdProcess, type HostdOptions } from "../support/hostd";

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

/** A data dir two hosts share, so one can be quit and the next can look. */
function ownDataDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-permission-"));
  dataDirs.push(dir);
  return dir;
}

async function openThread(client: HostClient, threadId: string, mode: string) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

/** Prompt, and come back with the `requestId` of the ask it produced. */
async function ask(host: HostdProcess, client: HostClient, threadId: string) {
  await client.prompt({ threadId, content: "rm -rf" });
  const asked = (await host.waitFor(
    (n) =>
      n.method === PERMISSION_ASK &&
      (n.params as PermissionAskParams).threadId === threadId,
  )).params as PermissionAskParams;
  return asked.requestId;
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("the permission broker", () => {
  it("advertises the method a client answers outstanding asks through", async () => {
    const { hello } = await connected();
    // The drift guard: a method the TS client can call has to be one the Rust
    // host admits to, or the two halves of the protocol have already parted.
    expect(hello.methods).toContain(PERMISSION_PENDING);
  });

  it("lists a live ask, and answers it exactly once", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-live", "permission");
    const requestId = await ask(host, client, "t-live");

    const pending = await client.pendingPermissions({ threadId: "t-live" });
    expect(pending.requests).toHaveLength(1);
    expect(pending.requests[0]).toMatchObject({
      requestId,
      threadId: "t-live",
      title: "Run ls",
      kind: "execute",
      // There is an adapter on the other end of this one.
      stale: false,
    });
    // The agent's own options, passed through rather than reinterpreted.
    expect(pending.requests[0].options).toEqual(
      expect.arrayContaining([expect.objectContaining({ optionId: "allow_once" })]),
    );

    const first = await client.replyPermission({
      requestId,
      deviceId: hello.device.deviceId,
      optionId: "allow_once",
    });
    expect(first).toMatchObject({ delivered: true, alreadyAnswered: false });

    // The second click — a double tap, or a second window. It must not be an
    // error and must not become a second answer; it reports what stands.
    const second = await client.replyPermission({
      requestId,
      deviceId: hello.device.deviceId,
      optionId: "reject_once",
    });
    expect(second).toMatchObject({
      alreadyAnswered: true,
      optionId: "allow_once",
      delivered: true,
    });

    const after = await client.pendingPermissions({ threadId: "t-live" });
    expect(after.requests).toEqual([]);
  });

  /**
   * The delivered half of the same fix.
   *
   * A folded thread that hits a permission resurfaces with an unread
   * `needs_you` card. Answering the ask has to retire that card — otherwise
   * the Inbox keeps asking a question the user has already answered, and the
   * badge keeps counting it.
   *
   * Narrow on purpose: only the ask's own card. A `stuck` card the same
   * thread raised is still owed and must survive.
   */
  it("retires the Inbox card once the ask on it is answered", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-card", "permission");
    await client.fold({ threadId: "t-card" });
    const requestId = await ask(host, client, "t-card");

    const before: InboxListResult = await client.inbox();
    expect(before.events.map((event) => event.kind)).toContain("needs_you");
    expect(before.unread).toBeGreaterThan(0);

    const answered = await client.replyPermission({
      requestId,
      deviceId: hello.device.deviceId,
      optionId: "allow_once",
    });
    expect(answered).toMatchObject({ delivered: true });

    // The question is answered, so the card stops asking it. Stated as "no
    // *unread* needs_you" rather than as a row count: `resurface_thread`
    // dismisses this thread's unread cards before inserting a new one, so
    // whether the row survives depends on ordering. Whether it is read or
    // gone, the user is not being asked again — which is the whole claim.
    const after: InboxListResult = await client.inbox();
    expect(after.events.filter((e) => e.kind === "needs_you" && !e.readAt)).toEqual([]);

    // And the narrowness is the other half of the claim. Answering lets the
    // turn finish, which resurfaces the folded thread as `done` — a genuinely
    // new thing the user has not seen. The blanket `mark_inbox_read` that
    // reopening a thread uses would have swallowed it; this must not.
    //
    // Polled, because that resurface lands on the ACP pump some time after
    // `permission/reply` returns. Reading the Inbox once straight afterwards
    // is a race that passes on a slow machine and fails on a fast one — which
    // is exactly how the first version of this test went red on CI having
    // been green locally.
    const deadline = Date.now() + 5000;
    let done: InboxListResult["events"] = [];
    for (;;) {
      const list: InboxListResult = await client.inbox();
      done = list.events.filter((event) => event.kind === "done");
      if (done.length > 0) break;
      if (Date.now() > deadline) {
        throw new Error(`the turn never resurfaced as done: ${JSON.stringify(list.events)}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    expect(done[0].readAt).toBeFalsy();
    // …and answering still did not leave an unread question behind it.
    const settled: InboxListResult = await client.inbox();
    expect(settled.events.filter((e) => e.kind === "needs_you" && !e.readAt)).toEqual([]);
  });

  it("still has the question after the host that asked it was quit", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-quit", "permission");
    // Folded, because that is the shape of the story: you sent it away, it hit
    // something it had to ask about, and you quit before answering.
    await first.client.fold({ threadId: "t-quit" });
    const requestId = await ask(first.host, first.client, "t-quit");
    await first.host.stop();

    const second = await connected({ dataDir });
    // #21 is what brings the thread back; this is what makes the ask on it
    // answerable rather than a sentence about one.
    const inbox: InboxListResult = await second.client.inbox();
    expect(inbox.events.map((event) => event.kind)).toEqual(["needs_you"]);

    const pending = await second.client.pendingPermissions({ threadId: "t-quit" });
    expect(pending.requests).toHaveLength(1);
    expect(pending.requests[0]).toMatchObject({
      requestId,
      title: "Run ls",
      kind: "execute",
      // The adapter died with the host, so nothing is listening for this.
      stale: true,
    });

    const answered = await second.client.replyPermission({
      requestId,
      deviceId: second.hello.device.deviceId,
      optionId: "allow_once",
    });
    // Recorded, and honest that it went nowhere: replaying a dead ACP request
    // is exactly what `state-machine.md` says not to do.
    expect(answered).toMatchObject({ delivered: false, alreadyAnswered: false });
    expect(
      (await second.client.pendingPermissions({ threadId: "t-quit" })).requests,
    ).toEqual([]);

    // And the card that brought the user here stops asking. It used to sit
    // there unread and counted in the badge — the same question re-expanded
    // as a stale row with no buttons on it, because nothing on the resolve
    // path ever cleared the `needs_you` row the resurface wrote.
    //
    // Cleared even though the answer was undelivered: `delivered` is about
    // whether a process heard it, and the user has still answered.
    const after: InboxListResult = await second.client.inbox();
    expect(after.unread).toBe(0);
    expect(after.events.filter((event) => !event.readAt)).toEqual([]);
  });

  it("resolves an unanswered ask as cancelled when the turn is cancelled", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-cancel", "permission");
    const requestId = await ask(host, client, "t-cancel");

    await client.cancel({ threadId: "t-cancel" });
    expect(
      (await client.pendingPermissions({ threadId: "t-cancel" })).requests,
    ).toEqual([]);

    // The click that raced the Stop button. It loses, and is told so.
    const late = await client.replyPermission({
      requestId,
      deviceId: hello.device.deviceId,
      optionId: "allow_once",
    });
    expect(late).toMatchObject({ alreadyAnswered: true, cancelled: true });
  });

  it("answers a read itself under Wait for Inbox and never lists it", async () => {
    const { client } = await connected();
    await openThread(client, "t-read", "read-permission");
    await client.fold({ threadId: "t-read", policy: "wait_for_inbox" });
    await client.prompt({ threadId: "t-read", content: "read it" });

    // The turn completes without the human, so nothing was ever outstanding.
    const deadline = Date.now() + 15_000;
    for (;;) {
      const state = await client.threadState({ threadId: "t-read" });
      if (state.latestRun?.state === "succeeded") break;
      if (Date.now() > deadline) {
        throw new Error(`t-read never finished: ${JSON.stringify(state)}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
    expect((await client.pendingPermissions()).requests).toEqual([]);
  });
});
