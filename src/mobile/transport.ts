//! The phone's transport: the same frames, over whatever the phone has.
//!
//! Decision #4 designed the host API "as if it were already a socket". #29 is
//! the milestone where that stops being a claim: a second device needs a
//! second connection, and `jabot-hostd --listen` opens one. What travels on it
//! is byte-for-byte what travels over Tauri IPC — JSON-RPC 2.0, one message
//! per line — so this file is framing and correlation and nothing else. There
//! is no mobile dialect to keep in sync, which is the point.
//!
//! [`LineChannel`] is the seam. A Unix socket (a second Mac, a test), a
//! WebSocket (a phone over Tailscale or a relay), or an SSH tunnel are all the
//! same thing to a client: something that carries lines both ways and can
//! close. The reach ladder in `remote-and-mobile/protocol-and-reach.md` is a
//! choice of `LineChannel`, not a choice of protocol.

import type {
  HostTransport,
  NotificationHandler,
} from "../host/client";
import type {
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
} from "../host/protocol";

/** A duplex of newline-delimited text. Whatever the device can actually open. */
export interface LineChannel {
  send(line: string): void;
  /** Called once per inbound line, newline already stripped. */
  onLine(handler: (line: string) => void): void;
  /** Called when the far end goes away, however it went. */
  onClose(handler: (reason?: Error) => void): void;
  close(): void;
}

export interface LineTransport extends HostTransport {
  /** Hang up. Every in-flight request rejects rather than hanging forever. */
  close(): void;
}

/** Rejecting a request because the connection went, not because the host said no. */
export class HostConnectionClosed extends Error {
  constructor(cause?: Error) {
    super(cause ? `host connection closed: ${cause.message}` : "host connection closed");
    this.name = "HostConnectionClosed";
  }
}

/**
 * A [`HostTransport`] over one line channel, for the production `HostClient`.
 *
 * Two rules are worth stating because getting either wrong looks like a host
 * bug from the outside:
 *
 * **A frame with an `id` is a response; anything else is a notification.**
 * That is the only demultiplexing there is, and it is why the host may push
 * `permission/ask` between a request and its answer without anyone noticing.
 *
 * **A closed connection rejects; it does not resolve.** A phone loses its
 * network mid-answer far more often than a webview does, and a promise that
 * never settles becomes a spinner that never stops.
 */
export function createLineTransport(channel: LineChannel): LineTransport {
  const pending = new Map<
    string,
    { resolve: (r: JsonRpcResponse) => void; reject: (e: Error) => void }
  >();
  const handlers = new Set<NotificationHandler>();
  let closed = false;

  channel.onLine((line) => {
    const text = line.trim();
    if (!text) return;
    let message: JsonRpcResponse | JsonRpcNotification;
    try {
      message = JSON.parse(text) as JsonRpcResponse | JsonRpcNotification;
    } catch {
      // A host that emits a bad line is a host bug, but dropping the line is
      // the only thing a client can usefully do: the stream is newline-framed,
      // so the next one is still parseable.
      return;
    }
    if ("id" in message && message.id !== null && message.id !== undefined) {
      const waiter = pending.get(String(message.id));
      if (!waiter) return;
      pending.delete(String(message.id));
      waiter.resolve(message as JsonRpcResponse);
      return;
    }
    for (const handler of handlers) handler(message as JsonRpcNotification);
  });

  const fail = (reason?: Error) => {
    if (closed) return;
    closed = true;
    const error = new HostConnectionClosed(reason);
    for (const [, waiter] of pending) waiter.reject(error);
    pending.clear();
  };
  channel.onClose(fail);

  return {
    request(request: JsonRpcRequest): Promise<JsonRpcResponse> {
      if (closed) return Promise.reject(new HostConnectionClosed());
      return new Promise((resolve, reject) => {
        pending.set(String(request.id), { resolve, reject });
        try {
          channel.send(JSON.stringify(request));
        } catch (err) {
          pending.delete(String(request.id));
          reject(err instanceof Error ? err : new Error(String(err)));
        }
      });
    },
    async subscribe(handler: NotificationHandler) {
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    },
    close() {
      fail();
      handlers.clear();
      channel.close();
    },
  };
}
