/**
 * The crew editor. The prototype had name, colour, instructions, and tool
 * chips; decision #6 added the harness, so the thing worth proving is that a
 * bot's engine is editable and travels with a template.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BotEditorModal } from "../components/BotEditorModal";
import type { Bot } from "../components/types";
import { BOT_TEMPLATES, HARNESSES, TOOL_CATALOG } from "../views/mock-host";

const WRITER: Bot = {
  id: "writer",
  name: "Writer",
  color: "b-orange",
  instructions: "Draft in my voice: plain, short, no filler.",
  tools: ["gmail", "notion"],
  harnessId: "claude",
  isChief: false,
};

const CHIEF: Bot = {
  id: "chief",
  name: "Chief",
  color: "b-teal",
  instructions: "Route work across the crew.",
  tools: ["handoff_to_bot"],
  harnessId: "claude",
  isChief: true,
};

function renderEditor(over: Partial<Parameters<typeof BotEditorModal>[0]> = {}) {
  const props = {
    bot: null,
    templates: BOT_TEMPLATES,
    tools: TOOL_CATALOG,
    harnesses: HARNESSES,
    onSave: vi.fn(),
    onRemove: vi.fn(),
    onCancel: vi.fn(),
    ...over,
  };
  render(<BotEditorModal {...props} />);
  return props;
}

describe("BotEditorModal", () => {
  it("opens an existing bot with its own fields, tools, and harness", () => {
    renderEditor({ bot: WRITER });

    expect(screen.getByLabelText("NAME")).toHaveValue("Writer");
    expect(screen.getByRole("button", { name: "Gmail" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Slack" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(
      screen.getByRole("button", { name: /Claude Code/ }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("saves the edits, including a changed harness", async () => {
    const props = renderEditor({ bot: WRITER });

    await userEvent.clear(screen.getByLabelText("NAME"));
    await userEvent.type(screen.getByLabelText("NAME"), "Ghostwriter");
    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
    await userEvent.click(screen.getByRole("button", { name: "Notion" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Ghostwriter",
        harnessId: "codex",
        tools: ["gmail"],
      }),
    );
  });

  it("fills the whole form from a template, harness included", async () => {
    const props = renderEditor();

    await userEvent.selectOptions(
      screen.getByLabelText("START FROM A TEMPLATE"),
      "ops",
    );

    expect(screen.getByLabelText("NAME")).toHaveValue("Ops / On-call");
    expect(screen.getByRole("button", { name: /Codex/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Ops / On-call",
        color: "b-orange",
        harnessId: "codex",
        tools: ["terminal", "slack"],
        templateId: "ops",
      }),
    );
  });

  it("does not offer a template when editing an existing bot", () => {
    renderEditor({ bot: WRITER });

    expect(
      screen.queryByLabelText("START FROM A TEMPLATE"),
    ).not.toBeInTheDocument();
  });

  it("refuses to remove Chief", () => {
    renderEditor({ bot: CHIEF });

    expect(
      screen.queryByRole("button", { name: "Remove" }),
    ).not.toBeInTheDocument();
  });

  it("removes a worker bot", async () => {
    const props = renderEditor({ bot: WRITER });

    await userEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(props.onRemove).toHaveBeenCalledWith("writer");
  });

  it("gives an unnamed new bot a name rather than an empty one", async () => {
    const props = renderEditor();

    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ name: "Unnamed bot" }),
    );
  });
});
