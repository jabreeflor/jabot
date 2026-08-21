//! The line transport: framing, correlation, and hanging up.

import { describe, expect, it, vi } from "vitest";

import { JSONRPC_VERSION, PERMISSION_ASK } from "../../host/protocol";
import { createLineTransport, HostConnectionClosed, type LineChannel } from "../transport";

/** A channel a test can drive both ways. */
function fakeChannel() {
  const sent: string[] = [];
  let onLine: ((line: string) => void) | null = null;
  let onClose: ((reason?: Error) => void) | null = null;
  const channel: LineChannel = {
    send: (line) => sent.push(line),
    onLine: (handler) => {
      onLine = handler;
    },
    onClose: (handler) => {
      onClose = handler;
    },
    close: vi.fn(),
  };
  return {
    channel,
    sent,
    deliver: (value: unknown) => onLine?.(JSON.stringify(value)),
    deliverRaw: (line: string) => onLine?.(line),
    hangUp: (reason?: Error) => onClose?.(reason),
  };
}

describe("the phone's transport", () => {
  it("correlates a response by id and leaves notifications alone", async () => {
    const wire = fakeChannel();
    const transport = createLineTransport(wire.channel);
    const seen: string[] = [];
    await transport.subscribe((n) => seen.push(n.method));

    const pending = transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: 1,
      method: "host/hello",
    });
    // The host may push between a request and its answer — a `permission/ask`
    // arriving mid-handshake must not be mistaken for the handshake's reply.
    wire.deliver({ jsonrpc: JSONRPC_VERSION, method: PERMISSION_ASK, params: {} });
    wire.deliver({ jsonrpc: JSONRPC_VERSION, id: 1, result: { ok: true } });

    expect((await pending).result).toEqual({ ok: true });
    expect(seen).toEqual([PERMISSION_ASK]);
    expect(JSON.parse(wire.sent[0])).toMatchObject({ id: 1, method: "host/hello" });
  });

  it("survives a line it cannot parse", async () => {
    const wire = fakeChannel();
    const transport = createLineTransport(wire.channel);
    const pending = transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: 1,
      method: "host/health",
    });
    // Newline framing means the next line is still readable; dying here would
    // take out a connection over one bad frame.
    wire.deliverRaw("{not json");
    wire.deliverRaw("");
    wire.deliver({ jsonrpc: JSONRPC_VERSION, id: 1, result: { ok: true } });
    expect((await pending).result).toEqual({ ok: true });
  });

  it("rejects everything in flight when the connection goes", async () => {
    const wire = fakeChannel();
    const transport = createLineTransport(wire.channel);
    const pending = transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: 1,
      method: "inbox/list",
    });
    // A phone loses the network mid-answer far more often than a webview does.
    // A promise that never settles is a spinner that never stops.
    wire.hangUp(new Error("EPIPE"));
    await expect(pending).rejects.toBeInstanceOf(HostConnectionClosed);
    await expect(
      transport.request({ jsonrpc: JSONRPC_VERSION, id: 2, method: "inbox/list" }),
    ).rejects.toBeInstanceOf(HostConnectionClosed);
  });
});
