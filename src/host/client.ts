//! Transport-agnostic JSON-RPC client for the JaBot host protocol.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  HOST_HEALTH,
  HOST_HELLO,
  HOST_RPC_EVENT,
  JSONRPC_VERSION,
  PERMISSION_REPLY,
  PROTOCOL_VERSION,
  SESSION_CANCEL,
  SESSION_PROMPT,
  SYNC_RESUME_FROM,
  THREAD_FOLD,
  type HealthResult,
  type HelloParams,
  type HelloResult,
  type JsonRpcError,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type PermissionReplyParams,
  type PromptParams,
  type ResumeFromParams,
  type ResumeFromResult,
  type SessionCancelParams,
  type ThreadFoldParams,
} from "./protocol";

export class HostRpcError extends Error {
  readonly code: number;
  readonly data?: unknown;

  constructor(error: JsonRpcError) {
    super(error.message);
    this.name = "HostRpcError";
    this.code = error.code;
    this.data = error.data;
  }
}

export type NotificationHandler = (notification: JsonRpcNotification) => void;

export interface HostTransport {
  request(request: JsonRpcRequest): Promise<JsonRpcResponse>;
  subscribe(handler: NotificationHandler): Promise<() => void>;
}

export function createTauriTransport(): HostTransport {
  return {
    async request(request) {
      return invoke<JsonRpcResponse>("host_rpc", { request });
    },
    async subscribe(handler) {
      const unlisten: UnlistenFn = await listen<JsonRpcNotification>(
        HOST_RPC_EVENT,
        (event) => handler(event.payload),
      );
      return unlisten;
    },
  };
}

export class HostClient {
  private nextId = 1;
  private unlisten: (() => void) | null = null;
  private notificationHandlers = new Set<NotificationHandler>();

  constructor(private readonly transport: HostTransport = createTauriTransport()) {}

  async connect(): Promise<void> {
    if (this.unlisten) return;
    this.unlisten = await this.transport.subscribe((notification) => {
      for (const handler of this.notificationHandlers) {
        handler(notification);
      }
    });
  }

  disconnect(): void {
    this.unlisten?.();
    this.unlisten = null;
    this.notificationHandlers.clear();
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandlers.add(handler);
    return () => {
      this.notificationHandlers.delete(handler);
    };
  }

  async hello(params: HelloParams = {}): Promise<HelloResult> {
    return this.request<HelloResult>(HOST_HELLO, {
      ...params,
      protocolVersion: params.protocolVersion ?? PROTOCOL_VERSION,
    });
  }

  async health(): Promise<HealthResult> {
    return this.request<HealthResult>(HOST_HEALTH);
  }

  async prompt(params: PromptParams): Promise<void> {
    await this.request(SESSION_PROMPT, params);
  }

  async cancel(params: SessionCancelParams): Promise<void> {
    await this.request(SESSION_CANCEL, params);
  }

  async fold(params: ThreadFoldParams): Promise<void> {
    await this.request(THREAD_FOLD, params);
  }

  async replyPermission(params: PermissionReplyParams): Promise<void> {
    await this.request(PERMISSION_REPLY, params);
  }

  async resumeFrom(params: ResumeFromParams): Promise<ResumeFromResult> {
    return this.request<ResumeFromResult>(SYNC_RESUME_FROM, params);
  }

  private async request<T>(method: string, params?: unknown): Promise<T> {
    const request: JsonRpcRequest = {
      jsonrpc: JSONRPC_VERSION,
      id: this.nextId++,
      method,
    };
    if (params !== undefined) {
      request.params = params;
    }
    const response = await this.transport.request(request);
    if (response.error) {
      throw new HostRpcError(response.error);
    }
    return response.result as T;
  }
}

export async function connectHost(): Promise<{
  client: HostClient;
  hello: HelloResult;
}> {
  const client = new HostClient();
  try {
    await client.connect();
  } catch {
    // Preview / unit builds may not have a Tauri event bus. Requests still work
    // through `invoke` when the webview is hosted by the app.
  }
  const hello = await client.hello();
  return { client, hello };
}
