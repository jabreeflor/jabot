/**
 * The crew view. Every card has to show what the bot is *made of* — its tools
 * and, after #6, its harness — and Chief has to be un-removable, because the
 * store allows exactly one and the product assumes it exists.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CrewView } from "../views/CrewView";
import {
  HARNESSES,
  HOST_TOOLS,
  TOOL_CATALOG,
  initialMockState,
} from "../views/mock-host";

function renderCrew(over: Partial<Parameters<typeof CrewView>[0]> = {}) {
  const props = {
    bots: initialMockState().bots,
    harnesses: HARNESSES,
    tools: [...TOOL_CATALOG, ...HOST_TOOLS],
    onEdit: vi.fn(),
    onAdd: vi.fn(),
    onRemove: vi.fn(),
    ...over,
  };
  render(<CrewView {...props} />);
  return props;
}

function card(name: string): HTMLElement {
  const heading = screen.getByText(name);
  const element = heading.closest(".crew-card");
  if (!element) throw new Error(`no crew card for ${name}`);
  return element as HTMLElement;
}

describe("CrewView", () => {
  it("shows each bot's configured tools and harness", () => {
    renderCrew();

    const recruiter = card("Bot Recruiter");
    expect(within(recruiter).getByText("Claude Code")).toBeInTheDocument();
    expect(within(card("Chief")).getByText("Handoff")).toBeInTheDocument();
  });

  it("labels Chief and gives it no Remove", () => {
    renderCrew();

    const chief = card("Chief");
    expect(within(chief).getByText("CHIEF")).toBeInTheDocument();
    expect(
      within(chief).queryByRole("button", { name: "Remove" }),
    ).not.toBeInTheDocument();
    expect(within(chief).getByRole("button", { name: "Edit" })).toBeEnabled();
  });

  it("names Chief's host tools instead of printing their ids", () => {
    renderCrew();

    expect(within(card("Chief")).getByText("Handoff")).toBeInTheDocument();
  });

  it("edits, removes, and adds by id", async () => {
    const props = renderCrew();

    await userEvent.click(
      within(card("Bot Recruiter")).getByRole("button", { name: "Edit" }),
    );
    expect(props.onEdit).toHaveBeenCalledWith("bot-recruiter");

    await userEvent.click(
      within(card("Bot Recruiter")).getByRole("button", { name: "Remove" }),
    );
    expect(props.onRemove).toHaveBeenCalledWith("bot-recruiter");

    await userEvent.click(screen.getByRole("button", { name: /Add a bot/ }));
    expect(props.onAdd).toHaveBeenCalled();
  });
});
