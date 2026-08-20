/**
 * End-to-end: the transcript overlay and the prompt queue (#14) over the wire.
 *
 * The production `HostClient`, a live `jabot-hostd`, a real SQLite store and a
 * real ACP subprocess under it. `src-tauri/tests/transcript.rs` makes the same
 * claims in-process, where a test can pump the adapter itself; this file makes
 * them the way a renderer experiences them — notifications arriving on their
 * own schedule, and `thread/transcript` answering across a restart.
 *
 * The case worth reading twice is the last one: it reduces the replayed rows
 * through the *production* reducer from `src/views/transcript.ts`. That is the
 * whole claim of the issue — reopening a thread replays from our store — and
 * it is only proven if the thing that draws the chat is the thing under test.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  RPC_ERROR,
  SESSION_UPDATE,
  THREAD_TRANSCRIPT,
  type JsonRpcNotification,
  type SessionUpdateParams,
} from "../../src/host/protocol";
import { applyAcpEvent, hydrate } from "../../src/views/transcript";
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

/** A data dir this file owns, so a host can be stopped and started on it. */
function ownDataDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-transcript-"));
  dataDirs.push(dir);
  return dir;
}

async function openThread(client: HostClient, threadId: string, mode?: string) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

/** Poll `thread/transcript` until the replay satisfies the test. */
async function settle(
  client: HostClient,
  threadId: string,
  predicate: (result: Awaited<ReturnType<HostClient["threadTranscript"]>>) => boolean,
  timeoutMs = 10_000,
) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const result = await client.threadTranscript({ threadId });
    if (predicate(result)) return result;
    if (Date.now() > deadline) {
      throw new Error(
        `transcript never settled; saw ${result.events.length} events`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

function updates(host: HostdProcess, threadId: string): SessionUpdateParams[] {
  return host
    .notifications(SESSION_UPDATE)
    .map((n: JsonRpcNotification) => n.params as SessionUpdateParams)
    .filter((params) => params.threadId === threadId);
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("transcript overlay", () => {
  // The drift guard: a method the TS client can call must be one the Rust host
  // admits to, or the two halves of the protocol have already parted company.
  it("is a method the host advertises", async () => {
    const { hello } = await connected();
    expect(hello.methods).toContain(THREAD_TRANSCRIPT);
  });

  it("persists what it streams, and stamps both with the same seq", async () => {
    const { host, client } = await connected();
    await openThread(client, "t-stream", "tools");
    await client.prompt({ threadId: "t-stream", content: "fix the guard" });

    // Wait on the *notification* that ends the turn, not on the row: the row
    // is written first, so a test that waits for the row can read the stream
    // before the last event has been delivered.
    await host.waitFor(
      (n) =>
        n.method === SESSION_UPDATE &&
        (n.params as SessionUpdateParams).threadId === "t-stream" &&
        (((n.params as SessionUpdateParams).acp as { sessionUpdate?: string })
          .sessionUpdate ?? "") === "state_update",
    );
    const replay = await client.threadTranscript({ threadId: "t-stream" });

    const streamed = updates(host, "t-stream");
    expect(streamed.length).toBeGreaterThan(0);
    // Delivered in `seq` order. The envelope's counter is only worth anything
    // if the stream respects it, and a host with two notification drainers
    // will interleave them unless it emits under its own lock.
    const seqs = streamed.map((params) => params.seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    // The pairing a client hydrating mid-stream depends on: every streamed
    // event names the row it was written to, and the rows are those events.
    expect(streamed.map((params) => params.transcriptSeq)).toEqual(
      replay.events.map((event) => event.seq),
    );

    // The user's own words are in the log too — the host writes them as the
    // ACP `user_message_chunk` an agent would have sent.
    expect(replay.events[0].payload).toMatchObject({
      sessionUpdate: "user_message_chunk",
      content: { text: "fix the guard" },
    });
  });

  it("replays a conversation to a host that never saw it", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    await openThread(first.client, "t-restart", "tools");
    await first.client.prompt({ threadId: "t-restart", content: "fix the guard" });
    await settle(first.client, "t-restart", (result) =>
      result.events.some(
        (event) =>
          (event.payload as { sessionUpdate?: string }).sessionUpdate ===
          "state_update",
      ),
    );
    // Quit: the adapter's process group dies with it, and nothing about this
    // conversation survives except the rows.
    await first.host.stop();

    const { client } = await connected({ dataDir });
    const replay = await client.threadTranscript({ threadId: "t-restart" });

    // Reduced through the production mapper, because "replays from our store"
    // is a claim about the chat, not about a row count.
    const stream = hydrate(replay);
    const kinds = stream.items.map((item) => item.kind);
    expect(kinds).toContain("user");
    expect(kinds).toContain("agent");
    expect(kinds).toContain("tool");
    expect(stream.items[0]).toMatchObject({
      kind: "user",
      text: "fix the guard",
    });
    // The edit's diff became the prototype's trailing note, from the ACP
    // content alone — no harness log was read to work it out.
    const edit = stream.items.find(
      (item) => item.kind === "tool" && item.call.kind === "edit",
    );
    expect(edit).toMatchObject({ call: { note: "+2 −1" } });
    // And the tool call whose kind nothing has ever heard of is a line, not a
    // crash and not a gap.
    expect(
      stream.items.some(
        (item) => item.kind === "tool" && item.call.target === "summon",
      ),
    ).toBe(true);
    expect(stream.lastStopReason).toBe("end_turn");
    expect(stream.busy).toBe(false);
  });

  it("hydrating while streaming applies every event exactly once", async () => {
    const { host, client } = await connected();
    await openThread(client, "t-race", "tools");
    await client.prompt({ threadId: "t-race", content: "go" });
    await settle(client, "t-race", (result) => result.events.length >= 4);

    // A renderer's exact sequence: subscribe, read, then replay the buffer.
    // Everything that arrived during the read is in both, and the `seq` on
    // each is what stops it being drawn twice.
    const replay = await client.threadTranscript({ threadId: "t-race" });
    let stream = hydrate(replay);
    const hydratedItems = stream.items.length;
    for (const params of updates(host, "t-race")) {
      stream = applyAcpEvent(stream, params.acp, params.transcriptSeq);
    }
    expect(stream.items).toHaveLength(hydratedItems);
  });
});

describe("steer vs redispatch", () => {
  it("queues a follow-up and sends it when the turn ends", async () => {
    const { host, client } = await connected();
    // `late-end` keeps the turn open long enough that the follow-up really is
    // sent mid-flight rather than racing a turn that already finished.
    await openThread(client, "t-queue", "late-end");
    await client.prompt({ threadId: "t-queue", content: "first" });

    const held = await client.prompt({
      threadId: "t-queue",
      content: "second",
      mode: "queue",
    });
    expect(held).toMatchObject({ queued: true, accepted: false, queuePosition: 1 });

    const waiting = await client.threadTranscript({ threadId: "t-queue" });
    expect(waiting.queued).toHaveLength(1);
    expect(waiting.queued[0].content).toBe("second");
    // The turn the replay cannot describe: the events end the same way whether
    // the agent is working or the host died under it, so the ledger comes too.
    expect(waiting.runState).toBe("running");

    const settled = await settle(client, "t-queue", (result) =>
      said(result.events).length === 2,
    );
    expect(said(settled.events)).toEqual(["first", "second"]);
    expect(settled.queued).toHaveLength(0);

    // And the same thing through the production reducer, which is where the
    // strip the user actually sees comes from: hydrate at the moment the
    // follow-up is waiting, then apply the real notifications that arrived
    // after it. The strip has to come down when the host dispatches the
    // prompt — left up, it shows a delivered message as still waiting and its
    // "Send now" button cancels an unrelated turn.
    let stream = hydrate(waiting);
    expect(stream.queued).toEqual(["second"]);
    expect(stream.busy).toBe(true);
    for (const params of updates(host, "t-queue")) {
      stream = applyAcpEvent(stream, params.acp, params.transcriptSeq);
    }
    expect(stream.queued).toEqual([]);

    // Two turns, two runs. Never one run collecting both outcomes (#15).
    const state = await client.threadState({ threadId: "t-queue" });
    expect(state.runs).toHaveLength(2);
  });

  it("still refuses an unqualified second prompt", async () => {
    const { client } = await connected();
    await openThread(client, "t-reject", "hang");
    await client.prompt({ threadId: "t-reject", content: "first" });

    // #15's contract, unchanged: a client that has not asked for a queue does
    // not get one by accident.
    const failure = await client
      .prompt({ threadId: "t-reject", content: "second" })
      .catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.RUN_IN_FLIGHT);

    const transcript = await client.threadTranscript({ threadId: "t-reject" });
    expect(said(transcript.events)).toEqual(["first"]);
    expect(transcript.queued).toHaveLength(0);
  });
});

/** Every prompt the agent was actually given, in order. */
function said(events: readonly { payload: unknown }[]): string[] {
  return events
    .map((event) => event.payload as Record<string, unknown>)
    .filter((payload) => payload?.sessionUpdate === "user_message_chunk")
    .map(
      (payload) =>
        ((payload.content as { text?: string } | undefined)?.text ?? "") as string,
    );
}
