/**
 * Fold, Archive and Delete, from the affordance to the host call (#26).
 *
 * `tests/e2e/fold.test.ts` proves what the *host* does with a live session.
 * What is checked here is the half a live host cannot check: that the three
 * places a user can fold from — the sidebar row's menu, the chat header of the
 * thread they are reading, and Chief's card — all send `thread/fold` for a
 * thread the host owns rather than quietly moving a fixture, that the policy
 * they picked is the policy on the wire, and that a fold the host refuses puts
 * the row back instead of leaving the sidebar lying about it.
 *
 * The policy assertions are the ones worth reading twice. "Disappear until
 * done" sends *no* `policy` field, because `state-machine.md` gives that
 * gesture the thread's existing policy; sending `default` would silently undo
 * a quieter one the user chose earlier.
 *
 * The other two items on the same menu are here for a blunter reason: they
 * used to reach the mock reducer even for a thread the host owned. A Delete
 * that only hides a row leaves the adapter running, the permissions
 * outstanding, the run open and the worktree orphaned — and the row comes back
 * on the next `folder/list`.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { FoldButton } from "../components/FoldButton";
import { ThreadContextMenu } from "../components/ThreadContextMenu";
import type { ThreadState } from "../components/types";
import {
  connectHost,
  HostRpcError,
  RPC_ERROR,
  type FolderListResult,
  type FolderThreadView,
  type FolderView,
  type HelloResult,
  type HostClient,
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

const AUTH: FolderThreadView = {
  threadId: "t-auth",
  folderId: "f-jabot",
  harnessId: "claude",
  title: "Auth migration",
  state: "active",
  foldPolicy: "default",
  runState: "running",
};

function folder(threads: FolderThreadView[]): FolderView {
  return {
    folderId: "f-jabot",
    name: "jabot",
    path: "/Users/j/code/jabot",
    cwd: "/Users/j/code/jabot",
    isGit: false,
    filesToCopy: [],
    sortOrder: 0,
    threads,
  };
}

const folded: ThreadStateResult = {
  threadId: "t-auth",
  title: "Auth migration",
  state: "folded",
  foldPolicy: "default",
  cwd: "/Users/j/code/jabot",
  harnessId: "claude",
  folderId: "f-jabot",
  process: {
    connected: true,
    acpState: "running",
    pendingPermissions: 0,
    resumable: false,
  },
  runs: [],
  unread: 0,
};

describe("FoldButton", () => {
  it("names both policies and says what each one lets the host answer", async () => {
    const onFold = vi.fn();
    render(<FoldButton onFold={onFold} />);

    await userEvent.click(screen.getByRole("button", { name: "Fold" }));
    const menu = screen.getByRole("menu", { name: "Fold" });

    // A permission policy the user cannot read is one they cannot consent to.
    expect(
      within(menu).getByText(/reads are allowed while you are away/),
    ).toBeInTheDocument();
    expect(within(menu).getByText(/Never an execute or a delete/)).toBeInTheDocument();

    await userEvent.click(
      within(menu).getByRole("menuitem", { name: /Disappear until done/ }),
    );
    // No policy at all — the gesture keeps whatever the thread already had.
    expect(onFold.mock.calls[0][0]).toBeUndefined();

    await userEvent.click(screen.getByRole("button", { name: "Fold" }));
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Wait for Inbox/ }),
    );
    expect(onFold).toHaveBeenLastCalledWith("wait_for_inbox");
  });
});

describe("ThreadContextMenu", () => {
  function menu(state: ThreadState, onFold = vi.fn()) {
    render(
      <ThreadContextMenu
        threadTitle="Auth migration"
        threadState={state}
        position={{ x: 10, y: 10 }}
        onFold={onFold}
        onArchive={vi.fn()}
        onDelete={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    return onFold;
  }

  it("offers both folds on an active row", async () => {
    const onFold = menu("active");

    await userEvent.click(
      screen.getByRole("menuitem", { name: /Disappear until done/ }),
    );
    expect(onFold).toHaveBeenCalled();
    expect(onFold.mock.calls[0][0]).toBeUndefined();
  });

  it("offers neither on a row that has already come back to you", () => {
    menu("resurfaced");

    // The transition table refuses to re-sleep a resurfaced thread, so
    // offering the gesture would be an error message where an affordance
    // should have been. Archive and Delete are still legal from there.
    expect(screen.queryByRole("menuitem", { name: /Disappear/ })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Wait for Inbox/ })).toBeNull();
    expect(screen.getByRole("menuitem", { name: /Archive/ })).toBeInTheDocument();
  });
});

describe("folding a host-owned thread", () => {
  const listFolders = vi.fn<() => Promise<FolderListResult>>();
  const fold = vi.fn<(params: unknown) => Promise<ThreadStateResult>>();
  const archiveThread = vi.fn<(params: unknown) => Promise<ThreadStateResult>>();
  const deleteThread = vi.fn<(params: unknown) => Promise<ThreadStateResult>>();

  function client(): HostClient {
    return {
      disconnect: vi.fn(),
      listFolders,
      fold,
      archiveThread,
      deleteThread,
      onNotification: vi.fn(() => () => {}),
      threadTranscript: vi.fn(async () => ({
        threadId: "t-auth",
        headSeq: 0,
        events: [],
        truncated: false,
        queued: [],
      })),
      pendingPermissions: vi.fn(async () => ({ requests: [] })),
    } as unknown as HostClient;
  }

  beforeEach(() => {
    listFolders.mockReset();
    fold.mockReset();
    archiveThread.mockReset();
    deleteThread.mockReset();
    // The host writes the row before it answers, so the `folder/list` that
    // follows a fold has to have lost the thread. A static mock would let the
    // "row leaves the sidebar" assertion pass on a detail the host does not
    // actually have.
    let asleep = false;
    listFolders.mockImplementation(async () => ({
      folders: [folder(asleep ? [] : [AUTH])],
    }));
    fold.mockImplementation(async () => {
      asleep = true;
      return folded;
    });
    // Archive and Delete take the row out of `folder/list` for good, the same
    // way the host does — the row is written before the call answers.
    archiveThread.mockImplementation(async () => {
      asleep = true;
      return { ...folded, state: "archived" };
    });
    deleteThread.mockImplementation(async () => {
      asleep = true;
      return { ...folded, state: "deleted" };
    });
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });
  });

  /**
   * Render, and wait for the *host's* folders to have replaced the fixtures.
   *
   * The fixture sidebar has an "Auth migration" row of its own, so waiting on
   * the row title would hand back a node React is about to throw away — and a
   * right-click on a detached node does nothing at all. "New thread in jabot"
   * only exists once `folder/list` has answered.
   */
  async function renderApp() {
    render(<App />);
    await screen.findByRole("button", { name: "New thread in jabot" });
  }

  const authRow = () => screen.getByRole("button", { name: /Auth migration/ });

  it("sends thread/fold from the row's menu and drops the row", async () => {
    await renderApp();
    await userEvent.pointer({ keys: "[MouseRight]", target: authRow() });
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Wait for Inbox/ }),
    );

    await waitFor(() =>
      expect(fold).toHaveBeenCalledWith({
        threadId: "t-auth",
        policy: "wait_for_inbox",
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Auth migration/ }),
      ).not.toBeInTheDocument(),
    );
  });

  it("folds from the chat you are reading, and stops pointing at it", async () => {
    await renderApp();
    await userEvent.click(authRow());
    expect(
      await screen.findByRole("heading", { name: "Auth migration" }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Fold" }));
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Disappear until done/ }),
    );

    // No `policy` key: the plain fold keeps the thread's own policy.
    await waitFor(() => expect(fold).toHaveBeenCalledWith({ threadId: "t-auth" }));
    // The pane cannot stay on a thread the user just sent away — that is the
    // one screen guaranteed to have nothing to show.
    await waitFor(() =>
      expect(
        screen.getByRole("heading", { level: 1, name: "Inbox" }),
      ).toBeInTheDocument(),
    );
  });

  it("sends thread/archive from the row's menu rather than moving a fixture", async () => {
    await renderApp();
    await userEvent.pointer({ keys: "[MouseRight]", target: authRow() });
    await userEvent.click(screen.getByRole("menuitem", { name: /Archive/ }));

    // The reducer knows how to hide a row. Only the host withdraws the
    // outstanding permissions, drains the queued prompts, closes the run and
    // releases the worktree — all of which would have kept running behind a
    // row that had quietly stopped being drawn.
    await waitFor(() =>
      expect(archiveThread).toHaveBeenCalledWith({ threadId: "t-auth" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Auth migration/ }),
      ).not.toBeInTheDocument(),
    );
  });

  it("sends thread/delete from the row's menu rather than moving a fixture", async () => {
    await renderApp();
    await userEvent.pointer({ keys: "[MouseRight]", target: authRow() });
    await userEvent.click(screen.getByRole("menuitem", { name: /Delete/ }));

    await waitFor(() =>
      expect(deleteThread).toHaveBeenCalledWith({ threadId: "t-auth" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Auth migration/ }),
      ).not.toBeInTheDocument(),
    );
  });

  it("puts the row back and says why when the host refuses an archive", async () => {
    archiveThread.mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.ILLEGAL_TRANSITION,
        message: "cannot archive an archived thread",
      }),
    );
    await renderApp();
    await userEvent.pointer({ keys: "[MouseRight]", target: authRow() });
    await userEvent.click(screen.getByRole("menuitem", { name: /Archive/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That thread is already archived.",
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Auth migration/ }),
      ).toBeInTheDocument(),
    );
  });

  it("puts the row back and says why when the host refuses the fold", async () => {
    fold.mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.ILLEGAL_TRANSITION,
        message: "cannot fold a resurfaced thread",
      }),
    );
    await renderApp();
    await userEvent.pointer({ keys: "[MouseRight]", target: authRow() });
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Disappear until done/ }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That thread has already come back to you.",
    );
    // The leave animation already pulled the row off screen; a refusal has to
    // bring it back, or the sidebar is quietly lying about what exists.
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Auth migration/ }),
      ).toBeInTheDocument(),
    );
  });
});

describe("Chief's card", () => {
  it("folds the thread it names on the host, not in the fixtures", async () => {
    const fold = vi.fn(async () => folded);
    // Chief's fixture card is about "auth"; give the host a row with that id
    // so the card is pointing at a thread the host owns.
    const chiefsThread: FolderThreadView = { ...AUTH, threadId: "auth" };
    vi.mocked(connectHost).mockResolvedValue({
      client: {
        disconnect: vi.fn(),
        listFolders: vi.fn(async () => ({ folders: [folder([chiefsThread])] })),
        fold,
        onNotification: vi.fn(() => () => {}),
        threadTranscript: vi.fn(async () => ({
          threadId: "auth",
          headSeq: 0,
          events: [],
          truncated: false,
          queued: [],
        })),
        pendingPermissions: vi.fn(async () => ({ requests: [] })),
      } as unknown as HostClient,
      hello: HELLO,
    });

    render(<App />);
    await screen.findByText("This Mac · v0.1.0");
    await userEvent.click(
      await screen.findByRole("button", { name: "Disappear until done" }),
    );

    // Chief's card is an affordance on a thread, so it has to reach the same
    // host call the sidebar's menu does — and with the same policy rule.
    await waitFor(() => expect(fold).toHaveBeenCalledWith({ threadId: "auth" }));
  });
});
