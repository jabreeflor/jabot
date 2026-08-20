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

  it("closes on Escape without starting anything", async () => {
    const props = renderModal();

    await userEvent.keyboard("{Escape}");

    expect(props.onCancel).toHaveBeenCalled();
    expect(props.onStart).not.toHaveBeenCalled();
  });
});
