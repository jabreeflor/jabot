//! The prop contract every view is rendered from.
//!
//! These shapes mirror the host store rows in `src-tauri/src/host/store/models.rs`
//! (camelCase, as serde emits them) so the feature issues that wire real data —
//! #14 transcript, #16 folders, #17 crew, #22 Inbox, #28 PRs — map a row onto a
//! prop without reshaping it. Where a row stores JSON text (`toolsJson`), the UI
//! takes the parsed value; where the UI needs a join the row cannot express
//! (a folder and its threads, a thread and its latest run), that join is named
//! here so there is one answer to what the host has to return.

import type { ResurfaceReason } from "../host";

/** `bots.color` — a class name, because the gradient *is* the identity. */
export type BotColor =
  | "b-teal"
  | "b-yellow"
  | "b-purple"
  | "b-violet"
  | "b-blue"
  | "b-orange"
  | "b-pink"
  | "b-green";

export const BOT_COLORS: readonly BotColor[] = [
  "b-teal",
  "b-yellow",
  "b-purple",
  "b-violet",
  "b-blue",
  "b-orange",
  "b-pink",
  "b-green",
];

/** `threads.state` — visibility only. Fold does not stop work (#5). */
export type ThreadState = "active" | "folded" | "resurfaced" | "archived";

/** `threads.fold_policy`. "Wait for Inbox" is a policy, not a fifth state. */
export type FoldPolicy = "default" | "wait_for_inbox";

/** `runs.state`. One thread has many sequential runs on one ACP session. */
export type RunState =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "timed_out"
  | "lost"
  | "needs_you";

/** `inbox_events.kind`. The three the host can also push as a resurface
    notification are exactly `ResurfaceReason`. */
export type InboxKind =
  | ResurfaceReason
  | "folded"
  | "judgment_call"
  | "permission"
  | "lost"
  | "stuck";

/** `thread_prs.status`. */
export type PrStatus = "open" | "draft" | "merged" | "closed";

/** ACP `tool_call` kinds, plus the `write` alias the edit family reports. */
export type ToolKind =
  | "read"
  | "edit"
  | "write"
  | "execute"
  | "search"
  | "fetch"
  | "think"
  | "delete"
  | "move"
  | "other";

/** ACP tool-call status. `pending` also covers "awaiting your approval". */
export type ToolStatus =
  | "pending"
  | "in_progress"
  | "completed"
  | "failed"
  | "cancelled";

/** `folders` row. Folder = one registered repo (#16). */
export interface Folder {
  id: string;
  name: string;
  path: string;
}

/** The sidebar needs each folder with its threads — the join #16 will do. */
export interface FolderWithThreads extends Folder {
  threads: ThreadSummary[];
}

/**
 * A sidebar row: the `threads` columns the list needs, plus the state of the
 * latest run. `runState` is null before a thread has ever run.
 */
export interface ThreadSummary {
  id: string;
  folderId: string | null;
  botId: string | null;
  harnessId: string;
  title: string;
  state: ThreadState;
  foldPolicy: FoldPolicy;
  runState: RunState | null;
  preview?: string;
}

/**
 * A `bots` row with `tools_json` parsed. `tools` are MCP catalog ids for
 * workers and host-tool ids for Chief (#6, #18).
 */
export interface Bot {
  id: string;
  name: string;
  color: BotColor;
  instructions: string;
  tools: string[];
  harnessId: string;
  isChief: boolean;
  templateId?: string | null;
  /** Unread work on this bot's standing thread — the red dot on its blob. */
  unread?: boolean;
}

/** A template is a bot without an id, harness included (#6). */
export interface BotTemplate {
  templateId: string;
  name: string;
  color: BotColor;
  instructions: string;
  tools: string[];
  harnessId: string;
}

/** What the bot editor emits. The host assigns the id on create (#17). */
export interface BotDraft {
  name: string;
  color: BotColor;
  instructions: string;
  tools: string[];
  harnessId: string;
  templateId?: string | null;
}

/**
 * A `harnesses` row as a New Chat / crew-editor card. `blurb` and `available`
 * come from the catalog and the Doctor probe (#13); `available: undefined`
 * means not probed yet, which is not the same as missing.
 */
export interface HarnessCard {
  id: string;
  label: string;
  blurb: string;
  /** Accent colour token, e.g. `var(--h-claude)`. */
  accent: string;
  available?: boolean;
  installHint?: string;
}

/** One line of a toolblock — one ACP `tool_call` / `tool_call_update`. */
export interface ToolCall {
  id: string;
  kind: ToolKind;
  /** What it acted on: a path, a command, a query. */
  target: string;
  status: ToolStatus;
  /** Trailing note the prototype shows after the target: "+18 −7", "6 files". */
  note?: string;
}

/**
 * One rendered transcript entry, in ACP terms rather than the prototype's
 * `[kind, html]` tuples. `tool` is per-call — `Transcript` groups a consecutive
 * run into one block, which is a render decision, not a data one.
 *
 * #20's permission prompt is a `notice` with its ACP options as actions.
 */
export type TranscriptItem =
  | { kind: "stamp"; id: string; text: string }
  | { kind: "sys"; id: string; text: string }
  | { kind: "user"; id: string; text: string }
  | { kind: "agent"; id: string; text: string; streaming?: boolean }
  | { kind: "tool"; id: string; call: ToolCall }
  | {
      kind: "notice";
      id: string;
      title: string;
      pill?: string;
      body: string;
      actions: NoticeAction[];
      /** The thread the decision is about — the one a fold offer folds and the
          one a #20 permission prompt is blocking. Absent for plain notices. */
      threadId?: string;
      /** Set once answered: the card animates out instead of vanishing. */
      resolved?: boolean;
    };

export interface NoticeAction {
  id: string;
  label: string;
  primary?: boolean;
}

/** Who a card is from: a crew bot with a face, or a code session. */
export type CardSource =
  | { type: "bot"; name: string; color: BotColor }
  | { type: "code" };

/** An `inbox_events` row plus what the row needs to draw itself (#22). */
export interface InboxCard {
  id: string;
  threadId: string;
  kind: InboxKind;
  title: string;
  summary: string;
  createdAt: string;
  source: CardSource;
  detail?: InboxDetail;
}

export interface InboxDetail {
  /** The thread's journey: "jabot-app · started → folded → resurfaced". */
  path: string;
  bullets: string[];
  actions: NoticeAction[];
}

/**
 * A `thread_prs` row plus the GitHub fields #28 polls. `threadId` is null for a
 * PR that exists in the repo but was not opened by a JaBot session.
 */
export interface PullRequest {
  id: string;
  threadId: string | null;
  repo: string;
  number: number;
  url: string;
  title: string;
  status: PrStatus;
  checkState: "passing" | "running" | "failing" | null;
  updatedAt: string;
  additions: number;
  deletions: number;
  headRef?: string;
  baseRef?: string;
  filesChanged?: number;
  /** Why the human should care — merged by you, waiting on a session, … */
  summary?: string;
  detail?: PrDetail;
}

export interface PrDetail {
  checks: PrCheck[];
  bullets: string[];
  actions: NoticeAction[];
}

export interface PrCheck {
  label: string;
  state: "passing" | "running" | "failing";
}

/**
 * A machine that can run threads. MVP1 has exactly one — this Mac — but the
 * chat header keeps the picker so adding a second is data, not new chrome.
 */
export interface HostTarget {
  hostId: string;
  name: string;
  reachable: boolean;
}

/** An MCP catalog entry as a chip in the bot editor (#18 fills the catalog). */
export interface ToolOption {
  id: string;
  label: string;
}

/** What New Chat emits. The host resolves the runtime and spawns (#6, #10). */
export interface NewChatDraft {
  harnessId: string;
  folderId: string | null;
  task: string;
}

/**
 * What the main pane is showing. Navigation state, not host data — but it is
 * the sidebar's prop, so it lives with the other contracts.
 */
export type Selection =
  | { view: "bot"; botId: string }
  | { view: "thread"; threadId: string }
  | { view: "crew" }
  | { view: "inbox" }
  | { view: "prs" };
