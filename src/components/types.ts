//! The prop contract every view is rendered from.
//!
//! These shapes mirror the host store rows in `src-tauri/src/host/store/models.rs`
//! (camelCase, as serde emits them) so the feature issues that wire real data —
//! #14 transcript, #16 folders, #17 crew, #22 Inbox, #28 PRs — map a row onto a
//! prop without reshaping it. Where a row stores JSON text (`toolsJson`), the UI
//! takes the parsed value; where the UI needs a join the row cannot express
//! (a folder and its threads, a thread and its latest run), that join is named
//! here so there is one answer to what the host has to return.

import type { ResurfaceReason, ToolConnectionStatus } from "../host";

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
  | "stuck"
  /** A pull request one of the sessions opened changed in a way worth saying
      out loud — it exists, its checks went red, a reviewer asked for work
      (#28). Its own kind because it is not a claim about a run: the session is
      usually finished and archived by the time its CI fails. */
  | "pr";

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
  /** Where a thread in this folder starts: the repository root when there is
      one, else the registered path. The host resolves it, so New Chat passes
      it straight through to `thread/open` (#16, and #23 swaps in a worktree). */
  cwd?: string;
  /** False for a directory git does not claim — a folder that works for
      threads and has no PR surface. `undefined` means not asked yet. */
  isGit?: boolean;
  /** `owner/name` from `origin`, when there is one. */
  repo?: string;
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
  /**
   * The picture this bot was given, as a `data:` URL, or null for the colour
   * mark. Held on the bot and not looked up per surface, because every place
   * that draws a bot already has the bot.
   */
  image?: string | null;
  /** Unread work on this bot's standing thread — the red dot on its avatar. */
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
  /**
   * The icon as the editor left it: a `data:` URL to set one, `null` to go
   * back to the colour mark. Distinct from absent — a draft that omits it is
   * one from a surface that does not edit icons, and the saved picture stays.
   */
  image?: string | null;
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

/** Who a card is from: a crew bot with an icon, or a code session.
 *
 * The bot variant carries everything its icon is drawn from rather than an id
 * to look one up by: a card is built where the crew is already in hand, and a
 * card holding only a reference would have to draw something else for the time
 * between the crew loading and the card doing so. `inbox/list` has no bot on it
 * at all today and every host card is a `code` one, so this costs the host
 * nothing; it is the fixtures and #24's handoff cards that supply it. */
export type CardSource =
  | { type: "bot"; name: string; color: BotColor; image?: string | null }
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
 * A `thread_prs` row plus the GitHub fields #28 polls. Every row has a thread:
 * `thread_prs.thread_id` is `NOT NULL`, because a PR gets here by a session
 * opening it. `provider` + `repo` + `number` is the key #28 dedupes on.
 */
export interface PullRequest {
  id: string;
  /** The session that opened it. Absent for one of the user's own pull
      requests written somewhere else (#28) — the board shows those too once
      they have signed in, and they have no thread here to reopen. */
  threadId?: string;
  provider: string;
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

/** An MCP catalog entry as a chip in the bot editor (#18).
 *
 * `status` is the host's answer for the *provider grant*, not for the chip:
 * Gmail, Calendar and Drive share one Google login, so connecting one connects
 * all three. `undefined` means not asked yet, which is not the same as
 * disconnected — the same distinction `HarnessCard.available` makes. */
export interface ToolOption {
  id: string;
  label: string;
  status?: ToolConnectionStatus;
  /** One sentence for the chip's tooltip: which account, or what went wrong. */
  detail?: string;
}

/** What New Chat emits. The host resolves the runtime and spawns (#6, #10). */
export interface NewChatDraft {
  harnessId: string;
  folderId: string | null;
  task: string;
  /** Work in the folder's own checkout instead of a fresh worktree (#23).
      Advanced, and omitted when unset so the ordinary request on the wire is
      exactly what it was. */
  useCheckout?: boolean;
  /** What the thread's branch forks from — a branch, tag or sha. Omitted for
      the host's own default, which is `origin/<default branch>` and never the
      user's possibly-dirty `HEAD`. */
  baseRef?: string;
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
  | { view: "prs" }
  | { view: "schedules" };
