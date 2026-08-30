/**
 * New Chat is where a thread's harness is chosen (#6). The picker has to
 * default to something, report the exact id the host will resolve, and be
 * honest about a harness the machine does not have.
 */
import { render, screen, within } from "@testing-library/react";
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
    expect(
      screen.getByRole("button", { name: /Claude Code/ }),
    ).toHaveAttribute("aria-pressed", "true");
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
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Add retry logic",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith({
      harnessId: "pi",
      folderId: "globnet-sync",
      task: "Add retry logic",
    });
  });

  it("moves the selection when another harness is picked", async () => {
    renderModal();

    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));

    expect(screen.getByRole("button", { name: /Codex/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.getByRole("button", { name: /Claude Code/ }),
    ).toHaveAttribute("aria-pressed", "false");
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

  /** FOLDER is a custom listbox (`Select.tsx`), not a native <select> — the
      browser's own popup will not take the rounded-menu shape the rest of
      the modal uses. */
  it("picks a folder from the custom dropdown", async () => {
    const props = renderModal();

    await userEvent.click(screen.getByLabelText("FOLDER"));
    const menu = screen.getByRole("listbox");
    await userEvent.click(
      within(menu).getByRole("option", { name: "globnet-sync" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ folderId: "globnet-sync" }),
    );
  });

  it("closes just the dropdown on Escape, not the whole card", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });

    await userEvent.click(screen.getByLabelText("FOLDER"));
    expect(screen.getByRole("listbox")).toBeInTheDocument();

    await userEvent.keyboard("{Escape}");

    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.getByLabelText("FOLDER")).toHaveTextContent("jabot-app");
    expect(props.onCancel).not.toHaveBeenCalled();
  });

  it("closes on Escape without starting anything", async () => {
    const props = renderModal();

    await userEvent.keyboard("{Escape}");

    expect(props.onCancel).toHaveBeenCalled();
    expect(props.onStart).not.toHaveBeenCalled();
  });
});

/**
 * The worktree controls (#23).
 *
 * `thread/open` has accepted `useCheckout` and `baseRef` since #23 — the Rust
 * host honours both, and `tests/e2e/worktree.test.ts` drives them — and
 * nothing in the renderer ever set either. They were reachable only by writing
 * JSON-RPC by hand.
 *
 * Advanced, and shut by default, because a fresh worktree per thread is what
 * stops two threads in one repo standing on each other's uncommitted work.
 */
describe("NewChatModal, where the thread will work", () => {
  const advanced = () => screen.getByRole("button", { name: "Advanced" });
  const checkbox = () =>
    screen.getByRole("checkbox", { name: /Work in my current folder/ });

  it("hides the controls until they are asked for", () => {
    renderModal({ defaultFolderId: "jabot-app" });

    expect(advanced()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByLabelText("BASE BRANCH")).toBeNull();
  });

  /** The load-bearing one. The overwhelming majority of sessions send the
      same three fields they always did, and a `useCheckout: false` on the wire
      would be a different request than the one that has been shipping. */
  it("sends nothing extra when nobody opens them", async () => {
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

  it("puts both on the draft when they are set", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });

    await userEvent.click(advanced());
    await userEvent.type(screen.getByLabelText("BASE BRANCH"), " release/2.0 ");
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ baseRef: "release/2.0" }),
    );
  });

  it("sends the opt-out when it is ticked", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });

    await userEvent.click(advanced());
    await userEvent.click(checkbox());
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );

    expect(props.onStart).toHaveBeenCalledWith(
      expect.objectContaining({ useCheckout: true }),
    );
  });

  /**
   * A thread working in the folder's own checkout starts on whatever is
   * checked out there. There is nothing to fork from, so offering a base
   * branch would be offering a setting that does nothing.
   */
  it("takes the base branch away while the opt-out is ticked", async () => {
    const props = renderModal({ defaultFolderId: "jabot-app" });

    await userEvent.click(advanced());
    await userEvent.type(screen.getByLabelText("BASE BRANCH"), "release/2.0");
    await userEvent.click(checkbox());

    expect(screen.getByLabelText("BASE BRANCH")).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "Start session" }),
    );
    // And it does not travel: a base ref sent beside `useCheckout` would ask
    // the host for two different things at once.
    expect(props.onStart).toHaveBeenCalledWith(
      expect.not.objectContaining({ baseRef: expect.anything() }),
    );
  });

  /** "No folder" is a scratch session: no checkout to work in and no branch to
      fork from, so there is nothing for either control to decide. */
  it("offers nothing at all without a folder", async () => {
    renderModal({ defaultFolderId: null });

    expect(screen.queryByRole("button", { name: "Advanced" })).toBeNull();
  });
});
