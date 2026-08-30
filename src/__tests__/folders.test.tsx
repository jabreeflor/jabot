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
  type FolderUpdateParams,
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
  /** The two fields folder settings edits (#16). Not carried before, so the
      settings card had nothing to seed itself from. */
  it("carries the editable fields onto the sidebar's shape", () => {
    const [row] = folderRows({
      folders: [
        folder({ setupCommand: "npm ci", filesToCopy: [".env", ".env.local"] }),
      ],
    });

    expect(row.setupCommand).toBe("npm ci");
    expect(row.filesToCopy).toEqual([".env", ".env.local"]);
    // A folder with neither keeps `undefined` and an empty list, which is what
    // the host answers and what the form reads as "nothing set".
    const [bare] = folderRows({ folders: [folder()] });
    expect(bare.setupCommand).toBeUndefined();
    expect(bare.filesToCopy).toEqual([]);
  });

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
  const updateFolder = vi.fn<(p: FolderUpdateParams) => Promise<FolderView>>();

  function client(): HostClient {
    return {
      disconnect: vi.fn(),
      listFolders,
      openThread,
      updateFolder,
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
    updateFolder.mockReset().mockImplementation(async () => folder());
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

/**
 * Folder settings (#16, #23).
 *
 * `folder/update` has been routed and handled since #16 — name, setup command,
 * files-to-copy, and a `refresh` that asks git again — and `client.updateFolder`
 * had no caller anywhere in `src/`. So everything a folder knows was typed once
 * at registration and frozen, and a wrong setup command silently produced a
 * half-built worktree on every thread with no way to correct it.
 */
describe("editing a registered folder", () => {
  const listFolders = vi.fn<() => Promise<FolderListResult>>();
  const updateFolder = vi.fn<(p: FolderUpdateParams) => Promise<FolderView>>();

  function client(): HostClient {
    return {
      disconnect: vi.fn(),
      listFolders,
      updateFolder,
      onNotification: vi.fn(() => () => {}),
      pendingPermissions: vi.fn(async () => ({ requests: [] })),
    } as unknown as HostClient;
  }

  beforeEach(() => {
    listFolders.mockReset().mockResolvedValue({
      folders: [
        folder({ setupCommand: "npm ci", filesToCopy: [".env", ".env.local"] }),
      ],
    });
    updateFolder.mockReset().mockImplementation(async () => folder());
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });
  });

  async function openSettings() {
    render(<App />);
    await screen.findByText("This Mac · v0.1.0");
    await userEvent.click(
      await screen.findByRole("button", { name: "Folder settings for jabot" }),
    );
    return screen.findByRole("heading", { name: "Folder — jabot" });
  }

  it("opens pre-filled with what the host holds", async () => {
    await openSettings();

    expect(screen.getByLabelText("DISPLAY NAME")).toHaveValue("jabot");
    expect(screen.getByLabelText(/SETUP SCRIPT/)).toHaveValue("npm ci");
    expect(screen.getByLabelText(/FILES TO COPY/)).toHaveValue(
      ".env, .env.local",
    );
    // The path is shown and not editable: the host has no move method, and a
    // folder pointed somewhere else is a different folder.
    expect(screen.getByText("/Users/j/code/jabot")).toBeInTheDocument();
  });

  /**
   * The load-bearing one. The host reads an absent field as "leave it alone",
   * so sending everything on every save would let an unrelated edit here
   * silently overwrite what another window had just changed.
   */
  it("sends only what actually moved", async () => {
    await openSettings();

    await userEvent.clear(screen.getByLabelText(/SETUP SCRIPT/));
    await userEvent.type(screen.getByLabelText(/SETUP SCRIPT/), "pnpm install");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(updateFolder).toHaveBeenCalledWith({
        folderId: "f-jabot",
        setupCommand: "pnpm install",
      }),
    );
  });

  /** The one field where the empty string is a value rather than an absence.
      Omitting it would mean a setup command could never be removed. */
  it("clears the setup command with an empty string, not by omitting it", async () => {
    await openSettings();

    await userEvent.clear(screen.getByLabelText(/SETUP SCRIPT/));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(updateFolder).toHaveBeenCalledWith({
        folderId: "f-jabot",
        setupCommand: "",
      }),
    );
  });

  it("sends the edited files-to-copy list, split the way it is typed", async () => {
    await openSettings();

    await userEvent.clear(screen.getByLabelText(/FILES TO COPY/));
    await userEvent.type(
      screen.getByLabelText(/FILES TO COPY/),
      ".env,  config/local.json ",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(updateFolder).toHaveBeenCalledWith({
        folderId: "f-jabot",
        filesToCopy: [".env", "config/local.json"],
      }),
    );
  });

  /** A remote added or re-pointed since registration. The host re-probes the
      directory rather than being told what it found. */
  it("asks git again without sending any edits", async () => {
    await openSettings();

    await userEvent.click(screen.getByRole("button", { name: "Ask git again" }));

    await waitFor(() =>
      expect(updateFolder).toHaveBeenCalledWith({
        folderId: "f-jabot",
        refresh: true,
      }),
    );
  });

  it("redraws the sidebar from the host's answer", async () => {
    listFolders.mockResolvedValueOnce({
      folders: [folder({ isGit: false, origin: undefined })],
    });
    render(<App />);
    await screen.findByText("This Mac · v0.1.0");
    expect(await screen.findByText("no git")).toBeInTheDocument();

    listFolders.mockResolvedValue({ folders: [folder()] });
    await userEvent.click(
      screen.getByRole("button", { name: "Folder settings for jabot" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Ask git again" }));

    // The re-probe found a checkout, so the badge the old answer earned goes.
    await waitFor(() => expect(screen.queryByText("no git")).toBeNull());
  });

  /** Same promise the Add card makes: a refused save keeps the draft, because
      the fix is one edit away and retyping it is not. */
  it("keeps the card open and says why when the host refuses", async () => {
    updateFolder.mockRejectedValue(
      new HostRpcError({
        code: RPC_ERROR.INVALID_PARAMS,
        message: "setupCommand is longer than 2000 characters",
      }),
    );
    await openSettings();

    await userEvent.clear(screen.getByLabelText(/SETUP SCRIPT/));
    await userEvent.type(screen.getByLabelText(/SETUP SCRIPT/), "make all");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "setupCommand is longer than 2000 characters",
    );
    expect(screen.getByLabelText(/SETUP SCRIPT/)).toHaveValue("make all");
  });
});
