/**
 * New Chat is where a thread's harness is chosen (#6). The picker has to
 * default to something, report the exact id the host will resolve, and be
 * honest about a harness the machine does not have.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NewChatModal } from "../components/NewChatModal";
import type { Folder, HarnessCard } from "../components/types";
import { HARNESSES } from "../views/mock-host";

const FOLDERS: Folder[] = [
  { id: "jabot-app", name: "jabot-app", path: "~/code/jabot-app" },
  { id: "globnet-sync", name: "globnet-sync", path: "~/code/globnet-sync" },
];

function renderModal(over: Partial<Parameters<typeof NewChatModal>[0]> = {}) {
  const props = {
    harnesses: HARNESSES,
    folders: FOLDERS,
    onStart: vi.fn(),
    onCancel: vi.fn(),
    ...over,
  };
  render(<NewChatModal {...props} />);
  return props;
}

describe("NewChatModal", () => {
  it("offers every catalog harness and pre-selects the first", () => {
    renderModal();

    for (const harness of HARNESSES) {
      expect(
        screen.getByRole("button", { name: new RegExp(harness.label) }),
      ).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: /Claude Code/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("describes Pi as a coding agent, not Inflection's chatbot", () => {
    renderModal();

    const pi = screen.getByRole("button", { name: /^Pi/ });
    expect(pi).toHaveTextContent("Mario Zechner's coding agent");
    expect(pi).not.toHaveTextContent(/Inflection/i);
  });

  it("starts the session with the harness, folder, and task picked", async () => {
    const props = renderModal({ defaultFolderId: "globnet-sync" });

    await userEvent.click(screen.getByRole("button", { name: /^Pi/ }));
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
    renderModal();

    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));

    expect(screen.getByRole("button", { name: /Codex/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: /Claude Code/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("names an unnamed session rather than starting a blank one", async () => {
    const props = renderModal();

    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ task: "Untitled session", folderId: null }),
    );
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
    renderModal({ harnesses: missing });

    expect(
      screen.getByText("Install Pi, then `pi-acp` on PATH."),
    ).toBeInTheDocument();
  });

  it("does not offer a second folder dropdown", () => {
    renderModal();
    expect(screen.queryByLabelText("FOLDER")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(screen.queryByRole("button", { name: "No folder" })).toBeNull();
    expect(
      screen.queryByRole("group", { name: "Selected workspace" }),
    ).toBeNull();
  });

  it("shows the chosen workspace and lets it be removed", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });
    expect(
      screen.getByRole("group", { name: "Selected workspace" }),
    ).toHaveTextContent("~/code/jabot-app");
    await userEvent.click(
      screen.getByRole("button", { name: "Remove selected workspace" }),
    );
    expect(
      screen.queryByRole("group", { name: "Selected workspace" }),
    ).toBeNull();
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );
    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "claude",
      folderId: null,
      task: "Untitled session",
    });
  });

  it("closes on Escape without starting anything", async () => {
    const props = renderModal();

    await userEvent.keyboard("{Escape}");

    expect(props.onCancel).toHaveBeenCalled();
    expect(props.onStart).not.toHaveBeenCalled();
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
 * card sends sets either.
 */
describe("NewChatModal, where the thread will work", () => {
  it("offers no worktree controls with a folder picked", () => {
    renderModal({ defaultFolderId: "jabot-app" });

    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByLabelText("BASE BRANCH")).toBeNull();
  });

  /** The load-bearing one. Every session sends the same three fields, and a
      `useCheckout: false` or an empty `baseRef` on the wire would be a
      different request than the one that has been shipping. */
  it("sends the three fields and nothing about the tree", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });

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
      fork from, and the card says nothing about either here too. */
  it("offers nothing about the tree without a folder either", () => {
    renderModal({ defaultFolderId: null });

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
    const props = renderModal({ workspaceActions });
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
    const props = renderModal({ workspaceActions });
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
    renderModal({ workspaceActions, defaultFolderId: "jabot-app" });
    await userEvent.click(screen.getByRole("button", { name: /Open folder/ }));
    expect(
      screen.getByRole("group", { name: "Selected workspace" }),
    ).toHaveTextContent("jabot-app");
  });
  it("shows clone failures and keeps the repository available for retry", async () => {
    const workspaceActions = {
      ...actions(),
      pickRepository: vi.fn(async () => {
        throw new Error("Clone failed");
      }),
    };
    renderModal({ workspaceActions });
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
    renderModal({ workspaceActions });
    await userEvent.click(
      screen.getByRole("button", { name: /GitHub repository/ }),
    );
    expect(workspaceActions.signIn).toHaveBeenCalledOnce();
    expect(workspaceActions.listRepositories).not.toHaveBeenCalled();
  });
});
