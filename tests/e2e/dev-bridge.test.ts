/**
 * End-to-end: the dev-server bridge in front of the real host.
 *
 * `scripts/dev/host-bridge.ts` is what `npm run dev` puts between a browser
 * tab and `jabot-hostd` when there is no Tauri (`scripts/dev/host-plugin.ts`).
 * Everything else in this suite drives the host through `HostdProcess`; this
 * file drives it through the bridge instead, with the binary the bridge would
 * spawn in a dev session, so what is asserted is the thing a web session's
 * screenshot depends on.
 *
 * Two tabs sharing one host is the case that matters: each `HostClient`
 * numbers its requests from 1, so without the bridge's id rewriting the
 * second tab's answers would land in the first tab's promises.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  HARNESS_LIST,
  HOST_HEALTH,
  HOST_HELLO,
  JSONRPC_VERSION,
  RPC_ERROR,
  SESSION_UPDATE,
  THREAD_DELETE,
  type HarnessListResult,
  type HelloResult,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
} from "../../src/host/protocol";
import {
  createHostBridge,
  type BridgeClient,
  type BridgeFrame,
  type CustomHarness,
  type HostBridge,
} from "../../scripts/dev/host-bridge";
import { fakeAcpAgentPath, fakeAcpRuntime, hostdBinaryPath } from "../support/hostd";

/** A tab: frames the bridge sends it, and a `HostTransport` over `handle`. */
function tab(bridge: HostBridge) {
  const received: BridgeFrame[] = [];
  const waiters = new Map<string | number, (r: JsonRpcResponse) => void>();
  const client: BridgeClient = {
    send(frame) {
      received.push(frame);
      if ("id" in frame && frame.id !== null) {
        waiters.get(frame.id)?.(frame);
        waiters.delete(frame.id);
      }
    },
  };
  const host = new HostClient({
    request(request: JsonRpcRequest) {
      return new Promise<JsonRpcResponse>((resolve) => {
        waiters.set(request.id as string | number, resolve);
        bridge.handle(request, client);
      });
    },
    async subscribe() {
      return () => {};
    },
  });
  return { client, host, received };
}

describe("dev bridge", () => {
  const cleanups: Array<() => void> = [];
  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup();
  });

  function bridgeUp(binary = hostdBinaryPath(), harnesses?: CustomHarness[]) {
    const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-bridge-"));
    const bridge = createHostBridge({
      binary,
      dataDir,
      harnesses,
      env: { JABOT_SECRETS_BACKEND: "memory" },
    });
    cleanups.push(() => {
      bridge.close();
      rmSync(dataDir, { recursive: true, force: true });
    });
    return bridge;
  }

  it("answers each tab on its own ids, with the real host behind it", async () => {
    const bridge = bridgeUp();
    expect(bridge.status()).toMatchObject({ running: true });
    expect(bridge.status().pid).toBeGreaterThan(0);

    const a = tab(bridge);
    const b = tab(bridge);
    // Both tabs' first request is id 1; they ask different things so the
    // answers are distinguishable if they were ever crossed.
    const [helloA, healthB] = await Promise.all([a.host.hello(), b.host.health()]);
    expect(helloA.hostMode).toBe("in-process");
    expect(healthB).toMatchObject({ connected: true });
    expect(a.received.map((f) => ("id" in f ? f.id : "n"))).toEqual([1]);
    expect(b.received.map((f) => ("id" in f ? f.id : "n"))).toEqual([1]);
    expect(bridge.status().requests).toBe(2);
  });

  it("forwards an out-of-band request before any tab has said hello", async () => {
    const bridge = bridgeUp();
    // The bridge's own hello is what makes this legal on the stdio connection.
    const response = await bridge.request({
      jsonrpc: JSONRPC_VERSION,
      id: "seed-1",
      method: HARNESS_LIST,
    });
    expect(response.id).toBe("seed-1");
    expect(response.error).toBeUndefined();
    expect((response.result as HarnessListResult).harnesses.length).toBeGreaterThan(0);
    expect(bridge.status().hello).toMatchObject({ hostName: expect.any(String) });

    const hello = await bridge.request({ jsonrpc: JSONRPC_VERSION, id: 2, method: HOST_HELLO });
    expect((hello.result as HelloResult).methods).toContain(HOST_HEALTH);
  });

  it("answers a frame that is not a request instead of forwarding it", () => {
    const bridge = bridgeUp();
    const { client, received } = tab(bridge);
    bridge.handle({ hello: "there" }, client);
    bridge.handle("not even an object", client);
    expect(received).toEqual([
      {
        jsonrpc: JSONRPC_VERSION,
        id: null,
        error: { code: RPC_ERROR.INVALID_REQUEST, message: "not a JSON-RPC 2.0 request" },
      },
      {
        jsonrpc: JSONRPC_VERSION,
        id: null,
        error: { code: RPC_ERROR.INVALID_REQUEST, message: "not a JSON-RPC 2.0 request" },
      },
    ]);
    expect(bridge.status().requests).toBe(0);
  });

  it("hands host notifications to every subscriber", async () => {
    const bridge = bridgeUp();
    const seen: JsonRpcNotification[] = [];
    const seenToo: JsonRpcNotification[] = [];
    bridge.onNotification((n) => seen.push(n));
    const off = bridge.onNotification((n) => seenToo.push(n));

    const { host } = tab(bridge);
    await host.hello();
    const updates = new Promise<void>((resolve) => {
      const stop = bridge.onNotification((n) => {
        if (n.method === SESSION_UPDATE) {
          stop();
          resolve();
        }
      });
    });
    await host.prompt({
      threadId: "t-bridge",
      content: "hello from the bridge",
      runtime: fakeAcpRuntime(),
    });
    await updates;

    expect(seen.some((n) => n.method === SESSION_UPDATE)).toBe(true);
    expect(seenToo.some((n) => n.method === SESSION_UPDATE)).toBe(true);
    const before = seenToo.length;
    off();
    await bridge.request({
      jsonrpc: JSONRPC_VERSION,
      id: "cleanup",
      method: THREAD_DELETE,
      params: { threadId: "t-bridge" },
    });
    expect(seenToo.length).toBe(before);
  });

  it("installs the harnesses it was given before the host reads its catalog", async () => {
    const bridge = bridgeUp(hostdBinaryPath(), [
      { id: "fake-acp", label: "Fake ACP", command: fakeAcpAgentPath(), args: [] },
    ]);
    const { host } = tab(bridge);
    await host.hello();
    const response = await bridge.request({
      jsonrpc: JSONRPC_VERSION,
      id: "h",
      method: HARNESS_LIST,
    });
    const listed = (response.result as HarnessListResult).harnesses;
    expect(listed.map((h) => h.id)).toContain("fake-acp");
  });

  it("answers, rather than hangs, when the binary is not there", async () => {
    const bridge = bridgeUp("/nonexistent/jabot-hostd");
    expect(bridge.status()).toMatchObject({ running: false, pid: null });
    const { host } = tab(bridge);
    await expect(host.hello()).rejects.toMatchObject({
      code: RPC_ERROR.INTERNAL_ERROR,
      message: expect.stringContaining("scripts/live.sh setup"),
    });
  });

  it("fails what is pending when the host dies, then starts a new one", async () => {
    const bridge = bridgeUp();
    const { host } = tab(bridge);
    await host.hello();
    const pid = bridge.status().pid!;
    process.kill(pid, "SIGKILL");
    await new Promise<void>((resolve) => {
      const poll = setInterval(() => {
        if (!bridge.status().running) {
          clearInterval(poll);
          resolve();
        }
      }, 10);
    });
    expect(bridge.status().exit).toMatchObject({ signal: "SIGKILL" });

    // The next request brings a fresh process up — greeted first, so even a
    // non-hello request is answered — and the data directory carries over, so
    // the second host is the same host as far as SQLite is concerned.
    const health = await host.health();
    expect(health.hostMode).toBe("in-process");
    const hello = await host.hello();
    expect(hello.hostMode).toBe("in-process");
    expect(bridge.status().running).toBe(true);
    expect(bridge.status().pid).not.toBe(pid);
  });
});
