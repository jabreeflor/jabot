//! The renderer's host transport when there is no Tauri: the same frames,
//! over Vite's own HMR WebSocket.
//!
//! `src/host/client.ts` is the renderer's entire Tauri surface — one
//! `invoke("host_rpc")` and one `listen("host-rpc")`. In a plain browser the
//! first throws, so until now a web session could only ever screenshot the
//! "Host unreachable" shell. Decision #4 designed the host API "as if it were
//! already a socket", and `jabot-hostd` is that socket on stdio; what was
//! missing is a channel from a browser tab to it.
//!
//! This is that channel, and it is deliberately the one Vite already opens.
//! The dev server's HMR WebSocket carries custom events in both directions
//! (`import.meta.hot.send` / `server.ws.on`), so the `jabot-host` plugin in
//! `scripts/dev/host-plugin.ts` spawns the host and forwards frames on the
//! event named below. No new port, no new dependency, and nothing here
//! survives into a production bundle: the plugin only applies to `vite serve`,
//! and `defaultTransport()` in `client.ts` picks this transport only when the
//! plugin has said it is there.
//!
//! What this file does beyond forwarding is request-id hygiene. Every
//! `HostClient` numbers its requests from 1, and the frames of every client in
//! a page share one WebSocket, so a response for `id: 1` would otherwise be
//! claimed by whichever transport saw it first. Each transport therefore
//! sends a wire id of its own (`<tag>:<n>`) and restores the caller's id on
//! the way back; the bridge does the same across tabs. The host allows string
//! ids (`RequestId::String`), so nothing on its side knows.

import type { HostTransport, NotificationHandler } from "./client";
import {
  JSONRPC_VERSION,
  RPC_ERROR,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type RequestId,
} from "./protocol";

/** The custom HMR event both directions of the bridge travel on. */
export const HOST_BRIDGE_EVENT = "jabot:host";

/** Frames the bridge sends a tab: answers to its requests, or host pushes. */
export type BridgeFrame = JsonRpcResponse | JsonRpcNotification;

/**
 * The slice of `import.meta.hot` this transport needs, so a test can hand it
 * a fake and the production code can be handed the real one.
 */
export interface HotChannel {
  send(event: string, data?: unknown): void;
  on(event: string, cb: (payload: unknown) => void): void;
  off(event: string, cb: (payload: unknown) => void): void;
}

export interface HotTransport extends HostTransport {
  /** Stop listening. Every in-flight request is answered with an error. */
  close(): void;
}

/** A frame with an `id` is a response; anything else is a notification. */
export function isBridgeFrame(value: unknown): value is BridgeFrame {
  if (typeof value !== "object" || value === null) return false;
  const frame = value as { jsonrpc?: unknown; method?: unknown };
  if (frame.jsonrpc !== JSONRPC_VERSION) return false;
  return "id" in frame || typeof frame.method === "string";
}

interface Waiter {
  id: RequestId;
  resolve: (response: JsonRpcResponse) => void;
}

function closedResponse(id: RequestId, message: string): JsonRpcResponse {
  return {
    jsonrpc: JSONRPC_VERSION,
    id,
    error: { code: RPC_ERROR.INTERNAL_ERROR, message },
  };
}

/**
 * A [`HostTransport`] over the HMR channel, for the production `HostClient`.
 *
 * `tag` distinguishes this transport's requests from every other transport
 * on the same socket; it only has to be unique within one page load.
 */
export function createHotTransport(
  hot: HotChannel,
  tag: string = Math.random().toString(36).slice(2, 10),
): HotTransport {
  const pending = new Map<string, Waiter>();
  let seq = 0;
  let notify: NotificationHandler | null = null;
  let closed = false;

  const failAll = (message: string) => {
    for (const [, waiter] of pending) {
      waiter.resolve(closedResponse(waiter.id, message));
    }
    pending.clear();
  };

  const onFrame = (payload: unknown) => {
    if (!isBridgeFrame(payload)) return;
    if ("id" in payload) {
      const waiter =
        typeof payload.id === "string" ? pending.get(payload.id) : undefined;
      // Not ours: another transport's answer, or an id the bridge made up
      // for a frame it could not parse. Both are somebody else's business.
      if (!waiter) return;
      pending.delete(payload.id as string);
      waiter.resolve({ ...payload, id: waiter.id });
      return;
    }
    notify?.(payload);
  };

  // The dev server going away is not the host saying no, but the caller
  // still has to hear something. A promise that never settles looks exactly
  // like a hung host, which is the one bug this transport must not invent.
  const onDisconnect = () => failAll("dev server connection lost");

  hot.on(HOST_BRIDGE_EVENT, onFrame);
  hot.on("vite:ws:disconnect", onDisconnect);

  return {
    request(request: JsonRpcRequest): Promise<JsonRpcResponse> {
      if (closed) {
        return Promise.resolve(closedResponse(request.id, "transport closed"));
      }
      const wire = `${tag}:${++seq}`;
      return new Promise((resolve) => {
        pending.set(wire, { id: request.id, resolve });
        hot.send(HOST_BRIDGE_EVENT, { ...request, id: wire });
      });
    },
    async subscribe(handler: NotificationHandler) {
      notify = handler;
      return () => {
        if (notify === handler) notify = null;
      };
    },
    close() {
      closed = true;
      hot.off(HOST_BRIDGE_EVENT, onFrame);
      hot.off("vite:ws:disconnect", onDisconnect);
      failAll("transport closed");
      notify = null;
    },
  };
}
