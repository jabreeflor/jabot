/**
 * The transcript's job is to turn a stream of ACP-shaped items into the
 * prototype's grammar. The rule worth pinning is the grouping one: a run of
 * tool calls is one block, and a bubble between them breaks the run.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Transcript } from "../components/Transcript";
import type { ToolStatus, TranscriptItem } from "../components/types";

function tool(
  id: string,
  target: string,
  status: ToolStatus = "completed",
  note?: string,
): TranscriptItem {
  return {
    kind: "tool",
    id,
    call: { id: `${id}-call`, kind: "execute", target, status, note },
  };
}

describe("Transcript", () => {
  it("draws consecutive tool calls as one block", () => {
    const { container } = render(
      <Transcript
        items={[
          tool("a", "npm test"),
          tool("b", "npm run lint"),
          tool("c", "npm run build"),
        ]}
      />,
    );

    const blocks = container.querySelectorAll(".toolblock");
    expect(blocks).toHaveLength(1);
    expect(blocks[0].querySelectorAll(".call")).toHaveLength(3);
  });

  it("starts a new block when the agent speaks in between", () => {
    const { container } = render(
      <Transcript
        items={[
          tool("a", "npm test"),
          { kind: "agent", id: "m1", text: "Tests are green." },
          tool("b", "git push"),
        ]}
      />,
    );

    expect(container.querySelectorAll(".toolblock")).toHaveLength(2);
  });

  it("marks a running call apart from a finished one", () => {
    const { container } = render(
      <Transcript
        items={[
          tool("a", "npm test", "in_progress"),
          tool("b", "npm run lint", "completed", "no findings"),
          tool("c", "cargo build", "failed"),
        ]}
      />,
    );

    expect(container.querySelector(".spin")?.textContent).toContain("running…");
    expect(container.querySelector(".tick")?.textContent).toContain(
      "no findings",
    );
    expect(container.querySelector(".fail")?.textContent).toContain("failed");
  });

  it("puts my words in a cream bubble and the bot's in a graphite one", () => {
    const { container } = render(
      <Transcript
        items={[
          { kind: "user", id: "u1", text: "Fold it." },
          { kind: "agent", id: "a1", text: "Done." },
        ]}
      />,
    );

    expect(container.querySelector(".msg.me")?.textContent).toBe("Fold it.");
    expect(container.querySelector(".msg.bot")?.textContent).toBe("Done.");
  });

  it("reports which notice action was taken and locks the card after", async () => {
    const onAction = vi.fn();
    const notice: TranscriptItem = {
      kind: "notice",
      id: "n1",
      title: "Auth migration",
      pill: "✳ Long-running",
      body: "Est. 40 min.",
      threadId: "auth",
      actions: [
        { id: "fold", label: "Disappear until done", primary: true },
        { id: "watch", label: "Keep watching" },
      ],
    };

    const { rerender } = render(
      <Transcript items={[notice]} onAction={onAction} />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Disappear until done" }),
    );
    expect(onAction).toHaveBeenCalledWith("n1", "fold");

    rerender(
      <Transcript items={[{ ...notice, resolved: true }]} onAction={onAction} />,
    );
    expect(
      screen.getByRole("button", { name: "Keep watching" }),
    ).toBeDisabled();
  });
});
