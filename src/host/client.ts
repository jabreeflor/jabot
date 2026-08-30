//! Transport-agnostic JSON-RPC client for the JaBot host protocol.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  CREW_CREATE,
  CREW_LIST,
  CREW_REMOVE,
  CREW_THREAD,
  CREW_UPDATE,
  FOLDER_FORGET,
  FOLDER_LIST,
  FOLDER_REGISTER,
  FOLDER_UPDATE,
  GITHUB_LOGIN,
  GITHUB_STATUS,
  HARNESS_DOCTOR,
  HARNESS_LIST,
  HOST_HEALTH,
  HOST_HELLO,
  HOST_RPC_EVENT,
  INBOX_LIST,
  JSONRPC_VERSION,
  NOTIFICATION_ACTIVATED_EVENT,
  NOTIFY_STATUS,
  PERMISSION_PENDING,
  PERMISSION_REPLY,
  PR_LIST,
  PR_MINE,
  PR_REFRESH,
  PROTOCOL_VERSION,
  SCHEDULE_CREATE,
  SCHEDULE_LIST,
  SCHEDULE_REMOVE,
  SCHEDULE_RUN,
  SCHEDULE_UPDATE,
  SESSION_CANCEL,
  SESSION_PROMPT,
  SETTINGS_GET,
  SETTINGS_SET,
  SUPERVISOR_STATUS,
  SYNC_RESUME_FROM,
  DEVICE_LIST,
  DEVICE_REVOKE,
  PAIRING_CANCEL,
  PAIRING_CLAIM,
  PAIRING_CONFIRM,
  PAIRING_START,
  PAIRING_STATUS,
  THREAD_ARCHIVE,
  THREAD_DELETE,
  THREAD_FOLD,
  THREAD_OPEN,
  THREAD_REOPEN,
  THREAD_RESUME,
  THREAD_STATE,
  THREAD_TRANSCRIPT,
  TOOLS_CONNECT,
  TOOLS_DISCONNECT,
  TOOLS_LIST,
  type BotView,
  type ScheduleCreateParams,
  type SettingsSetParams,
  type SettingsView,
  type ScheduleListResult,
  type ScheduleRefParams,
  type ScheduleRemoveResult,
  type ScheduleRunResult,
  type ScheduleUpdateParams,
  type ScheduleView,
  type CrewCreateParams,
  type CrewListResult,
  type CrewRefParams,
  type CrewRemoveResult,
  type CrewUpdateParams,
  type FolderForgetResult,
  type FolderListResult,
  type FolderRefParams,
  type FolderRegisterParams,
  type FolderUpdateParams,
  type FolderView,
  type GithubLoginParams,
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
  type NotificationActivated,
  type NotifyStatusResult,
  type JsonRpcResponse,
  type DeviceInfo,
  type PermissionPendingParams,
  type PermissionPendingResult,
  type PermissionReplyParams,
  type PermissionReplyResult,
  type PrListParams,
  type PrListResult,
  type PrMineParams,
  type PrMineResult,
  type PrRefreshParams,
  type PrRefreshResult,
  type PromptParams,
  type PromptResult,
  type DeviceListResult,
  type DeviceRefParams,
  type DeviceRevokeResult,
  type PairingCancelResult,
  type PairingClaimParams,
  type PairingClaimResult,
  type PairingConfirmParams,
  type PairingConfirmResult,
  type PairingRefParams,
  type PairingStartParams,
  type PairingStartResult,
  type PairingStatusResult,
  type ResumeFromParams,
  type ResumeFromResult,
  type SessionCancelParams,
  type SupervisorStatusResult,
  type ThreadFoldParams,
  type ThreadOpenParams,
  type ThreadRefParams,
  type ThreadResumeResult,
  type ThreadStateResult,
  type ThreadTranscriptParams,
  type ThreadTranscriptResult,
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
  private device: DeviceInfo | null = null;

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
    const result = await this.request<HelloResult>(HOST_HELLO, {
      ...params,
      protocolVersion: params.protocolVersion ?? PROTOCOL_VERSION,
    });
    // The host binds this connection to a device on hello, and several calls —
    // `permission/reply` above all — have to say which device is acting. Kept
    // here so a feature slice does not have to thread the handshake result
    // through every component that might answer something.
    this.device = result.device;
    return result;
  }

  /** The device the host bound this connection to, once hello has answered. */
  get deviceId(): string | null {
    return this.device?.deviceId ?? null;
  }

  async health(): Promise<HealthResult> {
    return this.request<HealthResult>(HOST_HEALTH);
  }

  /**
   * Send a turn. Throws `RUN_IN_FLIGHT` when one is already running unless
   * `mode` says what to do instead — `queue` holds it until the turn ends,
   * `interrupt` cancels the turn and then sends it (#14).
   */
  async prompt(params: PromptParams): Promise<PromptResult> {
    return this.request<PromptResult>(SESSION_PROMPT, params);
  }

  async cancel(params: SessionCancelParams): Promise<void> {
    await this.request(SESSION_CANCEL, params);
  }

  /** Replay a thread from our own store — never from harness JSONL (#14). */
  async threadTranscript(
    params: ThreadTranscriptParams,
  ): Promise<ThreadTranscriptResult> {
    return this.request<ThreadTranscriptResult>(THREAD_TRANSCRIPT, params);
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

  /**
   * Put a thread's ACP session back after a quit, a crash, or a lid close.
   *
   * Never `session/new`: that orphans the conversation. `outcome` says how far
   * it got — `resumed` / `loaded` attached it, `drifted` means the harness,
   * model, cwd, tools or permission mode moved and the stored session is no
   * longer this job, `cwd_missing` means the folder is gone.
   */
  async resumeThread(params: ThreadRefParams): Promise<ThreadResumeResult> {
    return this.request<ThreadResumeResult>(THREAD_RESUME, params);
  }

  /** What the supervisor is holding open, and what it reconciled at boot. */
  async supervisorStatus(): Promise<SupervisorStatusResult> {
    return this.request<SupervisorStatusResult>(SUPERVISOR_STATUS);
  }

  async inbox(params: InboxListParams = {}): Promise<InboxListResult> {
    return this.request<InboxListResult>(INBOX_LIST, params);
  }

  /** Whether a native notification can reach this user, and which Inbox kinds
      send one (#27). Informational only: a `denied` answer changes nothing
      about the Inbox, which is where every card lands regardless. */
  async notifyStatus(): Promise<NotifyStatusResult> {
    return this.request<NotifyStatusResult>(NOTIFY_STATUS);
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

  /** The crew, the shipped templates, and Chief's host tools — everything the
      Crew view and the bot editor draw, in one answer (#17). */
  async listCrew(): Promise<CrewListResult> {
    return this.request<CrewListResult>(CREW_LIST);
  }

  /** Add a bot. `templateId` copies a shipped pack's fields into the new row;
      it is a snapshot, so editing the bot later is unaffected by the pack. */
  async createBot(params: CrewCreateParams = {}): Promise<BotView> {
    return this.request<BotView>(CREW_CREATE, params);
  }

  /** Save the editor. An omitted field is left alone. */
  async updateBot(params: CrewUpdateParams): Promise<BotView> {
    return this.request<BotView>(CREW_UPDATE, params);
  }

  /** Remove a bot. Throws `CHIEF_REQUIRED` for Chief; every other bot goes,
      and its threads and its memory directory stay. */
  async removeBot(params: CrewRefParams): Promise<CrewRemoveResult> {
    return this.request<CrewRemoveResult>(CREW_REMOVE, params);
  }

  /** Open (or return) a bot's standing thread — the one conversation every
      non-Code crew member has, running in its memory directory with no
      worktree (#24, decision #6). Idempotent: the id is derived from the bot,
      so calling twice cannot make two threads. */
  async botThread(params: CrewRefParams): Promise<ThreadStateResult> {
    return this.request<ThreadStateResult>(CREW_THREAD, params);
  }

  /** Every app-wide preference, as it is actually in force (#26). */
  async settings(): Promise<SettingsView> {
    return this.request<SettingsView>(SETTINGS_GET);
  }

  /** Write what changed and get the whole view back — the host's answer is the
      state, so nothing here has to merge a patch into a guess. */
  async saveSettings(params: SettingsSetParams): Promise<SettingsView> {
    return this.request<SettingsView>(SETTINGS_SET, params);
  }

  /** Every recurring job, with its recent fires beside it (#25). */
  async listSchedules(): Promise<ScheduleListResult> {
    return this.request<ScheduleListResult>(SCHEDULE_LIST);
  }

  /** Add one. Throws `INVALID_PARAMS` naming the field when the cron does not
      parse or the bot does not exist — a schedule that can never run is worse
      than one that was refused, because nothing tells the user about it. */
  async createSchedule(params: ScheduleCreateParams): Promise<ScheduleView> {
    return this.request<ScheduleView>(SCHEDULE_CREATE, params);
  }

  /** Patch one. Editing the cron or the switch re-arms it from now; editing
      the prompt deliberately does not move a job that is due in ten minutes. */
  async updateSchedule(params: ScheduleUpdateParams): Promise<ScheduleView> {
    return this.request<ScheduleView>(SCHEDULE_UPDATE, params);
  }

  async removeSchedule(params: ScheduleRefParams): Promise<ScheduleRemoveResult> {
    return this.request<ScheduleRemoveResult>(SCHEDULE_REMOVE, params);
  }

  /** Run now. Its own occurrence, stamped with the moment the user asked: it
      does not consume or move the schedule's next due time. */
  async runSchedule(params: ScheduleRefParams): Promise<ScheduleRunResult> {
    return this.request<ScheduleRunResult>(SCHEDULE_RUN, params);
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

  /** The PR board: every pull request a session on this Mac opened (#28).
      A store read — it never touches the network, so a user with no GitHub
      login still gets their board. */
  async listPullRequests(params: PrListParams = {}): Promise<PrListResult> {
    return this.request<PrListResult>(PR_LIST, params);
  }

  /** Ask GitHub what those pull requests look like now. Resolves even when
      GitHub could not be reached — the reason is in `unavailable`, because a
      poll that throws every fifteen seconds takes the board down with it. */
  async refreshPullRequests(
    params: PrRefreshParams = {},
  ): Promise<PrRefreshResult> {
    return this.request<PrRefreshResult>(PR_REFRESH, params);
  }

  /** Whether the host can act as the user on GitHub, and as whom. Never
      carries a token: MVP auth is the user's own `gh` login (#16). */
  async githubStatus(
    params: GithubStatusParams = {},
  ): Promise<GithubStatusResult> {
    return this.request<GithubStatusResult>(GITHUB_STATUS, params);
  }

  /** Sign in: hand the host a token to give `gh`, and get back who it makes
      us. The one call that carries a secret, and it carries it one way — the
      token is never stored by JaBot and no method reads it back. Rejects when
      GitHub refused it, because a person is waiting at the dialog to be told
      whether their paste worked. */
  async githubLogin(params: GithubLoginParams): Promise<GithubStatusResult> {
    return this.request<GithubStatusResult>(GITHUB_LOGIN, params);
  }

  /** Every open pull request the signed-in user wrote, wherever it lives.
      Resolves even when GitHub could not be reached — the reason is in
      `unavailable`, exactly as for `refreshPullRequests`. */
  async myPullRequests(params: PrMineParams = {}): Promise<PrMineResult> {
    return this.request<PrMineResult>(PR_MINE, params);
  }

  /**
   * Answer an ask. Idempotent on the host: a second click comes back with
   * `alreadyAnswered` and whatever the first one decided, rather than an error
   * or a second answer reaching the agent (#20).
   */
  async replyPermission(
    params: PermissionReplyParams,
  ): Promise<PermissionReplyResult> {
    return this.request<PermissionReplyResult>(PERMISSION_REPLY, params);
  }

  /** Asks still waiting on a human — including ones a previous host took and
      never got an answer to. */
  async pendingPermissions(
    params: PermissionPendingParams = {},
  ): Promise<PermissionPendingResult> {
    return this.request<PermissionPendingResult>(PERMISSION_PENDING, params);
  }

  async resumeFrom(params: ResumeFromParams): Promise<ResumeFromResult> {
    return this.request<ResumeFromResult>(SYNC_RESUME_FROM, params);
  }

  /**
   * Put a pairing QR on the screen (#19). `full` devices only — the host
   * refuses this from a phone, which is the point of the role.
   *
   * `secret` and `code` come back exactly once, here. Losing them means
   * starting a new offer, not re-reading a live capability.
   */
  async startPairing(
    params: PairingStartParams = {},
  ): Promise<PairingStartResult> {
    return this.request<PairingStartResult>(PAIRING_START, params);
  }

  /**
   * Claim an offer as the *new* device. Answered before any `host/hello`,
   * because a device that is not paired yet cannot say hello — the
   * out-of-band secret stands in for one.
   *
   * The result deliberately carries no safety number: derive your own from
   * the transcript (see `PairingClaimParams`) and show that.
   */
  async claimPairing(params: PairingClaimParams): Promise<PairingClaimResult> {
    return this.request<PairingClaimResult>(PAIRING_CLAIM, params);
  }

  /** "The number on my screen is this one." Both sides must say it before
      the host writes the grant. */
  async confirmPairing(
    params: PairingConfirmParams,
  ): Promise<PairingConfirmResult> {
    return this.request<PairingConfirmResult>(PAIRING_CONFIRM, params);
  }

  /** Live offers, without their credentials — including the safety number to
      show the human once a device has claimed one. */
  async pairingStatus(): Promise<PairingStatusResult> {
    return this.request<PairingStatusResult>(PAIRING_STATUS);
  }

  async cancelPairing(params: PairingRefParams): Promise<PairingCancelResult> {
    return this.request<PairingCancelResult>(PAIRING_CANCEL, params);
  }

  /** The revoke list: every device this host ever admitted, tombstones and
      the local console included. */
  async listDevices(): Promise<DeviceListResult> {
    return this.request<DeviceListResult>(DEVICE_LIST);
  }

  /** Cut a device off. Durable before it is reported, and in force on that
      device's very next call rather than its next connection. */
  async revokeDevice(params: DeviceRefParams): Promise<DeviceRevokeResult> {
    return this.request<DeviceRevokeResult>(DEVICE_REVOKE, params);
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

/**
 * Clicking a native notification opens the thread it names (#27).
 *
 * Not a `HostClient` method: the click is an AppKit event the Tauri layer
 * forwards, not a JSON-RPC frame, so it must keep working on a client whose
 * transport is a socket rather than the webview bridge.
 *
 * Resolves to a no-op unsubscribe wherever there is no Tauri event bus — a
 * preview build, a unit test — so a caller never has to guess whether it is
 * running inside the app.
 */
export async function onNotificationActivated(
  handler: (activated: NotificationActivated) => void,
): Promise<UnlistenFn> {
  try {
    return await listen<NotificationActivated>(
      NOTIFICATION_ACTIVATED_EVENT,
      (event) => handler(event.payload),
    );
  } catch {
    return () => {};
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
