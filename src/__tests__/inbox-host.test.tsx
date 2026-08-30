/**
 * The Inbox on real data (#22): the swap, the badge, and the buttons.
 *
 * `tests/e2e/lifecycle.test.ts` proves what the *host* writes into
 * `inbox_events` and what `inbox/list` answers. What is checked here is the
 * half a live host cannot check — that the desktop actually shows it.
 *
 * The regression these are written against is a specific one: the shell used
 * to render `mock-host`'s three fixture cards no matter what the host said, so
 * every card the lifecycle group wrote was invisible, the badge counted the
 * fixtures, and Archive dismissed a fixture id the host had never heard of.
 * Each case below fails on that shell.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import {
  connectHost,
  INBOX_RESURFACE,
  type FolderListResult,
  type HelloResult,
  type HostClient,
  type InboxListResult,
  type JsonRpcNotification,
  type PermissionPendingResult,
  type ThreadStateResult,
} from "../host";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return { ...actual, connectHost: vi.fn() };
});

const HELLO: HelloResult = {
  protocolVersion: 1,
  hostId: "host-1",
  hostName: "This Mac",
  hostMode: "in-process",
  version: "0.1.0",
  platform: "macos",
  device: { deviceId: "dev-1", name: "This Mac", role: "full" },
  methods: [],
  notifications: [],
};

/** One card per state of mind: back and finished, back and asking, asleep. */
const INBOX: InboxListResult = {
  events: [
    {
      id: "ev-done",
      threadId: "t-sidebar",
      threadTitle: "Sidebar overflow fix",
      threadState: "resurfaced",
      kind: "done",
      title: "Sidebar overflow fix finished",
      summary: "1 file changed · tests green",
      createdAt: "2026-08-20T13:52:00Z",
    },
    {
      id: "ev-stuck",
      threadId: "t-auth",
      threadTitle: "Auth migration",
      threadState: "resurfaced",
      kind: "stuck",
      title: "Auth migration has gone quiet",
      summary: "no output for 20 minutes",
      createdAt: "2026-08-20T13:20:00Z",
    },
  ],
  sleeping: [
    {
      threadId: "t-nas",
      title: "Nightly NAS backup",
      foldPolicy: "wait_for_inbox",
      foldedAt: "2026-08-20T12:00:00Z",
      acpState: "running",
    },
  ],
  // Deliberately not the number of "needs you" rows above: this is the host's
  // own `count_unread_inbox`, and the badge has to be *that*.
  unread: 3,
};

const NO_ASKS: PermissionPendingResult = { requests: [] };

const FOLDERS: FolderListResult = {
  folders: [
    {
      folderId: "f-jabot",
      name: "jabot",
      path: "/Users/j/code/jabot",
      cwd: "/Users/j/code/jabot",
      isGit: false,
      filesToCopy: [],
      sortOrder: 0,
      threads: [],
    },
  ],
};

const reopened: ThreadStateResult = {
  threadId: "t-sidebar",
  title: "Sidebar overflow fix",
  state: "active",
  foldPolicy: "default",
  cwd: "/Users/j/code/jabot",
  harnessId: "claude",
  process: {
    connected: false,
    acpState: "idle",
    pendingPermissions: 0,
    resumable: true,
  },
  runs: [],
  unread: 0,
};

const inbox = vi.fn<() => Promise<InboxListResult>>();
const pendingPermissions = vi.fn<() => Promise<PermissionPendingResult>>();
const reopenThread = vi.fn<(p: unknown) => Promise<ThreadStateResult>>();
const archiveThread = vi.fn<(p: unknown) => Promise<ThreadStateResult>>();
const replyPermission = vi.fn<(p: unknown) => Promise<unknown>>();
type Notify = (notification: JsonRpcNotification) => void;
const handlers = new Set<Notify>();

function client(): HostClient {
  return {
    disconnect: vi.fn(),
    deviceId: "dev-1",
    listFolders: vi.fn(async () => FOLDERS),
    inbox,
    pendingPermissions,
    reopenThread,
    archiveThread,
    replyPermission,
    onNotification: (handler: Notify) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    threadTranscript: vi.fn(async () => ({
      threadId: "t-sidebar",
      headSeq: 0,
      events: [],
      truncated: false,
      queued: [],
    })),
  } as unknown as HostClient;
}

beforeEach(() => {
  handlers.clear();
  inbox.mockReset().mockResolvedValue(INBOX);
  pendingPermissions.mockReset().mockResolvedValue(NO_ASKS);
  reopenThread.mockReset().mockResolvedValue(reopened);
  archiveThread.mockReset().mockResolvedValue({ ...reopened, state: "archived" });
  replyPermission.mockReset().mockResolvedValue({ delivered: true });
  vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });
});

async function openInbox() {
  render(<App />);
  await screen.findByText("This Mac · v0.1.0");
  await userEvent.click(await screen.findByRole("button", { name: /^Inbox —/ }));
  return screen.getByRole("heading", { level: 1, name: "Inbox" });
}

/**
 * Open a card's disclosure.
 *
 * `InboxView` opens the first card that has one by itself, and whether that is
 * this card depends on whether the host had answered before the pane mounted —
 * so this asks the row rather than assuming, and a test about the buttons does
 * not become a test about that race.
 */
async function expand(name: RegExp) {
  const card = await screen.findByRole("button", { name });
  if (card.getAttribute("aria-expanded") === "false") {
    await userEvent.click(card);
  }
  return card;
}

describe("the Inbox on real data", () => {
  it("draws the host's cards and not the fixtures", async () => {
    await openInbox();

    await screen.findByText("Sidebar overflow fix finished");
    expect(screen.getByText("Auth migration has gone quiet")).toBeInTheDocument();
    // Still Sleeping is `threads.state = folded`, projected — not an event.
    expect(screen.getByText("Nightly NAS backup")).toBeInTheDocument();
    expect(screen.getByText("STILL SLEEPING")).toBeInTheDocument();

    // The fixtures the shell used to show regardless of the host.
    expect(screen.queryByText("Inbox Manager needs a call")).toBeNull();
    expect(screen.queryByText("Weekly digest draft ready")).toBeNull();
  });

  it("badges the nav with the host's count, not its own tally of the rows", async () => {
    render(<App />);

    // Three: what `count_unread_inbox` answered. A renderer-side count of the
    // "needs you" kinds in the same list would say one, and the phone — which
    // has always drawn `unread` — would then disagree with the Mac.
    expect(
      await screen.findByRole("button", { name: "Inbox — 3 waiting" }),
    ).toBeInTheDocument();
  });

  it("reopens the thread on the host when a card is opened", async () => {
    await openInbox();
    await screen.findByText("Sidebar overflow fix finished");

    await expand(/^Sidebar overflow fix finished/);
    await userEvent.click(screen.getByRole("button", { name: "Open thread" }));

    // Not a navigation: `thread/reopen` is what clears the badge and puts the
    // row back in the sidebar.
    await waitFor(() =>
      expect(reopenThread).toHaveBeenCalledWith({ threadId: "t-sidebar" }),
    );
  });

  it("archives the card's thread on the host", async () => {
    await openInbox();
    await screen.findByText("Sidebar overflow fix finished");

    await expand(/^Sidebar overflow fix finished/);
    await userEvent.click(screen.getByRole("button", { name: "Archive" }));

    await waitFor(() =>
      expect(archiveThread).toHaveBeenCalledWith({ threadId: "t-sidebar" }),
    );
    // And the list is re-read, because the card is gone from the host's answer.
    expect(inbox.mock.calls.length).toBeGreaterThan(1);
  });

  it("re-reads itself when a thread resurfaces while nobody is looking", async () => {
    await openInbox();
    await screen.findByText("Sidebar overflow fix finished");
    inbox.mockResolvedValue({
      ...INBOX,
      events: [
        {
          id: "ev-new",
          threadId: "t-nas",
          threadTitle: "Nightly NAS backup",
          threadState: "resurfaced",
          kind: "failed",
          title: "Nightly NAS backup failed",
          summary: "rsync exited 23",
          createdAt: "2026-08-20T14:00:00Z",
        },
      ],
      unread: 4,
    });

    for (const handler of handlers) {
      handler({
        jsonrpc: "2.0",
        method: INBOX_RESURFACE,
        params: { threadId: "t-nas", reason: "failed" },
      });
    }

    expect(await screen.findByText("Nightly NAS backup failed")).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Inbox — 4 waiting" }),
    ).toBeInTheDocument();
  });

  it("answers an outstanding permission from the card", async () => {
    pendingPermissions.mockResolvedValue({
      requests: [
        {
          requestId: "req-1",
          threadId: "t-auth",
          title: "Run a command",
          subject: { title: "Run a command", command: "rm -rf build" },
          options: [
            { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
            { optionId: "reject_once", name: "Deny", kind: "reject_once" },
          ],
          createdAt: "2026-08-20T13:59:00Z",
          stale: false,
        },
      ],
    });
    await openInbox();

    // The ask replaces the thread's `stuck` card rather than sitting beside
    // it: two rows for one question is how a human answers twice.
    const card = await expand(/^Run a command/);
    expect(screen.queryByText("Auth migration has gone quiet")).toBeNull();
    // The command, not just the title: "Run ls" and "Run rm -rf build" have
    // the same title and are not the same decision.
    expect(within(card).getByText("rm -rf build")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));

    await waitFor(() =>
      expect(replyPermission).toHaveBeenCalledWith({
        requestId: "req-1",
        deviceId: "dev-1",
        optionId: "allow_once",
      }),
    );
  });

  it("says why rather than showing an empty pane when the host will not answer", async () => {
    inbox.mockRejectedValue(new Error("store is unavailable"));
    await openInbox();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "store is unavailable",
    );
  });
});

/**
 * Notification permission reaches the pane, and a host that cannot answer
 * costs nothing.
 *
 * The stub client above has no `notifyStatus` at all, which is the older-host
 * case: the Inbox must be entirely unaffected. The other case is a host that
 * answers "denied", where the pane says so.
 */
describe("the Inbox and notification permission", () => {
  it("is unaffected by a host that cannot answer notify/status", async () => {
    // The default stub has no `notifyStatus` method whatsoever.
    await openInbox();

    // The cards are all there, and nothing claims banners are off.
    expect(await screen.findByText("Nightly NAS backup")).toBeInTheDocument();
    expect(
      screen.queryByText(/Notifications are turned off/),
    ).not.toBeInTheDocument();
  });

  it("tells the pane when the OS has refused banners", async () => {
    vi.mocked(connectHost).mockResolvedValue({
      client: {
        ...client(),
        notifyStatus: vi.fn(async () => ({
          supported: true,
          authorization: "denied" as const,
          kinds: ["needs_you"],
        })),
      } as unknown as HostClient,
      hello: HELLO,
    });

    await openInbox();

    expect(
      await screen.findByText(/Notifications are turned off/),
    ).toBeInTheDocument();
    // The cards are still drawn: this is an aside, not a replacement.
    expect(screen.getByText("Nightly NAS backup")).toBeInTheDocument();
  });
});

/**
 * The face on an Inbox row.
 *
 * Every host card wore the generic code mark, including cards on a named crew
 * member's thread, because `inbox/list` did not say whose thread it was. #22
 * was right to refuse to invent a bot; the fix was to put the id on the wire.
 */
describe("the Inbox and whose thread a card is on", () => {
  /** The stub host above serves no crew, which is the "roster has not loaded"
      case. These tests need one, because resolving a bot id to a face is the
      thing under test. */
  const CREW = {
    bots: [
      {
        botId: "writer",
        name: "Writer",
        color: "b-orange",
        instructions: "",
        tools: [],
        harnessId: "claude",
        isChief: false,
        memoryDir: "/data/bots/writer",
        sortOrder: 5,
        createdAt: "2026-08-20T10:00:00Z",
        updatedAt: "2026-08-20T10:00:00Z",
      },
    ],
    templates: [],
    hostTools: [],
  };

  function withBotCard(botId: string | undefined) {
    const listed = {
      ...INBOX,
      events: [{ ...INBOX.events[0], botId }],
      sleeping: [],
    };
    vi.mocked(connectHost).mockResolvedValue({
      client: {
        ...client(),
        inbox: vi.fn(async () => listed),
        listCrew: vi.fn(async () => CREW),
        listTools: vi.fn(async () => ({ tools: [] })),
        listHarnesses: vi.fn(async () => ({ harnesses: [], issues: [] })),
      } as unknown as HostClient,
      hello: HELLO,
    });
  }

  it("draws the bot's face on a card from that bot's thread", async () => {
    // `writer` is in the crew this stub host serves.
    withBotCard("writer");
    await openInbox();

    // The avatar is labelled with the bot's name; that label is the only thing
    // on the row that says who it is.
    expect(await screen.findByLabelText(/Writer/)).toBeInTheDocument();
  });

  it("keeps the code mark for a thread with no bot", async () => {
    withBotCard(undefined);
    await openInbox();
    await screen.findByText(INBOX.events[0].title);

    expect(screen.queryByLabelText(/Writer/)).not.toBeInTheDocument();
  });

  /**
   * The crew loads separately, so a card can arrive naming a bot the roster
   * does not have yet — or one that has since been removed. A face with the
   * wrong name on it would be worse than no face.
   */
  it("falls back to the code mark for a bot the roster does not have", async () => {
    withBotCard("nobody-by-that-id");
    await openInbox();
    await screen.findByText(INBOX.events[0].title);

    expect(screen.queryByLabelText(/Writer/)).not.toBeInTheDocument();
  });
});

/**
 * A thread the host owns that lives in no folder.
 *
 * `folder/list` walks folder rows, so a bot's standing thread — whose
 * `folder_id` is null — never appears in the flattened set the shell uses to
 * decide "is this the host's?". It does surface in the Inbox, because
 * `inbox/list` is not folder-scoped. So the card was clickable and led
 * nowhere: the main pane said "That thread is gone. Check the Inbox.", and a
 * fold or archive on it went to the mock reducer while the real permissions,
 * runs and process behind it were untouched.
 */
describe("a host thread that no folder lists", () => {
  const STANDING = "bot-writer";

  function withStandingThread(over: Record<string, unknown> = {}) {
    vi.mocked(connectHost).mockResolvedValue({
      client: {
        ...client(),
        inbox: vi.fn(async () => ({
          ...INBOX,
          events: [
            {
              ...INBOX.events[0],
              id: "ev-standing",
              threadId: STANDING,
              threadTitle: "Writer",
              title: "Overnight mail summarised",
            },
          ],
          sleeping: [],
        })),
        // Active, not resurfaced: "Open thread" runs `thread/reopen` first,
        // so by the time the pane asks `thread/state` the row is back.
        threadState: vi.fn(async () => ({
          threadId: STANDING,
          title: "Writer",
          state: "active",
          foldPolicy: "default",
          cwd: "/data/bots/writer",
          harnessId: "claude",
          botId: "writer",
          process: {
            connected: false,
            acpState: "idle",
            pendingPermissions: 0,
            resumable: false,
          },
          runs: [],
          unread: 0,
        })),
        threadTranscript: vi.fn(async () => ({
          threadId: STANDING,
          headSeq: 1,
          events: [],
          truncated: false,
          queued: [],
        })),
        ...over,
      } as unknown as HostClient,
      hello: HELLO,
    });
  }

  it("opens the thread instead of saying it is gone", async () => {
    withStandingThread();
    await openInbox();
    await expand(/^Overnight mail summarised/);
    await userEvent.click(screen.getByRole("button", { name: "Open thread" }));

    // The chat, not the dead end.
    await waitFor(() =>
      expect(screen.queryByText(/That thread is gone/)).not.toBeInTheDocument(),
    );
    expect(await screen.findByRole("heading", { level: 2, name: "Writer" })).toBeInTheDocument();
  });

  /**
   * The half that silently lost work. Folding a row the shell mistook for a
   * fixture moved it on screen and left the host's copy running.
   */
  it("folds it on the host rather than in the mock reducer", async () => {
    const fold = vi.fn(async () => ({ threadId: STANDING, state: "folded" }));
    withStandingThread({ fold });
    await openInbox();
    await expand(/^Overnight mail summarised/);
    await userEvent.click(screen.getByRole("button", { name: "Open thread" }));
    await screen.findByRole("heading", { level: 2, name: "Writer" });

    // Exact: the sidebar's folder-settings gear also starts with "Fold".
    await userEvent.click(screen.getByRole("button", { name: "Fold" }));
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Disappear until done/ }),
    );

    // "Disappear until done" sends no policy — the thread keeps the one it
    // has — so this is the whole of the call.
    await waitFor(() =>
      expect(fold).toHaveBeenCalledWith({ threadId: STANDING }),
    );
  });
});
