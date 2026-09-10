import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { AgentPill } from "../components/AgentPill";
import { Composer } from "../components/Composer";
import { Transcript } from "../components/Transcript";
import type { Bot } from "../components/types";

const SCOUT: Bot = {
  id: "scout",
  name: "Scout",
  color: "b-yellow",
  instructions: "Check sources.",
  tools: [],
  harnessId: "claude",
  isChief: false,
};

const CHIEF: Bot = {
  id: "chief",
  name: "Chief",
  color: "b-teal",
  instructions: "Route work.",
  tools: [],
  harnessId: "claude",
  isChief: true,
};

describe("AgentPill", () => {
  it("draws the monochrome BotMark and the name, not a colour circle", () => {
    const { container } = render(<AgentPill bot={SCOUT} variant="title" />);

    expect(screen.getByRole("heading", { name: "Scout" })).toBeInTheDocument();
    expect(container.querySelector(".bot-mark")).toHaveAttribute(
      "data-character",
      "scout",
    );
    expect(container.querySelector(".av")).toHaveClass("b-yellow");
  });

  it("names an interactive presence pill without colliding with the sidebar row", async () => {
    const onClick = vi.fn();
    render(<AgentPill bot={SCOUT} variant="presence" onClick={onClick} />);

    await userEvent.click(
      screen.getByRole("button", { name: "Open chat with Scout" }),
    );
    expect(onClick).toHaveBeenCalled();
  });
});

describe("mention pills in chat", () => {
  it("turns @Name in a user bubble into a pill", async () => {
    const onSelectBot = vi.fn();
    render(
      <Transcript
        items={[{ kind: "user", id: "u1", text: "Hand this to @Scout." }]}
        bots={[SCOUT, CHIEF]}
        onSelectBot={onSelectBot}
      />,
    );

    await userEvent.click(
      screen.getByRole("button", { name: "Open chat with Scout" }),
    );
    expect(onSelectBot).toHaveBeenCalledWith("scout");
  });

  it("offers crew pills from the composer when @ is typed", async () => {
    const onSend = vi.fn();
    render(
      <Composer
        placeholder="Message Chief"
        onSend={onSend}
        mentionBots={[CHIEF, SCOUT]}
      />,
    );

    await userEvent.type(screen.getByLabelText("Message Chief"), "ask @Sc");
    await userEvent.click(
      screen.getByRole("button", { name: "Mention Scout" }),
    );
    expect(screen.getByLabelText("Message Chief")).toHaveValue("ask @Scout ");
  });
});
