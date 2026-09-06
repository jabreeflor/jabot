import { describe, expect, it, vi } from "vitest";

import { HostClient, HostRpcError, selectTransport } from "./client";
import {
  createHotTransport,
  HOST_BRIDGE_EVENT,
  isBridgeFrame,
  type HotChannel,
} from "./devTransport";
import {
  HOST_HELLO,
  JSONRPC_VERSION,
  RPC_ERROR,
  SESSION_UPDATE,
  type JsonRpcRequest,
} from "./protocol";

/** An `import.meta.hot` with the far end in the test's hand. */
function fakeHot() {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const sent: Array<{ event: string; data: unknown }> = [];
  const hot: HotChannel = {
    send(event, data) {
      sent.push({ event, data });
    },
    on(event, cb) {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(cb);
    },
    off(event, cb) {
      listeners.get(event)?.delete(cb);
    },
  };
  const emit = (event: string, payload: unknown) => {
    for (const cb of listeners.get(event) ?? []) cb(payload);
  };
  return { hot, sent, emit, listeners };
}

function request(id: number | string, method = HOST_HELLO): JsonRpcRequest {
  return { jsonrpc: JSONRPC_VERSION, id, method };
}

describe("createHotTransport", () => {
  it("sends on the bridge event with a wire id and restores the caller's id", async () => {
    const { hot, sent, emit } = fakeHot();
    const transport = createHotTransport(hot, "t1");

    const answer = transport.request(request(7));
    expect(sent).toEqual([
      { event: HOST_BRIDGE_EVENT, data: { jsonrpc: JSONRPC_VERSION, id: "t1:1", method: HOST_HELLO } },
    ]);

    emit(HOST_BRIDGE_EVENT, { jsonrpc: JSONRPC_VERSION, id: "t1:1", result: { ok: true } });
    await expect(answer).resolves.toEqual({
      jsonrpc: JSONRPC_VERSION,
      id: 7,
      result: { ok: true },
    });
  });

  it("ignores answers addressed to another transport on the same socket", async () => {
    const { hot, emit } = fakeHot();
    const a = createHotTransport(hot, "a");
    const b = createHotTransport(hot, "b");

    const fromA = a.request(request(1));
    const fromB = b.request(request(1));

    emit(HOST_BRIDGE_EVENT, { jsonrpc: JSONRPC_VERSION, id: "b:1", result: "for b" });
    await expect(fromB).resolves.toMatchObject({ id: 1, result: "for b" });

    emit(HOST_BRIDGE_EVENT, { jsonrpc: JSONRPC_VERSION, id: "a:1", result: "for a" });
    await expect(fromA).resolves.toMatchObject({ id: 1, result: "for a" });
  });

  it("hands notifications to the subscriber, and stops after unsubscribe", async () => {
    const { hot, emit } = fakeHot();
    const transport = createHotTransport(hot);
    const seen = vi.fn();
    const unlisten = await transport.subscribe(seen);

    const update = { jsonrpc: JSONRPC_VERSION, method: SESSION_UPDATE, params: { n: 1 } };
    emit(HOST_BRIDGE_EVENT, update);
    expect(seen).toHaveBeenCalledWith(update);

    unlisten();
    emit(HOST_BRIDGE_EVENT, update);
    expect(seen).toHaveBeenCalledTimes(1);
  });

  it("answers every in-flight request when the dev server drops", async () => {
    const { hot, emit } = fakeHot();
    const client = new HostClient(createHotTransport(hot));
    const hello = client.hello();

    emit("vite:ws:disconnect", undefined);

    await expect(hello).rejects.toBeInstanceOf(HostRpcError);
    await expect(hello).rejects.toMatchObject({
      code: RPC_ERROR.INTERNAL_ERROR,
      message: "dev server connection lost",
    });
  });

  it("close() detaches from the channel and fails what was pending", async () => {
    const { hot, emit, listeners } = fakeHot();
    const transport = createHotTransport(hot);
    const pending = transport.request(request(3));

    transport.close();

    await expect(pending).resolves.toMatchObject({ id: 3, error: { message: "transport closed" } });
    expect(listeners.get(HOST_BRIDGE_EVENT)?.size ?? 0).toBe(0);
    // Nothing left to receive it, and nothing throws.
    emit(HOST_BRIDGE_EVENT, { jsonrpc: JSONRPC_VERSION, method: SESSION_UPDATE });
    await expect(transport.request(request(4))).resolves.toMatchObject({
      error: { message: "transport closed" },
    });
  });

  it("drops frames that are not JSON-RPC 2.0", () => {
    expect(isBridgeFrame(null)).toBe(false);
    expect(isBridgeFrame("hello")).toBe(false);
    expect(isBridgeFrame({ id: 1 })).toBe(false);
    expect(isBridgeFrame({ jsonrpc: JSONRPC_VERSION, id: 1 })).toBe(true);
    expect(isBridgeFrame({ jsonrpc: JSONRPC_VERSION, method: "x" })).toBe(true);
    expect(isBridgeFrame({ jsonrpc: JSONRPC_VERSION })).toBe(false);
  });
});

describe("selectTransport", () => {
  const hot = fakeHot().hot;

  it("uses the bridge only outside Tauri and only when the plugin announced it", () => {
    const bridged = selectTransport({ tauri: false, bridge: true, hot });
    expect("close" in bridged).toBe(true);
  });

  it("keeps Tauri IPC inside the app even when the page has HMR", () => {
    const inApp = selectTransport({ tauri: true, bridge: true, hot });
    expect("close" in inApp).toBe(false);
  });

  it("keeps Tauri IPC when there is HMR but nothing serving the bridge", () => {
    // A bare `vite`, or vitest — which stubs `import.meta.hot` with an object.
    const bare = selectTransport({ tauri: false, bridge: false, hot });
    expect("close" in bare).toBe(false);
    const noHot = selectTransport({ tauri: false, bridge: true, hot: undefined });
    expect("close" in noHot).toBe(false);
  });
});
