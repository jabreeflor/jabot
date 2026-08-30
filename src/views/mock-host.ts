//! Stand-in for the host, so the ported views can be exercised before the
//! feature issues wire real data.
//!
//! The seed mirrors `src-tauri/src/host/store/seed.rs` (same crew, same colours,
//! same harness ids) and the prototype's fixtures. Every action here is a host
//! call in disguise:
//!
//! | Action          | Becomes                                      |
//! |-----------------|----------------------------------------------|
//! | `startThread`   | `session/prompt` on a new thread (#10, #14)  |
//! | `foldThread`    | `thread/fold` (#26)                          |
//! | `archiveThread` | thread overlay transition (#15)              |
//! | `sendMessage`   | `session/prompt` (#14)                       |
//! | `saveBot`       | `crew/create` / `crew/update` (#17 — live)   |
//! | `removeBot`     | `crew/remove` (#17 — live)                   |
//!
//! It is *not* pure: folding stamps `createdAt` on the Inbox card it writes.
//! The host does that in SQLite, before it notifies anyone (#5).
//!
//! The rows marked *live* have been swapped for real host calls; what stays
//! here is the fixture the shell renders **before** the host has answered —
//! a preview build, a unit test, a host still starting — and the guards in
//! `mock-host.test.ts` are what keep the fixture from drifting away from the
//! catalogs the host actually serves.

import { NEEDS_YOU_KINDS } from "../components/status";
import type {
  Bot,
  BotDraft,
  BotTemplate,
  Folder,
  FolderWithThreads,
  FoldPolicy,
  HarnessCard,
  InboxCard,
  NewChatDraft,
  PullRequest,
  ThreadSummary,
  ToolOption,
  TranscriptItem,
} from "../components/types";

/**
 * The tier-1 compiled-in harnesses, and only those — the same three ids
 * `seed.rs` writes into `harnesses`, because `threads.harness_id` is a foreign
 * key onto that table.
 *
 * The prototype's "Custom" card is deliberately absent. Tier-3 harnesses are
 * user JSON with ids of their own (#6), so there is no id `"custom"` to spawn;
 * offering it here would let New Chat file a draft the host can only answer
 * with `HARNESS_UNAVAILABLE`. #13 adds the real cards once the catalog can
 * produce them.
 *
 * Pi is Mario Zechner's coding agent. The prototype called it "Inflection's
 * agent", which is wrong — Inflection Pi is a consumer chatbot.
 */
export const HARNESSES: readonly HarnessCard[] = [
  {
    id: "claude",
    label: "Claude Code",
    blurb: "Anthropic's coding agent, wrapped in JaBot's UI",
    accent: "var(--h-claude)",
  },
  {
    id: "codex",
    label: "Codex",
    blurb: "OpenAI's coding agent",
    accent: "var(--h-codex)",
  },
  {
    id: "pi",
    label: "Pi",
    blurb: "Mario Zechner's coding agent",
    accent: "var(--h-pi)",
  },
];

/**
 * The MCP catalog a bot can be allowed to use (#18), with the statuses a fresh
 * install really has: nothing signed in yet, and the two entries that need no
 * grant — Terminal, which is the harness's own `execute`, and the local
 * browser server — already usable. The live values come from `tools/list`.
 */
export const TOOL_CATALOG: readonly ToolOption[] = [
  { id: "gmail", label: "Gmail", status: "needs_auth" },
  { id: "calendar", label: "Calendar", status: "needs_auth" },
  { id: "github", label: "GitHub", status: "needs_auth" },
  {
    id: "terminal",
    label: "Terminal",
    status: "connected",
    detail: "Runs through the harness. Every command asks first.",
  },
  { id: "browser", label: "Browser", status: "connected" },
  { id: "notion", label: "Notion", status: "needs_auth" },
  { id: "drive", label: "Drive", status: "needs_auth" },
  { id: "slack", label: "Slack", status: "needs_auth" },
];

/** Chief's extra host tools (#6). Not MCP, so not offered to other bots. */
export const HOST_TOOLS: readonly ToolOption[] = [
  { id: "handoff_to_bot", label: "Handoff" },
  { id: "spawn_code_session", label: "Spawn code session" },
  { id: "fold_thread", label: "Fold thread" },
  { id: "list_crew_status", label: "Crew status" },
];

/**
 * Fallback copies of the shipped template packs in
 * `src-tauri/src/host/crew/templates/*.json`, which are the source of truth
 * (#17). `mock-host.test.ts` fails if the two drift: a template that promises
 * tools the host would refuse is worse than no template at all.
 */
export const BOT_TEMPLATES: readonly BotTemplate[] = [
  {
    templateId: "expense",
    name: "Expense Manager",
    color: "b-green",
    instructions:
      "Chase receipts, file the monthly report, flag anything unusual. Never move money — draft, then wait for me.",
    tools: ["gmail", "drive"],
    harnessId: "claude",
  },
  {
    templateId: "talent",
    name: "Talent Scout",
    color: "b-pink",
    instructions:
      "Watch for interesting people. Draft warm intros in my voice and hold them for review — nothing goes out unread.",
    tools: ["browser", "gmail"],
    harnessId: "claude",
  },
  {
    templateId: "social",
    name: "Social Media",
    color: "b-blue",
    instructions:
      "Draft posts from my shipped work. Never publish without approval.",
    tools: ["browser"],
    harnessId: "claude",
  },
  {
    templateId: "ops",
    name: "Ops / On-call",
    color: "b-orange",
    instructions:
      "Watch deploys and alerts. Wake me only for real fires; everything else goes in the morning digest.",
    tools: ["terminal", "slack"],
    harnessId: "codex",
  },
];

export interface MockState {
  bots: Bot[];
  folders: Folder[];
  threads: ThreadSummary[];
  /** Keyed by bot id or thread id — a conversation is a conversation. */
  transcripts: Record<string, TranscriptItem[]>;
  inbox: InboxCard[];
  pullRequests: PullRequest[];
  /** Monotonic id source, so the reducer stays self-contained. */
  seq: number;
}

const minutesAgo = (minutes: number) =>
  new Date(Date.now() - minutes * 60_000).toISOString();

export function initialMockState(): MockState {
  return {
    bots: [
      {
        id: "chief",
        name: "Chief",
        color: "b-teal",
        instructions:
          "Route work across the crew. Fold long tasks away, surface only what matters.",
        tools: [
          "handoff_to_bot",
          "spawn_code_session",
          "fold_thread",
          "list_crew_status",
        ],
        harnessId: "claude",
        isChief: true,
      },
      {
        id: "code",
        name: "Code",
        color: "b-yellow",
        instructions:
          "Run coding sessions in my repos. Open PRs, never push to main.",
        tools: ["github", "terminal"],
        harnessId: "claude",
        isChief: false,
        unread: true,
      },
      {
        id: "inboxm",
        name: "Inbox Mgr",
        color: "b-purple",
        instructions:
          "Keep Gmail at zero. Park drafts for anything that needs my voice.",
        tools: ["gmail"],
        harnessId: "claude",
        isChief: false,
      },
      {
        id: "sched",
        name: "Scheduler",
        color: "b-violet",
        instructions:
          "Guard the calendar. Fix conflicts, protect deep-work mornings.",
        tools: ["calendar"],
        harnessId: "claude",
        isChief: false,
      },
      {
        id: "rsrch",
        name: "Research",
        color: "b-blue",
        instructions: "Dig sources, pull context into GlobNet, brief me short.",
        tools: ["browser", "notion"],
        harnessId: "claude",
        isChief: false,
      },
      {
        id: "writer",
        name: "Writer",
        color: "b-orange",
        instructions: "Draft in my voice: plain, short, no filler.",
        tools: ["gmail", "notion"],
        harnessId: "claude",
        isChief: false,
      },
    ],
    folders: [
      { id: "jabot-app", name: "jabot-app", path: "~/code/jabot-app" },
      { id: "globnet-sync", name: "globnet-sync", path: "~/code/globnet-sync" },
    ],
    threads: [
      {
        id: "auth",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "claude",
        title: "Auth migration",
        state: "active",
        foldPolicy: "default",
        runState: "running",
      },
      {
        id: "sidebar",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "codex",
        title: "Sidebar overflow fix",
        state: "active",
        foldPolicy: "default",
        runState: "succeeded",
      },
      {
        id: "retry",
        folderId: "globnet-sync",
        botId: "code",
        harnessId: "codex",
        title: "Retry logic for backups",
        state: "active",
        foldPolicy: "default",
        runState: "needs_you",
      },
      {
        // Folded, so it is not in the sidebar at all — it is in the Inbox
        // under STILL SLEEPING. That is the promise fold makes (#5).
        id: "nas",
        folderId: "globnet-sync",
        botId: "code",
        harnessId: "pi",
        title: "NAS backup script",
        state: "folded",
        foldPolicy: "wait_for_inbox",
        runState: "running",
      },
      // Two sessions whose work is over but whose PRs are still on the board.
      // They stay because `thread_prs.thread_id` is NOT NULL: a PR without the
      // session that opened it is not a row the store can hold.
      {
        id: "deps",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "claude",
        title: "Bump dependencies",
        state: "archived",
        foldPolicy: "default",
        runState: "succeeded",
      },
      {
        id: "onboarding",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "claude",
        title: "Onboarding flow polish",
        state: "archived",
        foldPolicy: "default",
        runState: "succeeded",
      },
    ],
    transcripts: {
      chief: [
        { kind: "stamp", id: "chief-0", text: "2:23 PM" },
        {
          kind: "agent",
          id: "chief-1",
          text: "Morning. Two things: the auth migration is running — about 40 minutes left — and Scheduler fixed Thursday. Want me to keep the migration thread here, or fold it away?",
        },
        {
          kind: "user",
          id: "chief-2",
          text: "Fold it. I don't want to babysit a long task — just surface it in Inbox when it's done or stuck.",
        },
        {
          kind: "notice",
          id: "chief-3",
          title: "Auth migration",
          pill: "Long-running",
          body: "Est. 40 min. If folded, this thread disappears and reappears in Inbox on done, failure, or question.",
          threadId: "auth",
          actions: [
            { id: "fold", label: "Disappear until done", primary: true },
            { id: "watch", label: "Keep watching" },
          ],
        },
      ],
      code: [
        { kind: "stamp", id: "code-0", text: "Yesterday" },
        {
          kind: "agent",
          id: "code-1",
          text: "Four threads across two repos. Two are mine to finish; the retry work is waiting on a call from you.",
        },
      ],
      inboxm: [
        {
          kind: "agent",
          id: "inboxm-1",
          text: "Gmail is at zero except two UGREEN invoices. Archive them, or flag for finance?",
        },
      ],
      sched: [
        {
          kind: "agent",
          id: "sched-1",
          text: "Thursday is clear again — I moved the vendor sync to 4pm and kept the morning block.",
        },
      ],
      rsrch: [
        {
          kind: "agent",
          id: "rsrch-1",
          text: "Pulled six sources on ACP adapters into GlobNet. Short version: everyone is converging on stdio.",
        },
      ],
      writer: [
        {
          kind: "agent",
          id: "writer-1",
          text: "Weekly digest draft is 1,240 words in your voice. It is parked until you read it.",
        },
      ],
      auth: [
        { kind: "stamp", id: "auth-0", text: "1:48 PM" },
        {
          kind: "user",
          id: "auth-1",
          text: "Migrate auth from JWT to server sessions. Keep the cookie flow unchanged.",
        },
        {
          kind: "tool",
          id: "auth-2",
          call: {
            id: "auth-t1",
            kind: "read",
            target: "src/auth/*.ts",
            status: "completed",
            note: "6 files",
          },
        },
        {
          kind: "tool",
          id: "auth-3",
          call: {
            id: "auth-t2",
            kind: "edit",
            target: "middleware/session.ts",
            status: "completed",
          },
        },
        {
          kind: "tool",
          id: "auth-4",
          call: {
            id: "auth-t3",
            kind: "execute",
            target: "npm test",
            status: "in_progress",
          },
        },
        {
          kind: "agent",
          id: "auth-5",
          text: "Middleware rewritten, cookie flow untouched. Running the full suite now — about 40 min left. Fold me into Inbox if you don't want to watch.",
        },
      ],
      sidebar: [
        { kind: "stamp", id: "sidebar-0", text: "9:02 AM" },
        {
          kind: "user",
          id: "sidebar-1",
          text: "The sidebar clips at narrow widths. Fix it.",
        },
        {
          kind: "tool",
          id: "sidebar-2",
          call: {
            id: "sidebar-t1",
            kind: "edit",
            target: "src/ui/sidebar.css",
            status: "completed",
            note: "+18 −7",
          },
        },
        {
          kind: "tool",
          id: "sidebar-3",
          call: {
            id: "sidebar-t2",
            kind: "execute",
            target: "npm test",
            status: "completed",
            note: "48 passing",
          },
        },
        {
          kind: "agent",
          id: "sidebar-4",
          text: "Fixed — the rail now collapses under 900px instead of clipping. Opened PR #22, checks are green.",
        },
        {
          kind: "sys",
          id: "sidebar-5",
          text: "Session finished — PR #22 in Pull Requests",
        },
      ],
      retry: [
        { kind: "stamp", id: "retry-0", text: "7:44 AM" },
        {
          kind: "user",
          id: "retry-1",
          text: "The backup dies when the NAS drops off the network. Add retries.",
        },
        {
          kind: "tool",
          id: "retry-2",
          call: {
            id: "retry-t1",
            kind: "edit",
            target: "scripts/backup.sh",
            status: "completed",
            note: "+64 −12",
          },
        },
        {
          kind: "agent",
          id: "retry-3",
          text: "Retries are in with backoff. One call for you: should a failed run alert, or wait for the next nightly?",
        },
      ],
      nas: [
        { kind: "stamp", id: "nas-0", text: "Yesterday" },
        {
          kind: "user",
          id: "nas-1",
          text: "Write a nightly backup script for GlobNet on the NAS. Retry on network drops.",
        },
        {
          kind: "tool",
          id: "nas-2",
          call: {
            id: "nas-t1",
            kind: "write",
            target: "scripts/backup.sh",
            status: "completed",
          },
        },
        {
          kind: "sys",
          id: "nas-3",
          text: "Thread folded — will reappear in Inbox",
        },
      ],
      deps: [
        { kind: "stamp", id: "deps-0", text: "Yesterday" },
        {
          kind: "user",
          id: "deps-1",
          text: "Bump everything that has a patch release and see what breaks.",
        },
        {
          kind: "agent",
          id: "deps-2",
          text: "Opened PR #19 as a draft — the dependency audit has to clear before this is reviewable.",
        },
      ],
      onboarding: [
        { kind: "stamp", id: "onboarding-0", text: "Monday" },
        {
          kind: "user",
          id: "onboarding-1",
          text: "The first-run flow is three screens too long. Cut it down.",
        },
        {
          kind: "sys",
          id: "onboarding-2",
          text: "Session archived — PR #18 merged",
        },
      ],
    },
    inbox: [
      {
        id: "inbox-sidebar",
        threadId: "sidebar",
        kind: "done",
        title: "Sidebar overflow fix finished",
        summary:
          "jabot-app coding session · slept 18 min · 1 file changed · tests green · PR #22 opened",
        createdAt: minutesAgo(38),
        source: { type: "code" },
        detail: {
          path: "jabot-app · started → folded → ran 18 min → resurfaced",
          bullets: [
            "Rail collapses under 900px instead of clipping — 1 file changed",
            "All 48 tests passing",
            "One judgment call: chose 900px over 840px, flagged for review",
          ],
          actions: [
            { id: "open-pr", label: "Open PR #22", primary: true },
            { id: "reopen", label: "Reopen thread" },
            { id: "archive", label: "Archive" },
          ],
        },
      },
      {
        id: "inbox-invoices",
        threadId: "inboxm",
        kind: "needs_you",
        title: "Inbox Manager needs a call",
        summary: "Two invoices from UGREEN — archive, or flag for finance?",
        createdAt: minutesAgo(63),
        source: {
          type: "bot",
          name: "Inbox Mgr",
          color: "b-purple",
        },
      },
      {
        id: "inbox-digest",
        threadId: "writer",
        kind: "needs_you",
        title: "Weekly digest draft ready",
        summary: "1,240 words in your voice. Awaiting review before it sends.",
        createdAt: minutesAgo(140),
        source: {
          type: "bot",
          name: "Writer",
          color: "b-orange",
        },
      },
      {
        id: "inbox-nas",
        threadId: "nas",
        kind: "folded",
        title: "Nightly NAS backup",
        summary: "globnet-sync · resurfaces on success, failure, or question.",
        createdAt: minutesAgo(120),
        source: { type: "code" },
      },
    ],
    pullRequests: [
      {
        id: "pr-23",
        threadId: "auth",
        provider: "github",
        repo: "jabot-app",
        number: 23,
        url: "https://github.com/jabreeflor/jabot-app/pull/23",
        title: "Migrate auth to sessions",
        status: "open",
        checkState: "passing",
        updatedAt: minutesAgo(38),
        additions: 214,
        deletions: 96,
        headRef: "auth/sessions",
        baseRef: "main",
        filesChanged: 3,
        summary: "from folded session",
        detail: {
          checks: [
            { label: "48 tests passing", state: "passing" },
            { label: "lint", state: "passing" },
            { label: "build", state: "passing" },
          ],
          bullets: [
            "Session middleware rewritten, cookie flow unchanged",
            "Flagged: 30-day cookie expiry kept — confirm before merge",
          ],
          actions: [
            { id: "merge", label: "Merge", primary: true },
            { id: "diff", label: "View diff" },
            { id: "reopen", label: "Reopen thread" },
          ],
        },
      },
      {
        id: "pr-22",
        threadId: "sidebar",
        provider: "github",
        repo: "jabot-app",
        number: 22,
        url: "https://github.com/jabreeflor/jabot-app/pull/22",
        title: "Fix sidebar overflow at narrow widths",
        status: "open",
        checkState: "passing",
        updatedAt: minutesAgo(108),
        additions: 18,
        deletions: 7,
        summary: "checks green",
      },
      {
        id: "pr-21",
        threadId: "retry",
        provider: "github",
        repo: "globnet-sync",
        number: 21,
        url: "https://github.com/jabreeflor/globnet-sync/pull/21",
        title: "Add retry logic to NAS backup",
        status: "open",
        checkState: "running",
        updatedAt: minutesAgo(186),
        additions: 64,
        deletions: 12,
        summary: "2 of 3 checks done",
      },
      {
        id: "pr-19",
        threadId: "deps",
        provider: "github",
        repo: "jabot-app",
        number: 19,
        url: "https://github.com/jabreeflor/jabot-app/pull/19",
        title: "Bump dependencies",
        status: "draft",
        checkState: null,
        updatedAt: minutesAgo(1500),
        additions: 302,
        deletions: 288,
        summary: "waiting on dependency audit session",
      },
      {
        id: "pr-18",
        threadId: "onboarding",
        provider: "github",
        repo: "jabot-app",
        number: 18,
        url: "https://github.com/jabreeflor/jabot-app/pull/18",
        title: "Onboarding flow polish",
        status: "merged",
        checkState: "passing",
        updatedAt: minutesAgo(1600),
        additions: 142,
        deletions: 58,
        summary: "merged by you",
      },
    ],
    seq: 1,
  };
}

export type MockAction =
  | { type: "startThread"; draft: NewChatDraft }
  | { type: "foldThread"; threadId: string; policy?: FoldPolicy }
  | { type: "archiveThread"; threadId: string }
  | { type: "deleteThread"; threadId: string }
  | { type: "sendMessage"; conversationId: string; text: string }
  | {
      type: "answerNotice";
      conversationId: string;
      itemId: string;
      actionId: string;
    }
  | { type: "removeNotice"; conversationId: string; itemId: string }
  | { type: "dismissInboxCard"; cardId: string }
  | { type: "saveBot"; botId: string | null; draft: BotDraft }
  | { type: "removeBot"; botId: string };

export function mockHostReducer(
  state: MockState,
  action: MockAction,
): MockState {
  switch (action.type) {
    case "startThread":
      return startThread(state, action.draft);
    case "foldThread":
      return foldThread(state, action.threadId, action.policy);
    case "archiveThread":
      return {
        ...state,
        threads: state.threads.map((thread) =>
          thread.id === action.threadId
            ? { ...thread, state: "archived" }
            : thread,
        ),
      };
    case "deleteThread":
      return {
        ...state,
        threads: state.threads.filter(
          (thread) => thread.id !== action.threadId,
        ),
        inbox: state.inbox.filter((card) => card.threadId !== action.threadId),
      };
    case "sendMessage":
      return appendItems(state, action.conversationId, [
        {
          kind: "user",
          id: `msg-${state.seq}`,
          text: action.text,
        },
      ]);
    case "answerNotice":
      return answerNotice(state, action);
    case "removeNotice":
      return removeNotice(state, action.conversationId, action.itemId);
    case "dismissInboxCard":
      return {
        ...state,
        inbox: state.inbox.filter((card) => card.id !== action.cardId),
      };
    case "saveBot":
      return saveBot(state, action.botId, action.draft);
    case "removeBot":
      return {
        ...state,
        bots: state.bots.filter(
          (bot) => bot.id !== action.botId || bot.isChief,
        ),
      };
  }
}

/** The id a `startThread` will produce, so the caller can select it. */
export function nextThreadId(state: MockState): string {
  return `thread-${state.seq}`;
}

function startThread(state: MockState, draft: NewChatDraft): MockState {
  const id = nextThreadId(state);
  const folder = state.folders.find((f) => f.id === draft.folderId);
  const thread: ThreadSummary = {
    id,
    folderId: draft.folderId,
    botId: "code",
    harnessId: draft.harnessId,
    title: draft.task,
    state: "active",
    foldPolicy: "default",
    runState: "queued",
  };
  return {
    ...state,
    seq: state.seq + 1,
    threads: [...state.threads, thread],
    transcripts: {
      ...state.transcripts,
      [id]: [
        { kind: "user", id: `${id}-0`, text: draft.task },
        {
          kind: "tool",
          id: `${id}-1`,
          call: {
            id: `${id}-t0`,
            kind: "other",
            target: `${draft.harnessId} session in ${folder?.name ?? "~"}`,
            status: "in_progress",
            // Says which tree the thread will work in, because the card can
            // now ask for the folder's own checkout (#23). A mock that
            // reported "worktree" whatever was ticked would be quietly lying
            // about the one thing the new control decides.
            note: worktreeNote(draft, folder?.name),
          },
        },
      ],
    },
  };
}

/** What the bootstrap line says about where the thread will work (#23). */
function worktreeNote(draft: NewChatDraft, folderName?: string): string {
  if (!draft.folderId) return "starting…";
  if (draft.useCheckout) return `in ${folderName ?? "the folder"}`;
  return `worktree from ${draft.baseRef ?? "origin/main"}`;
}

/**
 * Fold: the thread keeps running, the row leaves the sidebar, and a sleeping
 * card lands in the Inbox. Writing the card *before* anything is notified is
 * the persist-then-notify rule from #5 — here it is the same statement.
 *
 * An omitted `policy` keeps the one the thread already has, exactly as
 * `thread/fold` does with an absent `policy` field (#26). The mock has to make
 * the same promise as the host, or the shell is tested against a rule the real
 * fold does not follow.
 */
function foldThread(
  state: MockState,
  threadId: string,
  policy?: FoldPolicy,
): MockState {
  const thread = state.threads.find((t) => t.id === threadId);
  if (!thread || thread.state === "folded") return state;
  const folder = state.folders.find((f) => f.id === thread.folderId);

  const card: InboxCard = {
    id: `inbox-${state.seq}`,
    threadId,
    kind: "folded",
    title: thread.title,
    summary: `${folder?.name ?? "no folder"} · folded session. Resurfaces on done, failure, or question.`,
    createdAt: new Date().toISOString(),
    source: { type: "code" },
  };

  return {
    ...state,
    seq: state.seq + 1,
    threads: state.threads.map((t) =>
      t.id === threadId
        ? { ...t, state: "folded", foldPolicy: policy ?? t.foldPolicy }
        : t,
    ),
    inbox: [card, ...state.inbox],
  };
}

/**
 * The thread a transcript notice is about, if it names one.
 *
 * Chief's fold card is an affordance on a *thread*, so the shell has to know
 * which one before it can decide whether the fold belongs to the host or to
 * the fixtures (#26). The reducer cannot make that call for it: it only knows
 * the threads it seeded.
 */
export function noticeThreadId(
  state: MockState,
  conversationId: string,
  itemId: string,
): string | null {
  const item = (state.transcripts[conversationId] ?? []).find(
    (entry) => entry.id === itemId,
  );
  return item?.kind === "notice" ? (item.threadId ?? null) : null;
}

function answerNotice(
  state: MockState,
  action: Extract<MockAction, { type: "answerNotice" }>,
): MockState {
  const items = state.transcripts[action.conversationId] ?? [];
  const notice = items.find(
    (item) => item.id === action.itemId && item.kind === "notice",
  );
  if (!notice || notice.kind !== "notice") return state;

  const resolved: MockState = {
    ...state,
    transcripts: {
      ...state.transcripts,
      [action.conversationId]: items.map((item) =>
        item.id === action.itemId && item.kind === "notice"
          ? { ...item, resolved: true }
          : item,
      ),
    },
  };

  if (action.actionId !== "fold" || !notice.threadId) return resolved;

  const folded = foldThread(resolved, notice.threadId);
  return appendItems(folded, action.conversationId, [
    {
      kind: "sys",
      id: `${action.itemId}-sys`,
      text: "Thread folded — will reappear in Inbox",
    },
    {
      kind: "agent",
      id: `${action.itemId}-reply`,
      text: "Done. It's out of your way — I'll ping you the moment it needs you.",
    },
  ]);
}

/**
 * Drop an answered card once its exit animation has run. `resolved` only fades
 * it; leaving it in the list would keep its box, its gap, and its place in the
 * accessibility tree — an invisible hole in the middle of the transcript.
 */
function removeNotice(
  state: MockState,
  conversationId: string,
  itemId: string,
): MockState {
  const items = state.transcripts[conversationId];
  if (!items?.some((item) => item.id === itemId)) return state;

  return {
    ...state,
    transcripts: {
      ...state.transcripts,
      [conversationId]: items.filter((item) => item.id !== itemId),
    },
  };
}

function appendItems(
  state: MockState,
  conversationId: string,
  items: TranscriptItem[],
): MockState {
  return {
    ...state,
    seq: state.seq + 1,
    transcripts: {
      ...state.transcripts,
      [conversationId]: [
        ...(state.transcripts[conversationId] ?? []),
        ...items,
      ],
    },
  };
}

function saveBot(
  state: MockState,
  botId: string | null,
  draft: BotDraft,
): MockState {
  if (botId) {
    return {
      ...state,
      bots: state.bots.map((bot) =>
        bot.id === botId ? { ...bot, ...draft } : bot,
      ),
    };
  }
  const id = `bot-${state.seq}`;
  return {
    ...state,
    seq: state.seq + 1,
    bots: [...state.bots, { id, isChief: false, ...draft }],
    transcripts: {
      ...state.transcripts,
      [id]: [
        {
          kind: "sys",
          id: `${id}-0`,
          text: `${draft.name} joined the crew.`,
        },
      ],
    },
  };
}

/**
 * Folded and archived threads are not in the sidebar. Everything else is
 * grouped under its folder; a thread with no folder has no home in the rail and
 * is reached from the Inbox or by having just started it.
 */
export function sidebarFolders(state: MockState): FolderWithThreads[] {
  return state.folders.map((folder) => ({
    ...folder,
    threads: state.threads.filter(
      (thread) =>
        thread.folderId === folder.id &&
        (thread.state === "active" || thread.state === "resurfaced"),
    ),
  }));
}

/** The red badge, for a shell with no host answer yet. Once `inbox/list` has
    answered, the badge is the host's own `unread` (#22). */
export function needsYouCount(state: MockState): number {
  return state.inbox.filter((card) => NEEDS_YOU_KINDS.includes(card.kind))
    .length;
}

export function openPrCount(state: MockState): number {
  return state.pullRequests.filter((pr) => pr.status === "open").length;
}
