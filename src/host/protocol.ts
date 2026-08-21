//! Typed JSON-RPC 2.0 host protocol.
//! Keep in sync with `src-tauri/src/host/protocol`.

export const JSONRPC_VERSION = "2.0" as const;
export const PROTOCOL_VERSION = 1;
export const HOST_RPC_EVENT = "host-rpc";

export const HOST_HELLO = "host/hello";
export const HOST_HEALTH = "host/health";
export const SESSION_PROMPT = "session/prompt";
export const SESSION_CANCEL = "session/cancel";
export const SESSION_UPDATE = "session/update";
export const PERMISSION_ASK = "permission/ask";
export const PERMISSION_REPLY = "permission/reply";
export const PERMISSION_PENDING = "permission/pending";
export const PERMISSION_RESOLVED = "permission/resolved";
export const THREAD_FOLD = "thread/fold";
export const THREAD_OPEN = "thread/open";
export const THREAD_REOPEN = "thread/reopen";
export const THREAD_ARCHIVE = "thread/archive";
export const THREAD_DELETE = "thread/delete";
export const THREAD_STATE = "thread/state";
export const THREAD_TRANSCRIPT = "thread/transcript";
export const THREAD_RESUME = "thread/resume";
export const SUPERVISOR_STATUS = "supervisor/status";
export const INBOX_RESURFACE = "inbox/resurface";
export const INBOX_LIST = "inbox/list";
export const HARNESS_LIST = "harness/list";
export const HARNESS_DOCTOR = "harness/doctor";
export const TOOLS_LIST = "tools/list";
export const TOOLS_CONNECT = "tools/connect";
export const TOOLS_DISCONNECT = "tools/disconnect";
export const FOLDER_LIST = "folder/list";
export const FOLDER_REGISTER = "folder/register";
export const FOLDER_UPDATE = "folder/update";
export const FOLDER_FORGET = "folder/forget";
export const GITHUB_STATUS = "github/status";
export const CREW_LIST = "crew/list";
export const CREW_CREATE = "crew/create";
export const CREW_UPDATE = "crew/update";
export const CREW_REMOVE = "crew/remove";
export const CREW_THREAD = "crew/thread";
export const SCHEDULE_LIST = "schedule/list";
export const SCHEDULE_CREATE = "schedule/create";
export const SCHEDULE_UPDATE = "schedule/update";
export const SCHEDULE_REMOVE = "schedule/remove";
export const SCHEDULE_RUN = "schedule/run";
export const SYNC_RESUME_FROM = "sync/resumeFrom";
/** A new Inbox card on a thread that did *not* resurface — a schedule firing
    on an active standing thread moves nothing in the sidebar (#25). */
export const INBOX_EVENT = "inbox/event";

export type RequestId = number | string | null;

export type DeviceRole = "full" | "approver";

/** Why a folded thread came back. `failed` (retry) and `stuck` (wait or
    cancel, process still alive) are deliberately not the same card. */
export type ResurfaceReason = "done" | "failed" | "stuck" | "needs_you";

/** `threads.state`, plus the `deleted` tombstone the UI never lists. */
export type ThreadOverlayState =
  | "active"
  | "folded"
  | "resurfaced"
  | "archived"
  | "deleted";

/** "Wait for Inbox" is a permission policy on a folded thread — auto-allow
    reads, still ask for execute and delete — not a fifth overlay state. */
export type FoldPolicy = "default" | "wait_for_inbox";

/** The process axis, reported next to the overlay state and never folded into
    it: a folded thread that is still `running` is the whole feature. */
export type AcpState = "running" | "idle" | "requires_action" | "unknown";

export type RunLedgerState =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "timed_out"
  | "lost"
  | "needs_you";

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

export interface JsonRpcRequest<T = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: RequestId;
  method: string;
  params?: T;
}

export interface JsonRpcNotification<T = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  method: string;
  params?: T;
}

export interface JsonRpcResponse<T = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: RequestId;
  result?: T;
  error?: JsonRpcError;
}

export interface DeviceInfo {
  deviceId: string;
  name: string;
  role: DeviceRole;
  createdAt?: string;
}

export interface HelloDevice {
  deviceId?: string;
  name?: string;
  role?: DeviceRole;
}

export interface HelloParams {
  protocolVersion?: number;
  device?: HelloDevice;
  /** Required for a paired device, absent for the local console (#19).
      See `DeviceAuth`. */
  auth?: DeviceAuth;
}

export interface HelloResult {
  protocolVersion: number;
  hostId: string;
  hostName: string;
  hostMode: string;
  version: string;
  platform: string;
  device: DeviceInfo;
  methods: string[];
  notifications: string[];
  /** The subset of `methods` *this* device may call, per the role its
      `paired_devices` row carries (#19's scope, surfaced for #29).

      A phone could hard-code the approver list and the host would still be
      what enforces it — but then the client's idea of what it may do and the
      host's could drift apart in silence, and the symptom would be a button
      that exists and always fails. Empty from a host too old to answer;
      fall back to `methods` in that case. */
  scopedMethods?: string[];
  store?: StoreStatus;
  storeError?: string;
}

export interface StoreStatus {
  path: string;
  schemaVersion: number;
  sqliteVersion: string;
  journalMode: string;
  secretsBackend: string;
  harnessCount: number;
  botCount: number;
}

export interface HealthResult {
  version: string;
  platform: string;
  hostMode: string;
  hostId: string;
  protocolVersion: number;
  connected: boolean;
  deviceId?: string;
  store?: StoreStatus;
  storeError?: string;
}

export interface RuntimeSpec {
  command: string;
  args?: string[];
  env?: Record<string, string>;
  installHint?: string;
}

/**
 * Steer vs redispatch (#14): what a prompt does to a thread that is already
 * busy. ACP has no mid-turn steering primitive — one turn per session, and the
 * stop reason comes back on the response — so the honest choices are to wait
 * for the turn or to end it first.
 *
 * `reject` is the default on the wire as well as in the host: a client that
 * has not been taught about the queue cannot grow one by accident.
 */
export type PromptMode = "reject" | "queue" | "interrupt";

export interface PromptParams {
  threadId: string;
  content: unknown;
  mode?: PromptMode;
  cwd?: string;
  harnessId?: string;
  runtime?: RuntimeSpec;
}

export interface PromptResult {
  threadId: string;
  acpSessionId: string;
  /** The agent has it. False with `queued` means it is only held. */
  accepted: boolean;
  queued?: boolean;
  /** 1 = next out. Only meaningful while `queued`. */
  queuePosition?: number;
}

export interface ThreadTranscriptParams {
  threadId: string;
  /** Exclusive: only rows above this `seq`. */
  afterSeq?: number;
  /** Newest-N window, still returned oldest-first. */
  limit?: number;
}

/** One `transcript_events` row: the ACP notification exactly as it arrived, so
    a replay and the live stream reduce through the same mapper. */
export interface TranscriptEventView {
  seq: number;
  method: string;
  payload: unknown;
  createdAt: string;
}

/** A prompt the user has sent that the agent has not been given yet. */
export interface QueuedPromptView {
  position: number;
  content: unknown;
  queuedAt: string;
}

export interface ThreadTranscriptResult {
  threadId: string;
  /** The log's head, whether or not it is in `events`. A live client applies
      notifications whose `transcriptSeq` is above this and drops the rest. */
  headSeq: number;
  events: TranscriptEventView[];
  truncated: boolean;
  queued: QueuedPromptView[];
  /** The thread's *open* run, if it has one; absent once it ended. The replay
      is history and cannot say whether the turn it stops in is still going, so
      the ledger's answer travels with it — that is what lets a view mounted
      mid-turn offer Stop instead of reporting the previous turn's outcome. */
  runState?: RunLedgerState;
}

export interface SessionCancelResult {
  threadId: string;
  cancelled: boolean;
}

/**
 * What became of one answer.
 *
 * `delivered` is the load-bearing field and it is not always `true`: an ask
 * whose adapter died — or whose host was quit and restarted — is still
 * answerable and still recorded, but there is no live ACP call left to hand
 * the answer to.
 */
export interface PermissionReplyResult {
  requestId: string;
  delivered: boolean;
  /** Already resolved before this call — a second click, or one that raced the
      turn being cancelled. The fields below then describe what the *first*
      resolution decided, not what this call asked for. */
  alreadyAnswered: boolean;
  optionId?: string;
  cancelled: boolean;
}

/** An ask nobody has answered yet (#20). */
export interface PendingPermissionView {
  requestId: string;
  threadId: string;
  title: string;
  kind?: string;
  /** The ACP `toolCall` (or whole request) the agent sent, verbatim. */
  subject: unknown;
  /** The ACP options the agent offered, verbatim. A client renders these and
      nothing else: the host never invents an option the agent did not offer. */
  options: unknown;
  createdAt: string;
  /** No live adapter call is waiting on this one — the host that took the ask
      is gone. Answering records the decision; the agent never hears it. */
  stale: boolean;
}

export interface PermissionPendingParams {
  /** One thread, or every thread when absent. */
  threadId?: string;
}

export interface PermissionPendingResult {
  requests: PendingPermissionView[];
}

export interface SessionCancelParams {
  threadId: string;
}

export interface ThreadFoldParams {
  threadId: string;
  /** Omitted keeps the thread's current policy. */
  policy?: FoldPolicy;
}

/** Every lifecycle method that only needs to name a thread. */
export interface ThreadRefParams {
  threadId: string;
}

/** New Chat: the edge into the state machine. Idempotent. */
export interface ThreadOpenParams {
  threadId?: string;
  title: string;
  cwd: string;
  harnessId: string;
  runtime?: RuntimeSpec;
  folderId?: string;
  botId?: string;
  foldPolicy?: FoldPolicy;
  /** Work in the folder's own checkout instead of a fresh worktree (#23). The
      advanced opt-out, never the default: two threads sharing the user's tree is
      the collision worktrees exist to prevent. */
  useCheckout?: boolean;
  /** What the thread's branch starts from — a branch, tag or sha. Default is
      `origin/<default branch>`, never the user's possibly-dirty `HEAD`. */
  baseRef?: string;
}

export interface RunView {
  id: string;
  seq: number;
  kind: string;
  state: RunLedgerState;
  error?: string;
  acpSessionId?: string;
  startedAt?: string;
  endedAt?: string;
  createdAt: string;
}

/** The receipt #21 compares against on resume; `fingerprint` is the cheap
    equality check and the fields beside it say what drifted. */
export interface ReceiptView {
  acpSessionId: string;
  nativeSessionRef?: string;
  harnessId: string;
  model?: string;
  cwd: string;
  tools: string[];
  permissionMode: string;
  fingerprint: string;
  updatedAt: string;
}

export interface ProcessView {
  connected: boolean;
  acpState: AcpState;
  pendingPermissions: number;
  /** The adapter's pid while one is attached. Diagnostic only — decision #4's
      durability is resume, and a pid does not survive a lid close. */
  pid?: number;
  /** Could `thread/resume` put this conversation back? Needs a stored session,
      a receipt that still matches, and a `cwd` that still exists (#21). */
  resumable: boolean;
  /** Fields that have moved since the session was created, by wire name.
      Non-empty means the stored session is no longer this job, so the next
      prompt starts a new one instead of continuing someone else's. */
  drift?: string[];
}

/** What `thread/resume` managed to do. */
export type ResumeOutcome =
  /** The adapter was still there; nothing needed restoring. */
  | "live"
  /** ACP `session/resume` — context back, no replay. */
  | "resumed"
  /** ACP `session/load` — the agent replayed its history to us. */
  | "loaded"
  /** The receipt no longer matches; resuming would continue a different job. */
  | "drifted"
  /** This thread has never had an ACP session to resume. */
  | "no_session"
  /** The adapter speaks neither `session/resume` nor `session/load`. */
  | "unsupported"
  /** The directory the session was created in is gone. */
  | "cwd_missing";

export interface ThreadResumeResult {
  threadId: string;
  /** True only when a conversation is attached — `outcome` says which way. */
  resumed: boolean;
  outcome: ResumeOutcome;
  acpSessionId?: string;
  drift?: string[];
  /** One sentence to show. Present whenever the outcome is not a plain
      success: "could not resume" with no reason sends users to the wrong fix. */
  detail?: string;
  /** The thread as it stands afterwards, so a resume is one round trip. */
  state: ThreadStateResult;
}

/** One adapter the supervisor is holding open. */
export interface LiveAdapterView {
  threadId: string;
  pid: number;
  harnessId: string;
  acpSessionId?: string;
  acpState: AcpState;
  /** Milliseconds since the adapter last said anything. */
  idleMs: number;
  pendingPermissions: number;
  /** Which adapter processes may be shared (#13). Two live threads with the
      same key could have been one process; today they are not, and this says
      so out loud rather than implying pooling that does not exist. */
  profileKey: string;
}

/** What the boot pass did to one run left open by a host that stopped. */
export interface BootNoteView {
  threadId: string;
  runId?: string;
  /** The run state the previous host left behind. */
  was: RunLedgerState;
  /** What it was moved to. Always terminal: nothing is reporting on it. */
  now: RunLedgerState;
  resurfacedAs?: ResurfaceReason;
  detail: string;
}

export interface SupervisorStatusResult {
  hostId: string;
  /** When this host process came up. Every `boot` note belongs to it. */
  bootedAt: string;
  liveAdapters: LiveAdapterView[];
  /** The reconciliation this launch performed — RAM, and rightly so: its
      durable half is already in `runs` and `inbox_events`. */
  boot: BootNoteView[];
  /** Grace before an idle adapter on an unwatched thread is closed. 0 = off. */
  idleEvictAfterMs: number;
  /** Unaccounted wall time that counts as a machine sleep. */
  sleepGapThresholdMs: number;
  /** Sleeps this host has noticed since it started. */
  sleepsObserved: number;
}

export interface ThreadStateResult {
  threadId: string;
  title: string;
  state: ThreadOverlayState;
  foldPolicy: FoldPolicy;
  resurfacedReason?: ResurfaceReason;
  cwd: string;
  /** The spawn record (#16, setup-porting §19): where this thread works,
      stamped when it was opened and never re-derived. It outlives the folder it
      was copied from — a thread whose folder has been forgotten still knows its
      checkout. */
  repoRoot?: string;
  /** `owner/name`. */
  repo?: string;
  forgeHost?: string;
  branch?: string;
  /** The host-owned git worktree this thread works in (#23), when it has one.
      Absent for every thread that is not a code thread — a worker's standing
      thread, a folder that is not a checkout, the "use my own checkout" opt-out
      — and absent again once the thread has been archived or deleted and its
      tree collected. */
  worktreePath?: string;
  /** Which machine opened it. One host in MVP1; recorded so a second never
      has to guess. */
  hostId?: string;
  harnessId: string;
  folderId?: string;
  botId?: string;
  acpSessionId?: string;
  lastStopReason?: string;
  lastError?: string;
  foldedAt?: string;
  resurfacedAt?: string;
  archivedAt?: string;
  deletedAt?: string;
  process: ProcessView;
  latestRun?: RunView;
  runs: RunView[];
  receipt?: ReceiptView;
  /** The most recent handoff onto this thread (#24). Absent for every thread
      the human started themselves, which is most of them. */
  handoff?: HandoffView;
  unread: number;
}

/** How a bot dispatched work: onto a crew member's standing thread, or into a
    fresh coding thread in a folder. */
export type HandoffKind = "handoff" | "code_session";

/** Where a thread's work came from, when a bot sent it rather than the human
    (#24). Chief routes by handing off, and without this the human reading that
    thread tomorrow cannot tell whether they asked for it or Chief did. */
export interface HandoffView {
  handoffId: string;
  kind: HandoffKind;
  task: string;
  context?: string;
  fromThreadId?: string;
  fromBotId?: string;
  /** Resolved for display. Absent once the sending bot has been removed — the
      trail survives the crew member. */
  fromBotName?: string;
  /** Whether the task actually reached an agent. A handoff to a bot whose
      harness is not installed is still a real handoff; this is what says
      nobody heard it, and `detail` says why. */
  dispatched: boolean;
  detail?: string;
  createdAt: string;
}

export interface InboxListParams {
  limit?: number;
  includeDismissed?: boolean;
}

export interface InboxEventView {
  id: string;
  threadId: string;
  threadTitle: string;
  threadState: ThreadOverlayState;
  kind: string;
  title: string;
  summary: string;
  runId?: string;
  payload?: unknown;
  createdAt: string;
  readAt?: string;
  dismissedAt?: string;
}

/** Still Sleeping is a projection of `threads.state = folded`, not an event. */
export interface SleepingThreadView {
  threadId: string;
  title: string;
  foldPolicy: FoldPolicy;
  foldedAt?: string;
  runState?: RunLedgerState;
  acpState: AcpState;
}

export interface InboxListResult {
  events: InboxEventView[];
  sleeping: SleepingThreadView[];
  unread: number;
}

/** Which tier of the catalog a card came from (#13): compiled-in card,
    compiled-in preset, or user JSON. Tiers 1 and 2 have reserved ids. */
export type HarnessTier = "shipped" | "preset" | "custom";

/** How many chats one adapter process may carry. Hermes multiplexes chats onto
    one process per profile; Claude and Codex get a process per thread. */
export type SessionScope = "thread" | "profile";

/** Why a harness is not ready. Each value is a different fix — which is the
    whole point of the Doctor, since "not installed" is wrong five times in six. */
export type HarnessStatus =
  | "ready"
  | "cli_missing"
  | "adapter_missing"
  | "adapter_outdated"
  | "logged_out"
  | "invalid_config"
  | "daemon_not_running"
  | "unknown";

/** A catalog row as a New Chat / crew-editor card. */
export interface HarnessCardView {
  id: string;
  label: string;
  blurb: string;
  /** Accent colour token, e.g. `var(--h-claude)`. */
  accent: string;
  tier: HarnessTier;
  command: string;
  args: string[];
  installHint?: string;
  installUrl?: string;
  sessionScope: SessionScope;
  /** Reserved ids cannot be shadowed by a user file. */
  reserved: boolean;
}

/** A tier-3 file that did not make it into the catalog, and why. */
export interface CatalogIssue {
  file: string;
  reason: string;
}

export interface HarnessListResult {
  harnesses: HarnessCardView[];
  issues: CatalogIssue[];
}

export interface HarnessDoctorParams {
  /** Probe one card instead of the catalog. */
  harnessId?: string;
  /** Also spawn each ready adapter and run the ACP handshake — the only way
      to learn which protocol version it really speaks. */
  deep?: boolean;
}

export interface HarnessReport {
  id: string;
  label: string;
  tier: HarnessTier;
  status: HarnessStatus;
  ready: boolean;
  /** One sentence naming what was found, in the user's terms. */
  detail: string;
  remedy?: string;
  /** The absolute path that resolved, when one did. */
  command?: string;
  args: string[];
  installHint?: string;
  installUrl?: string;
  elapsedMs: number;
}

export interface HarnessDoctorResult {
  reports: HarnessReport[];
  issues: CatalogIssue[];
  /** The PATH the probes searched. "It works in my terminal" is a PATH the app
      never inherited, and this is how the user can see the difference. */
  path: string[];
}

/** How a catalog tool reaches its provider (#18). `harness_execute` is
    Terminal, and it can never become an `mcpServers` entry. */
export type ToolTransport = "http" | "stdio" | "harness_execute";

/** What the bot editor's chip says. Each value is a different next action,
    which is why "not working" is not one of them. */
export type ToolConnectionStatus =
  | "connected"
  | "needs_auth"
  | "connecting"
  | "error"
  | "missing";

/** A catalog entry with today's connection status. */
export interface ToolCardView {
  id: string;
  label: string;
  blurb: string;
  transport: ToolTransport;
  /** False for Terminal: allowlisting it can never produce an MCP server. */
  mcp: boolean;
  /** The grant this tool draws on — Gmail, Calendar and Drive share one. */
  provider?: string;
  providerLabel?: string;
  scopes: string[];
  status: ToolConnectionStatus;
  /** One sentence for the chip: which account, or what went wrong. */
  detail?: string;
  account?: string;
  expiresAt?: string;
  /** Only while a consent window is open — the page to send the user to. */
  authorizeUrl?: string;
  redirectUri?: string;
  docsUrl: string;
}

export interface ToolListResult {
  tools: ToolCardView[];
}

export interface ToolRefParams {
  toolId: string;
}

/** `tools/connect` returns as soon as the flow is running, not when the user
    has finished signing in. Poll `tools/list` for `authorizeUrl` and for the
    outcome. */
export interface ToolConnectResult {
  toolId: string;
  provider: string;
  status: ToolConnectionStatus;
  authorizeUrl?: string;
  redirectUri: string;
  /** The other chips this one grant covers. */
  affects: string[];
}

export interface ToolDisconnectResult {
  toolId: string;
  provider: string;
  disconnected: boolean;
  /** Every chip that just lost its grant: there was only ever one login. */
  affects: string[];
}

/** A folder's `origin`, split the way `gh` splits it (#16). Absent when the
    directory has no remote, or one no forge claims — both still work as
    folders; only the PR surface skips them. */
export interface FolderOriginView {
  url: string;
  /** `github.com`, a GHES hostname, `gitlab.com`. Never assumed. */
  host: string;
  owner: string;
  name: string;
  /** `owner/name` — one spelling for `gh --repo`, `thread_prs.repo` and the PR
      view, so they cannot disagree about what this repository is called. */
  repo: string;
}

/** A sidebar row under a folder: the `threads` columns the list needs, plus
    the state of the latest run. */
export interface FolderThreadView {
  threadId: string;
  folderId?: string;
  botId?: string;
  harnessId: string;
  title: string;
  state: ThreadOverlayState;
  foldPolicy: FoldPolicy;
  runState?: RunLedgerState;
  preview?: string;
}

/** One registered local directory (#16) — a repo, not a group of them. */
export interface FolderView {
  folderId: string;
  /** Ours to display and rename. The directory keeps its own name. */
  name: string;
  path: string;
  /** What a thread in this folder starts in: the repository root when there is
      one, else the registered path. Resolved by the host so the renderer does
      not re-derive the rule, and so #23 has one thing to swap for a worktree. */
  cwd: string;
  repoRoot?: string;
  /** False for a directory git does not claim. Legal: threads run, the PR view
      skips it, and the sidebar says so. */
  isGit: boolean;
  origin?: FolderOriginView;
  defaultBranch?: string;
  /** Optional per-folder setup for a fresh worktree (#23 runs it). */
  setupCommand?: string;
  /** Gitignored files a fresh worktree needs — `.env` and friends (#23). */
  filesToCopy: string[];
  sortOrder: number;
  /** Active and resurfaced threads only: a folded thread is not listed, which
      is the promise fold makes. */
  threads: FolderThreadView[];
}

export interface FolderListResult {
  folders: FolderView[];
}

/** Register a directory the user picked. The host probes git once, here. */
export interface FolderRegisterParams {
  /** Absolute, or `~`-relative. The host canonicalises it. */
  path: string;
  /** Defaults to the directory's basename, and stays editable after. */
  name?: string;
  setupCommand?: string;
  filesToCopy?: string[];
}

/** A patch: an omitted field is left alone, an empty `setupCommand` clears it. */
export interface FolderUpdateParams {
  folderId: string;
  name?: string;
  setupCommand?: string;
  filesToCopy?: string[];
  /** Ask git again — a remote added or re-pointed since registration. */
  refresh?: boolean;
}

export interface FolderRefParams {
  folderId: string;
}

/** Forgetting a folder removes the sidebar row, never the directory. */
export interface FolderForgetResult {
  folderId: string;
  forgotten: boolean;
  /** Threads that lost their folder and kept everything else — their cwd and
      their repo were stamped on them at spawn. */
  detachedThreads: number;
}

export interface GithubStatusParams {
  /** Defaults to `github.com`. GHES folders pass their `origin` host. */
  host?: string;
}

/** Whether the host can act as the user on GitHub, and as whom.
 *
 * There is no token here and there never will be: MVP auth is the user's own
 * `gh` login, read on demand by the host (#16). `installed` and `authenticated`
 * are separate because they have different remedies. */
export interface GithubStatusResult {
  installed: boolean;
  authenticated: boolean;
  host: string;
  account?: string;
  detail: string;
  remedy?: string;
  /** Where `gh` resolved from, so "it works in my terminal" is comparable. */
  ghPath?: string;
}

/** A `bots` row as the crew grid and the bot editor see it (#17).
 *
 * `tools` is the parsed allowlist, not the stored JSON text. The editor **is**
 * this record: what it saves is what the next session is spawned with. */
export interface BotView {
  botId: string;
  name: string;
  /** A colour token the renderer can render — `BotColor` in `components/types`.
      The host keeps the vocabulary closed, so this cast is safe. */
  color: string;
  /** Persona / system prompt, also mirrored to `instructions.md` in the bot's
      memory directory where the session can read it. */
  instructions: string;
  /** MCP catalog ids, plus host-tool ids for Chief (#6, #18). */
  tools: string[];
  harnessId: string;
  /** Exactly one bot has this, it is seeded, and `crew/remove` refuses it. */
  isChief: boolean;
  /** Which pack's fields were copied when this bot was added. History, not a
      link: nothing ever reads back through it. */
  templateId?: string;
  /** The bot's own directory — `instructions.md`, `MEMORY.md`, and the cwd a
      worker's standing thread runs in. Absent on a host with no data dir. */
  memoryDir?: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

/** A shipped template pack: a bot record without an identity (#6). Adding one
    copies these fields; there is no live link back. */
export interface BotTemplateView {
  templateId: string;
  name: string;
  color: string;
  instructions: string;
  tools: string[];
  harnessId: string;
}

/** One of Chief's host tools (#6). Not MCP, so not in `tools/list` — but the
    crew grid still has to name it rather than print `handoff_to_bot`. */
export interface CrewHostToolView {
  id: string;
  label: string;
  blurb: string;
}

export interface CrewListResult {
  bots: BotView[];
  templates: BotTemplateView[];
  hostTools: CrewHostToolView[];
}

/** Add a bot. Every field is optional: a `templateId` supplies whatever the
    caller does not, and the result is a snapshot either way. */
export interface CrewCreateParams {
  templateId?: string;
  name?: string;
  color?: string;
  instructions?: string;
  tools?: string[];
  harnessId?: string;
}

/** A patch: an omitted field is left alone, so changing a harness cannot
    silently discard the instructions the user typed. */
export interface CrewUpdateParams {
  botId: string;
  name?: string;
  color?: string;
  instructions?: string;
  tools?: string[];
  harnessId?: string;
}

export interface CrewRefParams {
  botId: string;
}

/** Removing a bot takes the row, never the directory: its markdown memory
    outlives it, the same way forgetting a folder leaves the checkout alone. */
export interface CrewRemoveResult {
  botId: string;
  removed: boolean;
  /** Threads that lost their bot and kept everything else — their cwd,
      harness and runtime were stamped on them at spawn. */
  detachedThreads: number;
  memoryDir?: string;
}

export interface PermissionReplyParams {
  requestId: string;
  deviceId: string;
  optionId?: string;
  cancelled?: boolean;
}

export interface ResumeFromParams {
  threadId: string;
  seq: number;
}

export interface LoggedEvent {
  seq: number;
  method: string;
  params: unknown;
}

export interface ResumeFromResult {
  threadId: string;
  headSeq: number;
  events: LoggedEvent[];
}

export interface Envelope {
  hostId: string;
  threadId: string;
  seq: number;
}

export interface SessionUpdateParams extends Envelope {
  acp: unknown;
  /** Where this event landed in `transcript_events`. `seq` on the envelope
      orders notifications; this orders the durable log, and only this one can
      be compared with `thread/transcript`'s `headSeq`. Absent on a host with
      no store — which is also a host with nothing to hydrate from. */
  transcriptSeq?: number;
}

export interface PermissionAskParams extends Envelope {
  requestId: string;
  subject: unknown;
  options: unknown;
}

export interface PermissionResolvedParams extends Envelope {
  requestId: string;
  deviceId: string;
  optionId?: string;
  cancelled?: boolean;
}

export interface InboxResurfaceParams extends Envelope {
  reason: ResurfaceReason;
}

export interface InboxEventParams extends Envelope {
  /** `inbox_events.kind` — `done`, `failed`, and the rest of the closed list. */
  kind: string;
  title: string;
  summary: string;
}

// ---- Schedules (#25) --------------------------------------------------
//
// A schedule belongs to a bot and runs on that bot's standing thread. `cron` is
// evaluated in the Mac's *local* time — "9am" has to keep meaning 9am after the
// clocks change — while every timestamp below is UTC, like every other
// timestamp this host emits.

/** What happens to an occurrence whose time passed while JaBot was closed.
    `once` runs the most recent one and drops the rest; `skip` runs none. There
    is deliberately no policy that replays a backlog. */
export type CatchUpPolicy = "once" | "skip";

/** `schedule_fires.state`. `dispatched` is the only non-final one. */
export type ScheduleFireState =
  | "dispatched"
  | "skipped"
  | "failed"
  | "delivered";

export interface ScheduleFireView {
  fireId: string;
  scheduleId: string;
  threadId?: string;
  runId?: string;
  /** The occurrence this fire is for. */
  dueAt: string;
  /** When the host actually got to it. Hours later than `dueAt` if the Mac was
      shut in between — which is the only place catch-up is visible. */
  firedAt: string;
  state: ScheduleFireState;
  caughtUp: boolean;
  /** Occurrences dropped in favour of (or alongside) this one. */
  skippedCount: number;
  detail?: string;
  deliveredAt?: string;
}

export interface ScheduleView {
  scheduleId: string;
  botId: string;
  botName: string;
  name: string;
  cron: string;
  prompt: string;
  enabled: boolean;
  catchUp: CatchUpPolicy;
  /** Absent for a disabled schedule: it owes nothing. */
  nextRunAt?: string;
  lastRunAt?: string;
  /** The bot's standing thread, once a fire has opened one. */
  threadId?: string;
  lastFire?: ScheduleFireView;
  recentFires: ScheduleFireView[];
  createdAt: string;
  updatedAt: string;
}

export interface ScheduleListResult {
  schedules: ScheduleView[];
}

export interface ScheduleCreateParams {
  botId: string;
  name: string;
  /** 5-field cron, 6 with a leading seconds field, or an `@daily` shorthand.
      Refused at the door if it does not parse. */
  cron: string;
  prompt: string;
  enabled?: boolean;
  catchUp?: CatchUpPolicy;
}

/** A patch: an omitted field is left alone, so flipping the switch cannot
    silently discard the prompt the user typed. */
export interface ScheduleUpdateParams {
  scheduleId: string;
  name?: string;
  cron?: string;
  prompt?: string;
  enabled?: boolean;
  catchUp?: CatchUpPolicy;
}

export interface ScheduleRefParams {
  scheduleId: string;
}

export interface ScheduleRemoveResult {
  scheduleId: string;
  removed: boolean;
}

export interface ScheduleRunResult {
  scheduleId: string;
  fire: ScheduleFireView;
}

export const RPC_ERROR = {
  PARSE_ERROR: -32700,
  INVALID_REQUEST: -32600,
  METHOD_NOT_FOUND: -32601,
  INVALID_PARAMS: -32602,
  INTERNAL_ERROR: -32603,
  PROTOCOL_MISMATCH: -32000,
  UNIMPLEMENTED: -32001,
  HELLO_REQUIRED: -32002,
  UNPAIRED_DEVICE: -32003,
  HARNESS_UNAVAILABLE: -32004,
  ILLEGAL_TRANSITION: -32005,
  THREAD_NOT_FOUND: -32006,
  STORE_UNAVAILABLE: -32007,
  /** A prompt arrived while the thread's run was still in flight (#15). */
  RUN_IN_FLIGHT: -32008,
  /** This directory, or the checkout it belongs to, is already a folder (#16).
      `data.folderId` is the one that already has it. */
  FOLDER_EXISTS: -32009,
  /** Chief is the one crew seat the product assumes exists (#6, #17).
      `data.botId` is the bot that was refused. */
  CHIEF_REQUIRED: -32010,
  /** The thread's worktree could not be created (#23), so the thread was not
      opened at all. New Chat keeps the draft: the way through is a different
      base ref, or `useCheckout` to work in the folder itself. */
  WORKTREE_FAILED: -32011,
  /** The thread's working directory is gone — unmounted, moved, or a worktree
      removed under a folded thread (#21). `session/prompt` refuses rather than
      spawning the agent in whatever directory JaBot itself was launched from.
      `data.cwd` is the path that is missing. */
  CWD_MISSING: -32012,
  /** The pairing handshake did not complete (#19). `data.reason` is coarse on
      purpose — `unknown`, `expired`-shaped, `credential`, `proof`, `claimed`,
      `sas`, `vault` — so the host cannot be used as an oracle while somebody
      grinds an eight-character code. `sas` is the one to draw loudly: the two
      safety numbers disagreed, which is what a man in the middle looks like. */
  PAIRING_FAILED: -32013,
  /** A paired device called something its role does not cover (#19). The role
      comes from the host's `paired_devices` row, checked on every request, so
      this can also appear mid-session after a device is narrowed or revoked.
      `data.role` and `data.method` say which. */
  DEVICE_SCOPE: -32014,
} as const;

// ---- Device pairing (#19) --------------------------------------------
//
// The handshake lives in `src-tauri/src/host/pairing/`. Its module docs are
// the authority on what it guarantees; the derivations a client needs are on
// `PairingClaimParams` below and in `protocol/methods.rs`.

export const PAIRING_START = "pairing/start";
export const PAIRING_CLAIM = "pairing/claim";
export const PAIRING_CONFIRM = "pairing/confirm";
export const PAIRING_CANCEL = "pairing/cancel";
export const PAIRING_STATUS = "pairing/status";
export const DEVICE_LIST = "device/list";
export const DEVICE_REVOKE = "device/revoke";

/** Proof that a paired device holds the token its pairing derived.

    `mac = HMAC-SHA256(deviceToken, H["jabot/hello/v1", hostId, deviceId,
    protocolVersion, counter])`, hex. `counter` must be strictly greater than
    the last one the host accepted for this device — the replay guard, and the
    reason a captured frame is not a credential. Absent for the local console,
    which is implicitly paired because it spawned the host. */
export interface DeviceAuth {
  counter: number;
  mac: string;
}

export interface PairingStartParams {
  /** Seconds the offer stays scannable. The host clamps it. */
  ttlSecs?: number;
}

/** Returned exactly once, by the call that creates the offer. No list method
    ever hands `secret` or `code` back — a live capability is not something to
    be able to re-read. */
export interface PairingStartResult {
  pairingId: string;
  hostId: string;
  hostName: string;
  hostFingerprint: string;
  hostNonce: string;
  secret: string;
  /** Eight Crockford characters, for a host with no screen to draw a QR on. */
  code: string;
  expiresAt: string;
  /** The exact string to encode in the QR. */
  qrPayload: string;
}

/** What `qrPayload` parses to. */
export interface PairingQr {
  v: number;
  hostId: string;
  hostName: string;
  hostFingerprint: string;
  pairingId: string;
  hostNonce: string;
  secret: string;
  addrs: string[];
}

export interface PairingDevice {
  deviceId: string;
  name: string;
  /** A commitment to the device's own long-term key material — the material
      itself is never sent, and the host never needs it. */
  fingerprint: string;
  nonce: string;
}

/** Claim an offer with the out-of-band credential and a proof.

    `H[a, b, …]` is SHA-256 over each field written as a 4-byte big-endian
    length followed by its UTF-8 bytes; the framing stops one field absorbing
    another's characters.

    ```text
    transcript = hex(H["jabot/pairing/v1", hostId, hostFingerprint, hostNonce,
                       pairingId, deviceId, deviceFingerprint, deviceNonce, via])
    key        = secret (via = "qr")  |  normalized code (via = "code")
    bind(d)    = HMAC-SHA256(key, H[d, transcript])
    mac        = hex(bind("jabot/pairing/claim/v1"))
    hostMac    = hex(bind("jabot/pairing/host/v1"))
    confirmMac = hex(bind("jabot/pairing/confirm/v1"))
    sas        = eight decimal digits of bind("jabot/pairing/sas/v1"), "NNNN-NNNN"
    token      = base64url(bind("jabot/pairing/device-token/v1"))
    ```

    A client MUST derive its own `sas` and show that. The host deliberately
    does not return one: a number only one side computed proves nothing about
    the other side. */
export interface PairingClaimParams {
  pairingId: string;
  secret?: string;
  code?: string;
  device: PairingDevice;
  mac: string;
}

export interface PairingClaimResult {
  pairingId: string;
  hostId: string;
  hostName: string;
  hostFingerprint: string;
  hostNonce: string;
  /** The host's own proof of holding the out-of-band credential. Verify it:
      it is what says the far end is the machine whose screen you scanned. */
  hostMac: string;
  via: string;
  expiresAt: string;
  state: string;
}

export type PairingSide = "host" | "device";

export interface PairingConfirmParams {
  pairingId: string;
  side: PairingSide;
  /** The number on *this* screen. The host refuses unless both sides' numbers
      agree with each other and with its own. */
  sas: string;
  /** Host side only: the scope granted. `approver` if unsaid. A device never
      chooses its own. */
  role?: DeviceRole;
  /** Host side only: rename the device as it is admitted. */
  name?: string;
  /** Device side only: `confirmMac`. */
  mac?: string;
}

export interface PairingConfirmResult {
  pairingId: string;
  /** `awaiting_device` | `awaiting_host` | `paired`. */
  state: string;
  device?: DeviceInfo;
}

export interface PairingRefParams {
  pairingId: string;
}

export interface PairingCancelResult {
  pairingId: string;
  cancelled: boolean;
}

export interface PairingOfferView {
  pairingId: string;
  state: string;
  expiresAt: string;
  attempts: number;
  hostConfirmed: boolean;
  deviceConfirmed: boolean;
  /** The number to put on the host's screen, once a device has claimed. */
  sas?: string;
  device?: PairingDevice;
  via?: string;
}

export interface PairingStatusResult {
  offers: PairingOfferView[];
}

export interface PairedDeviceView {
  deviceId: string;
  name: string;
  role: DeviceRole;
  fingerprint: string;
  pairedVia: string;
  sas: string;
  createdAt: string;
  lastSeenAt?: string;
  revokedAt?: string;
  /** The console that spawned this host. Implicitly paired, never revocable. */
  local: boolean;
  connected: boolean;
}

export interface DeviceListResult {
  devices: PairedDeviceView[];
}

export interface DeviceRefParams {
  deviceId: string;
}

export interface DeviceRevokeResult {
  deviceId: string;
  /** `false` when it was already revoked — the caller's intent still holds. */
  revoked: boolean;
  revokedAt?: string;
}
