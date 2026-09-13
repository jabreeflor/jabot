/**
 * End-to-end: Cursor's blocking extensions as questions and plans (#298),
 * over the wire.
 *
 * `src-tauri/tests/cursor_extensions.rs` makes the same claims in-process.
 * This file makes them through the production `HostClient`, a real
 * `jabot-hostd`, real SQLite and a real ACP subprocess: a question reaches
 * the client in its own list and never in the permission one, the answer
 * reaches the agent in the agent's own ids exactly once, an answer that does
 * not fit is refused without disturbing the question, and a question does
 * not come back answerable after the host that took it was quit.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  INTERACTION_ASK,
  INTERACTION_PENDING,
  INTERACTION_REPLY,
  INTERACTION_RESOLVED,
  type InteractionAskParams,
  type InteractionResolvedParams,
} from "../../src/host/protocol";
import {
  fakeAcpRuntime,
  HostdProcess,
  type HostdOptions,
} from "../support/hostd";

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

function ownDataDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-interaction-"));
  dataDirs.push(dir);
  return dir;
}

async function openThread(client: HostClient, threadId: string, mode: string) {
  return client.openThread({
    threadId,
    title: "Cursor thread",
    cwd: tmpdir(),
    harnessId: "cursor",
    runtime: fakeAcpRuntime(mode),
  });
}

/** Prompt, and come back with the ask it produced. */
async function ask(host: HostdProcess, client: HostClient, threadId: string) {
  await client.prompt({ threadId, content: "go" });
  const asked = (
    await host.waitFor(
      (n) =>
        n.method === INTERACTION_ASK &&
        (n.params as InteractionAskParams).threadId === threadId,
    )
  ).params as InteractionAskParams;
  return asked;
}

async function transcriptMethods(client: HostClient, threadId: string) {
  const transcript = await client.threadTranscript({ threadId });
  return transcript.events.map((event) => event.method);
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("questions and plans from an agent's extensions", () => {
  it("advertises the methods a client settles them through", async () => {
    const { hello } = await connected();
    expect(hello.methods).toContain(INTERACTION_PENDING);
    expect(hello.methods).toContain(INTERACTION_REPLY);
  });

  it("lists a question in its own list, answers it once in the agent's ids, and records it", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-q", "cursor-ask");
    const asked = await ask(host, client, "t-q");
    expect(asked).toMatchObject({
      ask: "question",
      method: "cursor/ask_question",
      title: "Need input",
    });
    const request = asked.request as {
      questions: { id: string; options: { id: string }[] }[];
    };
    expect(request.questions[0].id).toBe("q1");
    expect(request.questions[0].options.map((o) => o.id)).toEqual([
      "agent",
      "plan",
    ]);

    const pending = await client.pendingInteractions({ threadId: "t-q" });
    expect(pending.requests).toHaveLength(1);
    expect(pending.requests[0]).toMatchObject({
      requestId: asked.requestId,
      ask: "question",
      stale: false,
    });
    // A question is never a permission.
    expect(
      (await client.pendingPermissions({ threadId: "t-q" })).requests,
    ).toEqual([]);

    const first = await client.replyInteraction({
      requestId: asked.requestId,
      deviceId: hello.device.deviceId,
      outcome: "answered",
      answers: [{ questionId: "q1", selectedOptionIds: ["plan"] }],
    });
    expect(first).toMatchObject({
      delivered: true,
      alreadyAnswered: false,
      outcome: "answered",
      state: "answered",
    });
    const resolved = (
      await host.waitFor(
        (n) =>
          n.method === INTERACTION_RESOLVED &&
          (n.params as InteractionResolvedParams).requestId === asked.requestId,
      )
    ).params as InteractionResolvedParams;
    expect(resolved).toMatchObject({
      outcome: "answered",
      deviceId: hello.device.deviceId,
      delivered: true,
    });

    // The second click reports what stands and reaches nobody twice.
    const second = await client.replyInteraction({
      requestId: asked.requestId,
      deviceId: hello.device.deviceId,
      outcome: "skipped",
    });
    expect(second).toMatchObject({
      alreadyAnswered: true,
      outcome: "answered",
    });
    expect(
      (await client.pendingInteractions({ threadId: "t-q" })).requests,
    ).toEqual([]);

    // The agent streamed the answer it was given, in the ids it sent.
    const log = await host.waitForAdapterLog("t-q", "permission_reply=");
    expect(log).toContain('"selectedOptionIds":["plan"]');
    const methods = await transcriptMethods(client, "t-q");
    expect(methods).toContain("cursor/ask_question");
    expect(methods.filter((m) => m === INTERACTION_RESOLVED)).toHaveLength(1);
  });

  it("refuses an answer that does not fit, and the question stays open", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-bad", "cursor-ask");
    const asked = await ask(host, client, "t-bad");
    await expect(
      client.replyInteraction({
        requestId: asked.requestId,
        deviceId: hello.device.deviceId,
        outcome: "answered",
        answers: [{ questionId: "q1", selectedOptionIds: ["yolo"] }],
      }),
    ).rejects.toMatchObject({ code: -32602 });
    await expect(
      client.replyInteraction({
        requestId: asked.requestId,
        deviceId: hello.device.deviceId,
        outcome: "accepted",
      }),
    ).rejects.toBeInstanceOf(HostRpcError);
    // Still there, still answerable — and never through the permission path.
    expect(
      (await client.pendingInteractions({ threadId: "t-bad" })).requests,
    ).toHaveLength(1);
    await expect(
      client.replyPermission({
        requestId: asked.requestId,
        deviceId: hello.device.deviceId,
        optionId: "plan",
      }),
    ).rejects.toMatchObject({ code: -32602 });
    const ok = await client.replyInteraction({
      requestId: asked.requestId,
      deviceId: hello.device.deviceId,
      outcome: "answered",
      answers: [{ questionId: "q1", selectedOptionIds: ["agent"] }],
    });
    expect(ok.delivered).toBe(true);
  });

  it("puts a plan in front of the human and hands back the decision", async () => {
    const { host, client, hello } = await connected();
    await openThread(client, "t-plan", "cursor-plan");
    const asked = await ask(host, client, "t-plan");
    expect(asked).toMatchObject({ ask: "plan", title: "Auth migration" });
    const rejected = await client.replyInteraction({
      requestId: asked.requestId,
      deviceId: hello.device.deviceId,
      outcome: "rejected",
      reason: "too broad",
    });
    expect(rejected).toMatchObject({ outcome: "rejected", delivered: true });
    const log = await host.waitForAdapterLog("t-plan", "permission_reply=");
    expect(log).toContain('"rejected"');
    expect(log).toContain("too broad");
  });

  it("does not bring a question back answerable after the host that took it was quit", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-quit", "cursor-ask");
    const asked = await ask(first.host, first.client, "t-quit");
    await first.host.stop();

    const second = await connected({ dataDir });
    expect(
      (await second.client.pendingInteractions({ threadId: "t-quit" }))
        .requests,
    ).toEqual([]);
    // The transcript says why: the process that asked is gone.
    const transcript = await second.client.threadTranscript({
      threadId: "t-quit",
    });
    const settled = transcript.events.find(
      (event) => event.method === INTERACTION_RESOLVED,
    );
    expect(settled?.payload).toMatchObject({
      requestId: asked.requestId,
      outcome: "unavailable",
    });
    // A late answer is a read of what stands, never a replay.
    const late = await second.client.replyInteraction({
      requestId: asked.requestId,
      deviceId: second.hello.device.deviceId,
      outcome: "answered",
      answers: [{ questionId: "q1", selectedOptionIds: ["plan"] }],
    });
    expect(late).toMatchObject({
      alreadyAnswered: true,
      outcome: "unavailable",
      state: "unavailable",
    });
  });
});
