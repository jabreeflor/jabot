/**
 * Conversation branching (#266) over the wire: the production client, a
 * live `jabot-hostd`, and a real store.
 */
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import { THREAD_BRANCH } from "../../src/host/protocol";
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
  await Promise.all(running.splice(0).map((h) => h.dispose()));
});

async function openThread(client: HostClient, threadId: string) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(),
  });
}

describe("thread/branch", () => {
  it("advertises the method", async () => {
    const { hello } = await connected();
    expect(hello.methods).toContain(THREAD_BRANCH);
  });

  it("copies history through the cut and is idempotent", async () => {
    const { client } = await connected();
    await openThread(client, "t-src");
    await client.prompt({ threadId: "t-src", content: "start the migration" });

    const source = await client.threadTranscript({ threadId: "t-src" });
    expect(source.headSeq).toBeGreaterThan(0);
    const throughSeq = source.events.find(
      (event) =>
        event.payload &&
        typeof event.payload === "object" &&
        "sessionUpdate" in event.payload &&
        (event.payload as { sessionUpdate?: string }).sessionUpdate ===
          "user_message_chunk",
    )?.seq;
    expect(throughSeq).toBeDefined();

    const first = await client.branchThread({
      threadId: "t-src",
      throughSeq: throughSeq!,
    });
    expect(first.title).toBe("Branch of Auth migration");
    expect(first.branchedFrom).toMatchObject({
      threadId: "t-src",
      throughSeq,
    });

    const replay = await client.threadTranscript({ threadId: first.threadId });
    expect(replay.events.some((event) => event.seq === throughSeq)).toBe(true);
    expect(
      replay.events.some(
        (event) =>
          event.payload &&
          typeof event.payload === "object" &&
          "jabot" in event.payload &&
          (event.payload as { jabot?: { event?: string } }).jabot?.event ===
            "branched_from",
      ),
    ).toBe(true);

    const second = await client.branchThread({
      threadId: "t-src",
      throughSeq: throughSeq!,
    });
    expect(second.threadId).toBe(first.threadId);

    const later = await client.threadTranscript({ threadId: "t-src" });
    expect(later.headSeq).toBeGreaterThanOrEqual(source.headSeq);
  });

  it("refuses a standing bot thread", async () => {
    const { client } = await connected();
    await openThread(client, "bot-chief");
    await client.prompt({ threadId: "bot-chief", content: "hi" });
    const head = (await client.threadTranscript({ threadId: "bot-chief" }))
      .headSeq;
    const failure = await client
      .branchThread({ threadId: "bot-chief", throughSeq: head || 1 })
      .catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).message).toMatch(/Code conversations/);
  });
});
