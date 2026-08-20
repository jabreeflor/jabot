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
export const PERMISSION_RESOLVED = "permission/resolved";
export const THREAD_FOLD = "thread/fold";
export const INBOX_RESURFACE = "inbox/resurface";
export const SYNC_RESUME_FROM = "sync/resumeFrom";

export type RequestId = number | string | null;

export type DeviceRole = "full" | "approver";

export type ResurfaceReason = "done" | "failed" | "needs_you";

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

export interface PromptParams {
  threadId: string;
  content: unknown;
  cwd?: string;
  harnessId?: string;
  runtime?: RuntimeSpec;
}

export interface PromptResult {
  threadId: string;
  acpSessionId: string;
  accepted: boolean;
}

export interface SessionCancelResult {
  threadId: string;
  cancelled: boolean;
}

export interface PermissionReplyResult {
  requestId: string;
  delivered: boolean;
}

export interface SessionCancelParams {
  threadId: string;
}

export interface ThreadFoldParams {
  threadId: string;
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
} as const;
