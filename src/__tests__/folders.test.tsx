/**
 * Folders, from the wire to the sidebar (#16).
 *
 * Three things are worth asserting here and the host is not one of them —
 * `tests/e2e/folders.test.ts` drives the real host for that. What is checked
 * here is the renderer's side of the contract: that a `folder/list` result
 * becomes sidebar rows without being reshaped, that "New thread in …" opens a
 * thread in *that* folder's cwd, and that a registration the host refuses
 * leaves the user's typing on screen.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { AddFolderModal } from "../components/AddFolderModal";
import {
  connectHost,
  HostRpcError,
  RPC_ERROR,
  type FolderListResult,
  type FolderView,
  type HelloResult,
  type HostClient,
  type ThreadOverlayState,
  type ThreadStateResult,
} from "../host";
import { folderRows } from "../views/folders";

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

function folder(overrides: Partial<FolderView> = {}): FolderView {
  return {
    folderId: "f-jabot",
    name: "jabot",
    path: "/Users/j/code/jabot",
    cwd: "/Users/j/code/jabot",
    repoRoot: "/Users/j/code/jabot",
    isGit: true,
    origin: {
      url: "git@github.com:jabreeflor/jabot.git",
      host: "github.com",
      owner: "jabreeflor",
      name: "jabot",
      repo: "jabreeflor/jabot",
    },
    defaultBranch: "main",
    filesToCopy: [],
    sortOrder: 0,
    threads: [],
    ...overrides,
  };
}

describe("folderRows", () => {
  it("maps a folder/list result onto the sidebar's own shape", () => {
    const result: FolderListResult = {
      folders: [
        folder({
          threads: [
            {
              threadId: "t-auth",
              folderId: "f-jabot",
              botId: "code",
              harnessId: "claude",
              title: "Auth migration",
              state: "resurfaced",
              foldPolicy: "default",
              runState: "succeeded",
              preview: "Middleware rewritten",
            },
          ],
        }),
      ],
    };

    const [row] = folderRows(result);
    expect(row).toMatchObject({
      id: "f-jabot",
      name: "jabot",
      path: "/Users/j/code/jabot",
      cwd: "/Users/j/code/jabot",
      isGit: true,
      repo: "jabreeflor/jabot",
    });
    expect(row.threads[0]).toEqual({
      id: "t-auth",
      folderId: "f-jabot",
      botId: "code",
      harnessId: "claude",
      title: "Auth migration",
      state: "resurfaced",
      foldPolicy: "default",
      runState: "succeeded",
      preview: "Middleware rewritten",
    });
  });

  it("carries the absent answers through as absent", () => {
    const [row] = folderRows({
      folders: [
        folder({
          isGit: false,
          repoRoot: undefined,
          origin: undefined,
          cwd: "/Users/j/notes",
          path: "/Users/j/notes",
          threads: [
            {
              threadId: "t-1",
              harnessId: "claude",
              title: "Scratch",
              state: "active",
              foldPolicy: "default",
            },
          ],
        }),
      ],
    });
    expect(row.isGit).toBe(false);
    expect(row.repo).toBeUndefined();
    // A thread that has never run has no run state, which is not "queued".
    expect(row.threads[0].runState).toBeNull();
    expect(row.threads[0].folderId).toBeNull();
  });
});

describe("AddFolderModal", () => {
  it("sends the path the user typed, trimming the optional fields away", async () => {
    const onRegister = vi.fn().mockResolvedValue(folder());
    const onCancel = vi.fn();
    render(<AddFolderModal onRegister={onRegister} onCancel={onCancel} />);

    await userEvent.type(
      screen.getByLabelText("FOLDER — ONE REPO"),
      "  ~/code/jabot  ",
    );
    await userEvent.type(
      screen.getByLabelText("FILES TO COPY — OPTIONAL"),
      ".env, .env.local",
    );
    await userEvent.click(screen.getByRole("button", { name: "Add folder" }));

    expect(onRegister).toHaveBeenCalledWith({
      path: "~/code/jabot",
      name: undefined,
      setupCommand: undefined,
      filesToCopy: [".env", ".env.local"],
    });
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
  });

  it("keeps the form and says so when the host refuses the directory", async () => {
    const onRegister = vi.fn().mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.FOLDER_EXISTS,
        message: "/Users/j/code/jabot is already registered",
      }),
    );
    const onCancel = vi.fn();
    render(<AddFolderModal onRegister={onRegister} onCancel={onCancel} />);

    await userEvent.type(
      screen.getByLabelText("FOLDER — ONE REPO"),
      "~/code/jabot",
    );
    await userEvent.click(screen.getByRole("button", { name: "Add folder" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That checkout is already a folder.",
    );
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByLabelText("FOLDER — ONE REPO")).toHaveValue(
      "~/code/jabot",
    );
  });
});

describe("App, once the host has answered", () => {
  const listFolders = vi.fn<() => Promise<FolderListResult>>();
  const openThread = vi.fn<() => Promise<ThreadStateResult>>();

  function client(): HostClient {
    return {
      disconnect: vi.fn(),
      listFolders,
      openThread,
      // Selecting a host-owned thread now renders it live (#14): it hydrates
      // from `thread/transcript` and subscribes for `session/update`. A stub
      // without these is a host the renderer cannot talk to at all, so the
      // stub grows them rather than the app checking whether they exist.
      onNotification: vi.fn(() => () => {}),
      threadTranscript: vi.fn(async () => ({
        threadId: "t-new",
        headSeq: 0,
        events: [],
        truncated: false,
        queued: [],
      })),
      // And it asks the broker what the agent is still waiting on (#20) — a
      // thread reopened after a quit draws that card from here, not from the
      // transcript.
      pendingPermissions: vi.fn(async () => ({ requests: [] })),
    } as unknown as HostClient;
  }

  beforeEach(() => {
    listFolders.mockReset();
    openThread.mockReset();
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });
  });

  async function renderApp() {
    render(<App />);
    await screen.findByText("This Mac · v0.1.0");
  }

  it("draws the registered folders instead of the fixtures", async () => {
    listFolders.mockResolvedValue({
      folders: [
        folder({
          name: "jabot",
          threads: [
            {
              threadId: "t-auth",
              folderId: "f-jabot",
              harnessId: "claude",
              title: "Auth migration",
              state: "active",
              foldPolicy: "default",
              runState: "running",
            },
          ],
        }),
        folder({
          folderId: "f-notes",
          name: "notes",
          path: "/Users/j/notes",
          cwd: "/Users/j/notes",
          repoRoot: undefined,
          origin: undefined,
          isGit: false,
        }),
      ],
    });
    await renderApp();

    await screen.findByRole("button", { name: "New thread in jabot" });
    expect(
      screen.getByRole("button", { name: /Auth migration/ }),
    ).toBeInTheDocument();
    // The fixture folders are gone the moment the host has its own answer.
    expect(
      screen.queryByRole("button", { name: "New thread in globnet-sync" }),
    ).not.toBeInTheDocument();
    // A directory git does not claim says so rather than disappearing.
    const notes = screen
      .getByRole("button", { name: /^notes/ })
      .closest(".folder-head");
    expect(within(notes as HTMLElement).getByText("no git")).toBeInTheDocument();
  });

  it("starts a thread in the folder's own checkout", async () => {
    const registered = folder();
    // The host persists the row before it answers, so the `folder/list` that
    // follows `thread/open` carries the new thread. A static mock would let
    // the assertion below pass or fail on a detail the host does not have.
    let opened = false;
    listFolders.mockImplementation(async () => ({
      folders: [
        {
          ...registered,
          threads: opened
            ? [
                {
                  threadId: "t-new",
                  folderId: registered.folderId,
                  harnessId: "codex",
                  title: "Rotate the backup keys",
                  state: "active" as ThreadOverlayState,
                  foldPolicy: "default" as const,
                },
              ]
            : [],
        },
      ],
    }));
    openThread.mockImplementation(async () => {
      opened = true;
      return {
        threadId: "t-new",
        title: "Rotate the backup keys",
        state: "active" as ThreadOverlayState,
        foldPolicy: "default" as const,
        cwd: registered.cwd,
        harnessId: "codex",
        folderId: registered.folderId,
        process: {
          connected: false,
          acpState: "unknown" as const,
          pendingPermissions: 0,
          // A thread with no session and no receipt: nothing to resume, which
          // is what the real host answers for a row `thread/open` just made.
          resumable: false,
        },
        runs: [],
        unread: 0,
      };
    });
    await renderApp();

    await userEvent.click(
      await screen.findByRole("button", { name: "New thread in jabot" }),
    );
    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Rotate the backup keys",
    );
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));

    // The cwd is the folder's, not the app's, and the folder travels with it —
    // that pair is what the host stamps the row with (setup-porting §19).
    expect(openThread).toHaveBeenCalledWith({
      title: "Rotate the backup keys",
      cwd: "/Users/j/code/jabot",
      harnessId: "codex",
      folderId: "f-jabot",
    });
    expect(
      await screen.findByRole("heading", { name: "Rotate the backup keys" }),
    ).toBeInTheDocument();
  });

  /**
   * The worktree controls reaching the host (#23).
   *
   * `useCheckout` and `baseRef` have been on `thread/open` and honoured by the
   * Rust host since #23, and no renderer ever set them. This is the trip the
   * modal's own tests cannot make: draft to wire.
   */
  it("carries the advanced worktree choices through to thread/open", async () => {
    const registered = folder();
    listFolders.mockResolvedValue({ folders: [registered] });
    openThread.mockResolvedValue({
      threadId: "t-new",
      title: "Rotate the backup keys",
      state: "active" as ThreadOverlayState,
      foldPolicy: "default" as const,
      cwd: registered.cwd,
      harnessId: "claude",
      folderId: registered.folderId,
      process: {
        connected: false,
        acpState: "unknown" as const,
        pendingPermissions: 0,
        resumable: false,
      },
      runs: [],
      unread: 0,
    });
    await renderApp();

    await userEvent.click(
      await screen.findByRole("button", { name: "New thread in jabot" }),
    );
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Rotate the backup keys",
    );
    await userEvent.click(screen.getByRole("button", { name: "Advanced" }));
    await userEvent.type(
      screen.getByLabelText("BASE BRANCH"),
      "release/2.0",
    );
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));

    expect(openThread).toHaveBeenCalledWith(
      expect.objectContaining({
        folderId: "f-jabot",
        baseRef: "release/2.0",
      }),
    );
  });

  /** A base ref the repository does not have is the host's to refuse, and its
      sentence is the useful one — "v9.9.9 is not a commit in this repository"
      rather than "could not start". The card keeps the draft either way. */
  it("shows the host's refusal of a base ref that does not resolve", async () => {
    listFolders.mockResolvedValue({ folders: [folder()] });
    openThread.mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.WORKTREE_FAILED,
        message: "v9.9.9 is not a commit in this repository",
      }),
    );
    await renderApp();

    await userEvent.click(
      await screen.findByRole("button", { name: "New thread in jabot" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Advanced" }));
    await userEvent.type(screen.getByLabelText("BASE BRANCH"), "v9.9.9");
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "v9.9.9 is not a commit in this repository",
    );
    // And the card is still holding what was typed, so the fix is one edit.
    expect(screen.getByLabelText("BASE BRANCH")).toHaveValue("v9.9.9");
  });

  it("keeps the New Chat card open when the host refuses the spawn", async () => {
    listFolders.mockResolvedValue({ folders: [folder()] });
    openThread.mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.HARNESS_UNAVAILABLE,
        message: "Harness unavailable: codex-acp",
      }),
    );
    await renderApp();

    await userEvent.click(
      await screen.findByRole("button", { name: "New thread in jabot" }),
    );
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Rotate the backup keys",
    );
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Harness unavailable: codex-acp",
    );
    expect(screen.getByLabelText("WHAT SHOULD IT DO?")).toHaveValue(
      "Rotate the backup keys",
    );
  });

  it("says the sidebar is empty rather than looking broken", async () => {
    listFolders.mockResolvedValue({ folders: [] });
    await renderApp();

    expect(await screen.findByText(/No folders yet/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add folder" }),
    ).toBeInTheDocument();
  });

  it("falls back to the fixtures when the host cannot list folders", async () => {
    listFolders.mockRejectedValue(new Error("store unavailable"));
    await renderApp();

    // The shell is still usable, and it is honest about which rows it has:
    // the fixtures, unchanged.
    expect(
      await screen.findByRole("button", { name: "New thread in jabot-app" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/No folders yet/)).not.toBeInTheDocument();
  });
});
