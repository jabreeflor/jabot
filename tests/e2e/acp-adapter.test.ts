/**
 * End-to-end: the ACP adapter layer (#10) through the real wire.
 *
 * `src-tauri/tests/acp_adapter.rs` drives `HostSession` in-process, where the
 * test can call `pump_acp()` itself. This file makes the same claims from the
 * other side of the protocol — a `HostClient` and a `jabot-hostd` process with
 * a real adapter subprocess under it — so "the adapter works" means the bytes
 * actually make the round trip, not that a Rust helper was polled.
 *
 * The adapter under test is `fake_acp_agent.rs`, addressed by absolute path
 * the way `acp_adapter.rs` addresses it through `CARGO_BIN_EXE_*`.
 *
 * Four of these cases were written first against defects they then proved:
 * `jabot-hostd` drained outbound notifications only after handling a request
 * and never called `pump_acp`, so adapter events sat unread and no
 * `session/update` or `permission/ask` ever reached a stdio client; and
 * `session_cancel` cancelled the turn before answering outstanding permission
 * requests, the reverse of what #10 specifies. Both are fixed — the stdio host
 * now runs the same pump thread the Tauri host does, and cancel answers first
 * — and the cases run live.
 * ---------------------------------------------------------------------------
 */
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  PERMISSION_ASK,
  PERMISSION_RESOLVED,
  RPC_ERROR,
  SESSION_CANCEL,
  SESSION_PROMPT,
  SESSION_UPDATE,
  type JsonRpcNotification,
  type PermissionAskParams,
  type PermissionResolvedParams,
  type PromptResult,
  type SessionCancelResult,
  type SessionUpdateParams,
} from "../../src/host/protocol";
import {
  cargoDebugDir,
  fakeAcpRuntime,
  HostdProcess,
  type HostdOptions,
} from "../support/hostd";

const running: HostdProcess[] = [];

async function connected(options?: HostdOptions) {
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

/** Prompt params that spawn the fake agent for a thread the store never saw. */
function promptFor(threadId: string, mode?: string, content = "hi") {
  return { threadId, content, cwd: tmpdir(), runtime: fakeAcpRuntime(mode) };
}

function updateOf(notification: JsonRpcNotification): SessionUpdateParams {
  return notification.params as SessionUpdateParams;
}

function acpOf(notification: JsonRpcNotification): Record<string, unknown> {
  return updateOf(notification).acp as Record<string, unknown>;
}

function isUpdate(threadId: string, kind: string) {
  return (n: JsonRpcNotification) =>
    n.method === SESSION_UPDATE &&
    updateOf(n).threadId === threadId &&
    acpOf(n).sessionUpdate === kind;
}

/** The fake agent narrates to stderr, which the host tees into the thread log. */
async function waitForAdapterLog(
  host: HostdProcess,
  threadId: string,
  needle: string,
  timeoutMs = 10_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const log = host.readAdapterLog(threadId);
    if (log.includes(needle)) return log;
    if (Date.now() > deadline) {
      throw new Error(`adapter log for ${threadId} never mentioned ${needle}; saw: ${log}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

describe("adapter spawn", () => {
  it("completes the ACP handshake and accepts the prompt", async () => {
    const { host } = await connected();

    const response = await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-spawn"));

    // `accepted` is only reachable if initialize and session/new both round
    // tripped over the adapter's stdio — the session id comes from the agent.
    expect(response.error).toBeUndefined();
    expect(response.result).toMatchObject({
      threadId: "t-spawn",
      acpSessionId: "sess-fake-1",
      accepted: true,
    });
  });

  it("resolves a bare adapter command through the host's PATH", async () => {
    const { host } = await connected({
      env: { PATH: `${cargoDebugDir()}:${process.env.PATH ?? ""}` },
    });

    const response = await host.call<PromptResult>(SESSION_PROMPT, {
      threadId: "t-path",
      content: "hi",
      cwd: tmpdir(),
      runtime: { command: "fake-acp-agent" },
    });

    expect(response.error).toBeUndefined();
    expect(response.result?.accepted).toBe(true);
  });

  it("answers a missing adapter binary with an install hint and stays up", async () => {
    const { host, client } = await connected();

    const response = await host.call(SESSION_PROMPT, {
      threadId: "t-missing",
      content: "hi",
      cwd: tmpdir(),
      runtime: {
        command: "jabot-definitely-not-on-path-xyz",
        installHint: "npm i -g jabot-definitely-not-on-path-xyz",
      },
    });

    expect(response.error?.code).toBe(RPC_ERROR.HARNESS_UNAVAILABLE);
    expect(response.error?.data).toMatchObject({
      command: "jabot-definitely-not-on-path-xyz",
      installHint: "npm i -g jabot-definitely-not-on-path-xyz",
    });
    // A missing harness is a message, not a crash: the same connection keeps
    // serving. If the host had panicked or hung, this would time out instead.
    expect((await client.health()).connected).toBe(true);
  });

  it("refuses a prompt it cannot resolve a runtime for", async () => {
    const { client } = await connected();

    // Until #15 puts threads in the store, an unknown thread must carry its
    // own runtime; guessing one would spawn the wrong harness.
    await expect(
      client.prompt({ threadId: "t-no-runtime", content: "hi" }),
    ).rejects.toMatchObject({ name: "HostRpcError", code: RPC_ERROR.INVALID_PARAMS });
  });

  it("tees adapter stderr to a per-thread log under the data dir", async () => {
    const { host } = await connected({ persistent: true });
    await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-log"));

    const cancel = await host.call<SessionCancelResult>(SESSION_CANCEL, {
      threadId: "t-log",
    });

    expect(cancel.result?.cancelled).toBe(true);
    // The agent prints `cancelled` when the ACP notification lands, so the log
    // is proof the cancel travelled host → adapter stdin, not just that the
    // host returned success.
    expect(await waitForAdapterLog(host, "t-log", "cancelled")).toContain("cancelled");
  });
});

describe("adapter errors", () => {
  it("rejects a cancel for a thread with no live adapter", async () => {
    const { client } = await connected();

    await expect(client.cancel({ threadId: "t-never-prompted" })).rejects.toMatchObject({
      name: "HostRpcError",
      code: RPC_ERROR.INTERNAL_ERROR,
    });
  });

  it("rejects a permission reply for a request it is not holding", async () => {
    const { client, hello } = await connected();

    const failure = await client
      .replyPermission({
        requestId: "not-a-pending-request",
        deviceId: hello.device.deviceId,
        optionId: "allow_once",
      })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.INVALID_PARAMS);
  });

  it("replays nothing for a thread the host has never streamed", async () => {
    const { client } = await connected();

    const replay = await client.resumeFrom({ threadId: "t-unknown", seq: 0 });

    expect(replay).toEqual({ threadId: "t-unknown", headSeq: 0, events: [] });
  });
});

describe("adapter streaming", () => {
  it("streams session/update with a host-stamped, strictly increasing envelope", async () => {
    const { host, hello } = await connected();
    await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-stream"));

    // Seq 1 is the host's own echo of the prompt: #14 writes the user's words
    // into the transcript as an ACP `user_message_chunk`, so the agent's first
    // chunk is the second event on the thread rather than the first.
    const echo = await host.waitFor(isUpdate("t-stream", "user_message_chunk"));
    expect(updateOf(echo).seq).toBe(1);

    const chunk = await host.waitFor(isUpdate("t-stream", "agent_message_chunk"));
    expect(updateOf(chunk).hostId).toBe(hello.hostId);
    expect(updateOf(chunk).threadId).toBe("t-stream");
    expect(updateOf(chunk).seq).toBe(2);
    expect(JSON.stringify(acpOf(chunk))).toContain("hello from fake-acp");

    // The turn ending is itself an update, so the client learns idle without
    // polling — and it must be numbered after the chunk it follows.
    const idle = await host.waitFor(isUpdate("t-stream", "state_update"));
    expect(acpOf(idle).stopReason).toBe("end_turn");

    const seqs = host.notifications(SESSION_UPDATE).map((n) => updateOf(n).seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    expect(new Set(seqs).size).toBe(seqs.length);
  });

  it("surfaces a permission request and routes the answer back to the agent", async () => {
    const { host, client, hello } = await connected();
    await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-perm", "permission", "rm -rf"));

    const ask = await host.waitFor(PERMISSION_ASK);
    const asked = ask.params as PermissionAskParams;
    expect(asked.hostId).toBe(hello.hostId);
    expect(asked.threadId).toBe("t-perm");
    expect(asked.requestId).toMatch(/^[0-9a-f-]{36}$/);
    expect(asked.subject).toMatchObject({ kind: "execute", title: "Run ls" });
    expect(asked.options).toEqual(
      expect.arrayContaining([expect.objectContaining({ optionId: "allow_once" })]),
    );

    await client.replyPermission({
      requestId: asked.requestId,
      deviceId: hello.device.deviceId,
      optionId: "allow_once",
    });

    const resolved = (await host.waitFor(PERMISSION_RESOLVED))
      .params as PermissionResolvedParams;
    expect(resolved.requestId).toBe(asked.requestId);
    expect(resolved.optionId).toBe("allow_once");
    expect(resolved.deviceId).toBe(hello.device.deviceId);
    expect(resolved.seq).toBeGreaterThan(asked.seq);

    // The agent only emits this chunk once it has read our outcome, so it is
    // the proof the answer reached ACP rather than stopping at the host.
    const allowed = await host.waitFor(
      (n) =>
        isUpdate("t-perm", "agent_message_chunk")(n) &&
        JSON.stringify(acpOf(n)).includes("allowed"),
    );
    expect(updateOf(allowed).seq).toBeGreaterThan(resolved.seq);
  });

  it("replays through sync/resumeFrom exactly what a disconnected client missed", async () => {
    const { host, client } = await connected();
    await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-resume"));
    const echo = await host.waitFor(isUpdate("t-resume", "user_message_chunk"));
    const chunk = await host.waitFor(isUpdate("t-resume", "agent_message_chunk"));
    const idle = await host.waitFor(isUpdate("t-resume", "state_update"));

    const missedAll = await client.resumeFrom({ threadId: "t-resume", seq: 0 });
    expect(missedAll.headSeq).toBe(updateOf(idle).seq);
    // The prompt echo is on the thread's event log like anything else, so a
    // client that missed the whole turn is replayed the whole turn (#14).
    expect(missedAll.events.map((e) => e.seq)).toEqual([
      updateOf(echo).seq,
      updateOf(chunk).seq,
      updateOf(idle).seq,
    ]);
    // A reconnecting client must get the same envelope it would have received
    // live, or the transcript it rebuilds is a different one.
    expect(missedAll.events[1].params).toEqual(chunk.params);

    const missedTail = await client.resumeFrom({
      threadId: "t-resume",
      seq: updateOf(chunk).seq,
    });
    expect(missedTail.headSeq).toBe(updateOf(idle).seq);
    expect(missedTail.events).toEqual([
      { seq: updateOf(idle).seq, method: SESSION_UPDATE, params: idle.params },
    ]);
  });
});

describe("cancel ordering", () => {
  it("answers outstanding permission requests as cancelled before cancelling the turn", async () => {
    const { host, client } = await connected({ persistent: true });
    await host.call<PromptResult>(SESSION_PROMPT, promptFor("t-cancel", "permission", "rm -rf"));
    const asked = (await host.waitFor(PERMISSION_ASK)).params as PermissionAskParams;

    await client.cancel({ threadId: "t-cancel" });

    const resolved = (await host.waitFor(PERMISSION_RESOLVED))
      .params as PermissionResolvedParams;
    expect(resolved.requestId).toBe(asked.requestId);
    expect(resolved.cancelled).toBe(true);
    expect(resolved.optionId).toBeUndefined();

    // Ordering is the contract, not just that both happen: an agent that gets
    // session/cancel first is free to tear the turn down with a permission
    // request still outstanding, which is the hang #10 exists to prevent.
    // The fake agent logs `permission_reply=` and `cancelled` as it reads
    // them, so the log is the order the host wrote to its stdin. This is the
    // assertion `session_cancel`'s current call order fails — see the header.
    const log = await waitForAdapterLog(host, "t-cancel", "cancelled");
    expect(log).toContain("permission_reply=");
    expect(log.indexOf("permission_reply=")).toBeLessThan(log.indexOf("cancelled"));
  });
});
