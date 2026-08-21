//! The phone client itself: what it sends, and what a notification does to it.
//!
//! Driven through a fake `HostTransport` rather than a live host — the live
//! host is `tests/e2e/mobile-inbox.test.ts`, which is where "and the adapter
//! actually heard it" is proved. What is asserted here is client behaviour:
//! the handshake it performs, the frames it emits, and the fact that a card
//! disappears when *another* device answers.

import { describe, expect, it, vi } from "vitest";

import type { HostTransport, NotificationHandler } from "../../host/client";
import {
  HOST_HELLO,
  INBOX_LIST,
  JSONRPC_VERSION,
  PERMISSION_ASK,
  PERMISSION_PENDING,
  PERMISSION_REPLY,
  PERMISSION_RESOLVED,
  SESSION_CANCEL,
  SESSION_PROMPT,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
} from "../../host/protocol";
import { APPROVER_METHODS } from "../scope";
import { MobileSession, OutOfScopeError } from "../session";

/** A host that answers the four methods this client uses, and can push. */
class FakeHost implements HostTransport {
  readonly sent: JsonRpcRequest[] = [];
  private handlers = new Set<NotificationHandler>();
  asks: unknown[] = [];
  scoped: readonly string[] = APPROVER_METHODS;

  constructor(private readonly deviceRole: "full" | "approver" = "approver") {}

  async request(request: JsonRpcRequest): Promise<JsonRpcResponse> {
    this.sent.push(request);
    const ok = (result: unknown): JsonRpcResponse => ({
      jsonrpc: JSONRPC_VERSION,
      id: request.id,
      result,
    });
    switch (request.method) {
      case HOST_HELLO:
        return ok({
          protocolVersion: 1,
          hostId: "host-1",
          hostName: "This Mac",
          hostMode: "in-process",
          version: "0.1.0",
          platform: "macos",
          device: {
            deviceId: "phone-1",
            name: "Jabree's iPhone",
            role: this.deviceRole,
          },
          methods: [...APPROVER_METHODS, SESSION_PROMPT],
          scopedMethods: [...this.scoped],
          notifications: [],
        });
      case INBOX_LIST:
        return ok({ events: [], sleeping: [], unread: 0 });
      case PERMISSION_PENDING:
        return ok({ requests: this.asks });
      case PERMISSION_REPLY:
        return ok({
          requestId: (request.params as { requestId: string }).requestId,
          delivered: true,
          alreadyAnswered: false,
          cancelled: false,
        });
      default:
        return {
          jsonrpc: JSONRPC_VERSION,
          id: request.id,
          error: { code: -32601, message: "method not found" },
        };
    }
  }

  async subscribe(handler: NotificationHandler) {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  push(notification: JsonRpcNotification) {
    for (const handler of this.handlers) handler(notification);
  }
}

const CREDENTIALS = {
  deviceId: "phone-1",
  name: "Jabree's iPhone",
  signHello: () => ({ counter: 7, mac: "ab".repeat(32) }),
};

function askNotification(requestId = "req-1"): JsonRpcNotification {
  return {
    jsonrpc: JSONRPC_VERSION,
    method: PERMISSION_ASK,
    params: {
      hostId: "host-1",
      threadId: "t9",
      seq: 3,
      requestId,
      subject: { title: "Run ls", command: "ls -la" },
      options: [{ optionId: "allow_once", name: "Allow once", kind: "allow_once" }],
    },
  };
}

describe("the phone client", () => {
  it("says hello as its own device, with the proof its pairing derived", async () => {
    const host = new FakeHost();
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    const hello = await session.connect();

    const sent = host.sent.find((r) => r.method === HOST_HELLO);
    expect(sent?.params).toMatchObject({
      protocolVersion: 1,
      device: { deviceId: "phone-1", name: "Jabree's iPhone" },
      auth: { counter: 7 },
    });
    // The role is the host's answer, never what the device asked for.
    expect(hello.device.role).toBe("approver");
    expect(session.scopedMethods).toEqual(APPROVER_METHODS);
  });

  it("never sends a method an approver may not call", async () => {
    const host = new FakeHost();
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    await session.connect();
    host.push(askNotification());
    await session.refresh();
    await session.answer("req-1", "allow_once");
    await session.cancelThread("t9").catch(() => {});
    await session.transcript("t9").catch(() => {});

    // The host enforces this on every request from the `paired_devices` row;
    // what is asserted here is that the client cannot even *emit* something
    // outside the role, so there is no button that always fails.
    for (const request of host.sent) {
      expect(APPROVER_METHODS).toContain(request.method);
    }
    expect(APPROVER_METHODS).not.toContain(SESSION_PROMPT);
  });

  /// Drift, in the direction that matters: the host narrowed the role and this
  /// client has not been rebuilt. It must refuse locally rather than send a
  /// call it now knows will come back `DEVICE_SCOPE`.
  it("stops offering a method the host has taken off this device", async () => {
    const host = new FakeHost();
    host.scoped = APPROVER_METHODS.filter((m) => m !== SESSION_CANCEL);
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    await session.connect();

    await expect(session.cancelThread("t9")).rejects.toBeInstanceOf(OutOfScopeError);
    expect(host.sent.some((r) => r.method === SESSION_CANCEL)).toBe(false);
    // Everything still granted keeps working.
    await expect(session.refresh()).resolves.toBeTruthy();
  });

  it("draws a card from the ask notification alone", async () => {
    const host = new FakeHost();
    const session = new MobileSession({
      transport: host,
      credentials: CREDENTIALS,
      now: () => new Date("2026-08-20T11:00:00.000Z"),
    });
    await session.connect();
    const seen = vi.fn();
    session.onInbox(seen);

    // No round trip: the notification is what woke the phone up, and it
    // carries everything the card needs.
    host.push(askNotification());
    expect(session.inbox.needs).toHaveLength(1);
    expect(session.inbox.needs[0]).toMatchObject({
      id: "req-1",
      title: "Run ls",
      summary: "ls -la",
    });
    expect(seen).toHaveBeenCalledTimes(2); // initial snapshot, then the ask
  });

  it("answers with the option the agent offered, naming this device", async () => {
    const host = new FakeHost();
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    await session.connect();
    host.push(askNotification());

    const result = await session.answer("req-1", "allow_once");
    expect(result.delivered).toBe(true);
    expect(host.sent[host.sent.length - 1]).toMatchObject({
      method: PERMISSION_REPLY,
      params: { requestId: "req-1", optionId: "allow_once", deviceId: "phone-1" },
    });
    // The card goes as soon as the answer lands; the host is idempotent, so a
    // `permission/resolved` arriving afterwards agrees rather than fights.
    expect(session.inbox.needs).toEqual([]);
  });

  it("drops the card when another device answers first", async () => {
    const host = new FakeHost();
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    await session.connect();
    host.push(askNotification());
    expect(session.inbox.needs).toHaveLength(1);

    // The research's contract: the host broadcasts, the first authentic reply
    // wins, everyone else is told it was resolved. On a phone that has to mean
    // the buttons go away — otherwise the second tap is one tap away.
    host.push({
      jsonrpc: JSONRPC_VERSION,
      method: PERMISSION_RESOLVED,
      params: {
        hostId: "host-1",
        threadId: "t9",
        seq: 4,
        requestId: "req-1",
        deviceId: "mac-1",
        optionId: "allow_once",
      },
    });
    expect(session.inbox.needs).toEqual([]);
  });

  it("pulls the Inbox and the outstanding asks in one refresh", async () => {
    const host = new FakeHost();
    host.asks = [
      {
        requestId: "req-2",
        threadId: "t1",
        title: "Write src/main.rs",
        subject: { title: "Write src/main.rs" },
        options: [],
        createdAt: "2026-08-20T10:00:00.000Z",
        stale: true,
      },
    ];
    const session = new MobileSession({ transport: host, credentials: CREDENTIALS });
    await session.connect();
    const inbox = await session.refresh();

    expect(host.sent.map((r) => r.method)).toEqual(
      expect.arrayContaining([INBOX_LIST, PERMISSION_PENDING]),
    );
    // An ask whose adapter is gone is still answerable and says so (#20).
    expect(inbox.needs[0].ask).toMatchObject({ requestId: "req-2", stale: true });
  });
});
