/**
 * New Chat is where a thread's harness is chosen (#6). The picker has to
 * default to something, report the exact id the host will resolve, and be
 * honest about a harness the machine does not have.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NewChatView } from "../components/NewChatView";
import type { Folder, HarnessCard } from "../components/types";
import { HARNESSES } from "../views/mock-host";

const FOLDERS: Folder[] = [
  { id: "jabot-app", name: "jabot-app", path: "~/code/jabot-app" },
  { id: "globnet-sync", name: "globnet-sync", path: "~/code/globnet-sync" },
];

function renderView(over: Partial<Parameters<typeof NewChatView>[0]> = {}) {
  const props = {
    harnesses: HARNESSES,
    folders: FOLDERS,
    onStart: vi.fn(),
    ...over,
  };
  render(<NewChatView {...props} />);
  return props;
}

async function pickHarness(name: RegExp) {
  await userEvent.click(screen.getByRole("button", { name: /Harness:/ }));
  await userEvent.click(screen.getByRole("option", { name }));
}

describe("NewChatView", () => {
  it("falls back when the saved default is disabled", async () => {
    const enabled = HARNESSES.filter((harness) => harness.id !== "claude");
    const props = renderView({ harnesses: enabled, defaultHarnessId: "claude" });
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(props.onStart).toHaveBeenCalledWith(expect.objectContaining({ harnessId: enabled[0].id }));
  });
  it("explains how to recover when all harnesses are disabled", async () => {
    const props = renderView({ harnesses: [], defaultHarnessId: "claude" });
    expect(screen.getByText("Enable a harness in Settings to start a chat.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Start session" })).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox", { name: /Plan, build/ }), "hello{Enter}");
    expect(props.onStart).not.toHaveBeenCalled();
  });

  it("is a chat window, not a dialog", () => {
    renderView();

    expect(screen.getByRole("region", { name: "New Chat" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.getByRole("textbox", { name: /Plan, build/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open folder" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "GitHub repository" }),
    ).toBeInTheDocument();
  });

  it("offers every catalog harness and pre-selects the first", async () => {
    renderView();

    await userEvent.click(screen.getByRole("button", { name: /Harness:/ }));
    for (const harness of HARNESSES) {
      expect(
        screen.getByRole("option", { name: new RegExp(harness.label) }),
      ).toBeInTheDocument();
    }
    expect(
      screen.getByRole("button", { name: /Harness: Claude Code/ }),
    ).toBeInTheDocument();
  });

  it("describes Pi as a coding agent, not Inflection's chatbot", async () => {
    renderView();

    await userEvent.click(screen.getByRole("button", { name: /Harness:/ }));
    const pi = screen.getByRole("option", { name: /^Pi/ });
    expect(pi).toHaveTextContent("Mario Zechner's coding agent");
    expect(pi).not.toHaveTextContent(/Inflection/i);
  });

  it("offers a model picker for OpenCode and sends the chosen model", async () => {
    const props = renderView({
      harnesses: HARNESSES.map((harness) =>
        harness.id === "opencode"
          ? { ...harness, models: ["anthropic/claude-sonnet-4-5"] }
          : harness,
      ),
    });
    await pickHarness(/OpenCode/);
    expect(
      screen.getByRole("button", { name: /Model: Project default/ }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Model:/ }));
    await userEvent.click(
      screen.getByRole("option", { name: "anthropic/claude-sonnet-4-5" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({
        harnessId: "opencode",
        model: "anthropic/claude-sonnet-4-5",
      }),
    );
  });

  it("starts the session with the harness, folder, and task picked", async () => {
    const props = renderView({ defaultFolderId: "globnet-sync" });

    await pickHarness(/^Pi/);
    expect(screen.queryByLabelText("WHAT SHOULD IT DO?")).toBeNull();
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "pi",
      folderId: "globnet-sync",
      task: "Untitled session",
    });
  });

  it("moves the selection when another harness is picked", async () => {
    renderView();

    await pickHarness(/Codex/);

    expect(
      screen.getByRole("button", { name: /Harness: Codex/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Harness: Claude Code/ }),
    ).toBeNull();
  });

  it("names an unnamed session rather than starting a blank one", async () => {
    const props = renderView();

    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ task: "Untitled session", folderId: null }),
    );
  });

  it("sends the typed prompt as the session's task", async () => {
    const props = renderView();

    await userEvent.type(
      screen.getByRole("textbox", { name: /Plan, build/ }),
      "Fix the auth middleware",
    );
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "claude",
      folderId: null,
      task: "Fix the auth middleware",
    });
  });

  it("says how to install a harness the Doctor could not find", () => {
    const missing: HarnessCard[] = [
      {
        id: "pi",
        label: "Pi",
        blurb: "Mario Zechner's coding agent",
        accent: "var(--h-pi)",
        available: false,
        installHint: "Install Pi, then `pi-acp` on PATH.",
      },
    ];
    renderView({ harnesses: missing });

    expect(
      screen.getByText("Install Pi, then `pi-acp` on PATH."),
    ).toBeInTheDocument();
  });

  it("starts with no folder until one is picked", () => {
    renderView();
    expect(
      screen.getByRole("button", { name: /Workspace: No folder/ }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("FOLDER")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(
      screen.queryByRole("group", { name: "Selected workspace" }),
    ).toBeNull();
  });

  it("shows the chosen workspace and lets it be cleared", async () => {
    const props = renderView({ defaultFolderId: "jabot-app" });
    expect(
      screen.getByRole("button", { name: /Workspace: jabot-app/ }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Workspace:/ }));
    await userEvent.click(screen.getByRole("option", { name: "No folder" }));
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );
    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "claude",
      folderId: null,
      task: "Untitled session",
    });
  });

  it("stays open on Escape so a half-typed prompt is not thrown away", async () => {
    const props = renderView();

    await userEvent.keyboard("{Escape}");

    expect(props.onStart).not.toHaveBeenCalled();
    expect(screen.getByRole("region", { name: "New Chat" })).toBeInTheDocument();
  });
});

/**
 * Where the thread will work (#23, #92).
 *
 * The card used to carry an Advanced disclosure with a checkout opt-out and a
 * base branch. Both are gone: a folder thread always gets a fresh worktree from
 * the host's own default base ref, which is what stops two threads in one repo
 * standing on each other's uncommitted work.
 *
 * `thread/open` still accepts `useCheckout` and `baseRef` — the Rust host
 * honours both and `tests/e2e/worktree.test.ts` drives them — but nothing the
 * window sends sets either.
 */
describe("NewChatView, where the thread will work", () => {
  it("offers no worktree controls with a folder picked", () => {
    renderView({ defaultFolderId: "jabot-app" });

    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByLabelText("BASE BRANCH")).toBeNull();
  });

  /** The load-bearing one. Every session sends the same three fields, and a
      `useCheckout: false` or an empty `baseRef` on the wire would be a
      different request than the one that has been shipping. */
  it("sends the three fields and nothing about the tree", async () => {
    const props = renderView({ defaultFolderId: "jabot-app" });

    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "claude",
      folderId: "jabot-app",
      task: "Untitled session",
    });
  });

  /** "No folder" is a scratch session: no checkout to work in and no branch to
      fork from, and the window says nothing about either here too. */
  it("offers nothing about the tree without a folder either", () => {
    renderView({ defaultFolderId: null });

    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
    expect(screen.queryByLabelText("BASE BRANCH")).toBeNull();
  });
});

describe("workspace entry points", () => {
  const actions = () => ({
    pickFolder: vi.fn(async () => "globnet-sync"),
    listRepositories: vi.fn(async () => [
      { full_name: "team/private-app", description: "Our app", private: true },
    ]),
    pickRepository: vi.fn(async () => "jabot-app"),
    signedIn: true,
    signIn: vi.fn(),
  });
  it("browses a folder and starts without a prompt", async () => {
    const workspaceActions = actions();
    const props = renderView({ workspaceActions });
    await userEvent.click(screen.getByRole("button", { name: /Open folder/ }));
    expect(workspaceActions.pickFolder).toHaveBeenCalledOnce();
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );
    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({
        folderId: "globnet-sync",
        task: "Untitled session",
      }),
    );
  });
  it("lists authenticated repositories and selects the cloned folder", async () => {
    const workspaceActions = actions();
    const props = renderView({ workspaceActions });
    await userEvent.click(
      screen.getByRole("button", { name: /GitHub repository/ }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /team\/private-app/ }),
    );
    expect(workspaceActions.pickRepository).toHaveBeenCalledWith(
      "team/private-app",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );
    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ folderId: "jabot-app" }),
    );
  });
  it("keeps selection after cancelling the native chooser", async () => {
    const workspaceActions = {
      ...actions(),
      pickFolder: vi.fn(async () => null),
    };
    renderView({ workspaceActions, defaultFolderId: "jabot-app" });
    await userEvent.click(screen.getByRole("button", { name: /Open folder/ }));
    expect(
      screen.getByRole("button", { name: /Workspace: jabot-app/ }),
    ).toBeInTheDocument();
  });
  it("shows clone failures and keeps the repository available for retry", async () => {
    const workspaceActions = {
      ...actions(),
      pickRepository: vi.fn(async () => {
        throw new Error("Clone failed");
      }),
    };
    renderView({ workspaceActions });
    await userEvent.click(
      screen.getByRole("button", { name: /GitHub repository/ }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /team\/private-app/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("Clone failed");
    expect(
      screen.getByRole("button", { name: /team\/private-app/ }),
    ).toBeEnabled();
  });
  it("offers sign-in before requesting repositories", async () => {
    const workspaceActions = { ...actions(), signedIn: false };
    renderView({ workspaceActions });
    await userEvent.click(
      screen.getByRole("button", { name: /GitHub repository/ }),
    );
    expect(workspaceActions.signIn).toHaveBeenCalledOnce();
    expect(workspaceActions.listRepositories).not.toHaveBeenCalled();
  });
});
