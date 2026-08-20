//! Transport-agnostic JSON-RPC client for the JaBot host protocol.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  FOLDER_FORGET,
  FOLDER_LIST,
  FOLDER_REGISTER,
  FOLDER_UPDATE,
  GITHUB_STATUS,
  HARNESS_DOCTOR,
  HARNESS_LIST,
  HOST_HEALTH,
  HOST_HELLO,
  HOST_RPC_EVENT,
  INBOX_LIST,
  JSONRPC_VERSION,
  PERMISSION_REPLY,
  PROTOCOL_VERSION,
  SESSION_CANCEL,
  SESSION_PROMPT,
  SYNC_RESUME_FROM,
  THREAD_ARCHIVE,
  THREAD_DELETE,
  THREAD_FOLD,
  THREAD_OPEN,
  THREAD_REOPEN,
  THREAD_STATE,
  TOOLS_CONNECT,
  TOOLS_DISCONNECT,
  TOOLS_LIST,
  type FolderForgetResult,
  type FolderListResult,
  type FolderRefParams,
  type FolderRegisterParams,
  type FolderUpdateParams,
  type FolderView,
  type GithubStatusParams,
  type GithubStatusResult,
  type HarnessDoctorParams,
  type HarnessDoctorResult,
  type HarnessListResult,
  type HealthResult,
  type HelloParams,
  type HelloResult,
  type JsonRpcError,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type InboxListParams,
  type InboxListResult,
  type JsonRpcResponse,
  type PermissionReplyParams,
  type PromptParams,
  type ResumeFromParams,
  type ResumeFromResult,
  type SessionCancelParams,
  type ThreadFoldParams,
  type ThreadOpenParams,
  type ThreadRefParams,
  type ThreadStateResult,
  type ToolConnectResult,
  type ToolDisconnectResult,
  type ToolListResult,
  type ToolRefParams,
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

  /** New Chat. Idempotent — reopening the same id returns the same thread. */
  async openThread(params: ThreadOpenParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_OPEN, params);
  }

  /** Hide the thread and keep the subprocess. Never closes the ACP session. */
  async fold(params: ThreadFoldParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_FOLD, params);
  }

  async reopenThread(params: ThreadRefParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_REOPEN, params);
  }

  async archiveThread(params: ThreadRefParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_ARCHIVE, params);
  }

  async deleteThread(params: ThreadRefParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_DELETE, params);
  }

  async threadState(params: ThreadRefParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(THREAD_STATE, params);
  }

  async inbox(params: InboxListParams = {}): Promise<InboxListResult> {
    return this.request<InboxListResult>(INBOX_LIST, params);
  }

  /** The New Chat / crew-editor catalog. Cheap: no probing, so opening the
      picker never waits on a vendor CLI. */
  async listHarnesses(): Promise<HarnessListResult> {
    return this.request<HarnessListResult>(HARNESS_LIST);
  }

  /** Why each harness is or is not ready. Probes run concurrently in the host. */
  async harnessDoctor(
    params: HarnessDoctorParams = {},
  ): Promise<HarnessDoctorResult> {
    return this.request<HarnessDoctorResult>(HARNESS_DOCTOR, params);
  }

  /** The MCP catalog with each entry's connection status (#18). */
  async listTools(): Promise<ToolListResult> {
    return this.request<ToolListResult>(TOOLS_LIST);
  }

  /** Start an OAuth flow. Returns immediately: consent happens in the user's
      browser, so poll `listTools` for `authorizeUrl` and for the outcome. */
  async connectTool(params: ToolRefParams): Promise<ToolConnectResult> {
    return this.request<ToolConnectResult>(TOOLS_CONNECT, params);
  }

  /** Forget the grant behind this tool — and every tool that shared it. */
  async disconnectTool(params: ToolRefParams): Promise<ToolDisconnectResult> {
    return this.request<ToolDisconnectResult>(TOOLS_DISCONNECT, params);
  }

  /** Every registered folder with the threads the sidebar draws under it —
      the join the host owns, so the renderer never assembles one (#16). */
  async listFolders(): Promise<FolderListResult> {
    return this.request<FolderListResult>(FOLDER_LIST);
  }

  /** Register a directory. Throws `FOLDER_EXISTS` when this checkout is
      already a folder; `data.folderId` is the one that already has it. */
  async registerFolder(params: FolderRegisterParams): Promise<FolderView> {
    return this.request<FolderView>(FOLDER_REGISTER, params);
  }

  /** Rename, edit the setup script or files-to-copy, or re-probe git. */
  async updateFolder(params: FolderUpdateParams): Promise<FolderView> {
    return this.request<FolderView>(FOLDER_UPDATE, params);
  }

  /** Remove the sidebar row. Never the directory, never the threads. */
  async forgetFolder(params: FolderRefParams): Promise<FolderForgetResult> {
    return this.request<FolderForgetResult>(FOLDER_FORGET, params);
  }

  /** Whether the host can act as the user on GitHub, and as whom. Never
      carries a token: MVP auth is the user's own `gh` login (#16). */
  async githubStatus(params: GithubStatusParams = {}): Promise<GithubStatusResult> {
    return this.request<GithubStatusResult>(GITHUB_STATUS, params);
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
