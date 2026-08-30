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
      pill: "Long-running",
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

/**
 * Markdown inside an agent bubble (#14).
 *
 * An agent replies in markdown because that is what agents do, and the bubble
 * rendered it as a text node — so a fenced diff arrived as a wall of backticks
 * and a bulleted plan arrived as a paragraph starting with a hyphen.
 *
 * The user bubble stays literal throughout. A person's asterisks are their
 * own, and a renderer that ate them would be editing what somebody said.
 */
describe("markdown in an agent's reply", () => {
  function bot(text: string) {
    const { container } = render(
      <Transcript items={[{ kind: "agent", id: "a1", text }]} />,
    );
    return container.querySelector(".msg.bot") as HTMLElement;
  }

  function me(text: string) {
    const { container } = render(
      <Transcript items={[{ kind: "user", id: "u1", text }]} />,
    );
    return container.querySelector(".msg.me") as HTMLElement;
  }

  it("draws a fence as a code block, without its backticks", () => {
    const bubble = bot("Here:\n```ts\nconst x = 1;\n```\nDone.");

    const code = bubble.querySelector("pre code");
    expect(code?.textContent).toBe("const x = 1;");
    expect(bubble.textContent).not.toContain("```");
    // The prose either side survives as prose.
    expect(bubble.textContent).toContain("Here:");
    expect(bubble.textContent).toContain("Done.");
  });

  /**
   * The streaming case, and the reason it is not an edge case. A fence being
   * typed is unterminated on every chunk until the last one, so falling back
   * to literal text would make the block flicker in and out on every token.
   */
  it("grows a code block from a fence that is still being typed", () => {
    const bubble = bot("```sh\nnpm ci");

    expect(bubble.querySelector("pre code")?.textContent).toBe("npm ci");
  });

  it("draws a bulleted run as a list", () => {
    const bubble = bot("Plan:\n- read the file\n- fix the bug\n- run tests");

    const items = [...bubble.querySelectorAll("ul li")].map((li) => li.textContent);
    expect(items).toEqual(["read the file", "fix the bug", "run tests"]);
  });

  it("draws a numbered run as an ordered list", () => {
    const bubble = bot("1. first\n2. second");

    expect(bubble.querySelectorAll("ol li")).toHaveLength(2);
    // The numbers are the list's own, not text: "1." must not appear twice.
    expect(bubble.textContent).not.toContain("1.");
  });

  it("draws emphasis and inline code", () => {
    const bubble = bot("**four** tests, one *slow*, in `auth.test.ts`");

    expect(bubble.querySelector("strong")?.textContent).toBe("four");
    expect(bubble.querySelector("em")?.textContent).toBe("slow");
    expect(bubble.querySelector("code")?.textContent).toBe("auth.test.ts");
  });

  /**
   * The load-bearing negative. An agent writing about multiplication, a lone
   * underscore in a filename, or a hyphen mid-sentence has to come out as
   * typed — this is prose, not a document, and most of it is not markup.
   */
  it("leaves an unmatched delimiter as the character it is", () => {
    expect(bot("2 * 3 * 4 is 24").textContent).toBe("2 * 3 * 4 is 24");
    expect(bot("see run_once_at for the field").textContent).toBe(
      "see run_once_at for the field",
    );
    expect(bot("**").textContent).toBe("**");
  });

  /** Inside a backtick span nothing is markup: `**` there is two asterisks. */
  it("does not read markup inside code", () => {
    const bubble = bot("use `**kwargs` for that");

    expect(bubble.querySelector("code")?.textContent).toBe("**kwargs");
    expect(bubble.querySelector("strong")).toBeNull();
  });

  it("leaves what a person typed exactly as they typed it", () => {
    expect(me("**bold**").textContent).toBe("**bold**");
    expect(me("- a\n- b").textContent).toBe("- a\n- b");
    expect(me("```\ncode\n```").textContent).toBe("```\ncode\n```");
  });

  /**
   * The streaming budget (#14). Appending a chunk replaces one item and leaves
   * every sibling the same object, and `memo(TranscriptRow)` keys on that — so
   * a streamed token must reparse one bubble and leave the neighbouring
   * toolblock's DOM node alone, literally the same element.
   */
  it("reparses one bubble and leaves its neighbours untouched", () => {
    const call = {
      id: "c1",
      kind: "read" as const,
      target: "src/auth.ts",
      status: "completed" as const,
    };
    const tool: TranscriptItem = { kind: "tool", id: "t1", call };
    const { container, rerender } = render(
      <Transcript items={[tool, { kind: "agent", id: "a1", text: "```sh\nnpm" }]} />,
    );
    const before = container.querySelector(".toolblock");

    rerender(
      <Transcript
        items={[tool, { kind: "agent", id: "a1", text: "```sh\nnpm ci" }]}
      />,
    );

    expect(container.querySelector(".toolblock")).toBe(before);
    expect(container.querySelector("pre code")?.textContent).toBe("npm ci");
  });
});

/**
 * Windowing (#14).
 *
 * The deferred half of end-anchored virtualization: render from the tail
 * rather than the whole history. The record deferred it because nothing had
 * produced a transcript long enough to jank, and that is still true — so the
 * bar this has to clear is that it costs nothing, and in particular that it
 * does not defeat the two memoization properties `Transcript`'s own comments
 * protect.
 */
describe("a transcript long enough to window", () => {
  const many = (count: number): TranscriptItem[] =>
    Array.from({ length: count }, (_, i) => ({
      kind: "agent" as const,
      id: `a${i}`,
      text: `line ${i}`,
    }));

  it("draws the end of a long conversation, not all of it", () => {
    const items = many(500);
    const { container } = render(<Transcript items={items} />);

    expect(container.querySelectorAll(".msg")).toHaveLength(80);
    // The end is what a conversation opens at.
    expect(container.textContent).toContain("line 499");
    expect(container.textContent).not.toContain("line 0");
  });

  it("leaves an ordinary conversation whole, with nothing extra on screen", () => {
    const { container } = render(<Transcript items={many(12)} />);

    expect(container.querySelectorAll(".msg")).toHaveLength(12);
    expect(container.querySelector(".show-earlier")).toBeNull();
  });

  it("says how much is above, and grows the window when asked", async () => {
    const { container } = render(<Transcript items={many(500)} />);

    const earlier = screen.getByRole("button", { name: /Show earlier/ });
    expect(earlier).toHaveTextContent("420 more");

    await userEvent.click(earlier);

    expect(container.querySelectorAll(".msg")).toHaveLength(160);
    expect(container.textContent).toContain("line 340");
  });

  /**
   * The property windowing must not cost. #14's reducer returns a new array
   * whose other elements are the same objects, and `sameCalls` compares by
   * pointer — so appending a chunk has to leave every other row's DOM node
   * literally the same element. Slicing `items` before grouping would break
   * this; slicing the grouped array does not.
   */
  it("leaves untouched rows as the same DOM nodes across a chunk", () => {
    const call = {
      id: "c1",
      kind: "read" as const,
      target: "src/auth.ts",
      status: "completed" as const,
    };
    const prefix: TranscriptItem[] = [
      ...many(100),
      { kind: "tool", id: "t1", call },
    ];
    const streaming = { kind: "agent" as const, id: "a-live", text: "typ" };
    const { container, rerender } = render(
      <Transcript items={[...prefix, streaming]} />,
    );
    const toolBefore = container.querySelector(".toolblock");
    const neighbour = container.querySelector(".msg.bot");

    // Exactly what `replaceAt` produces: a new array, the same objects.
    rerender(
      <Transcript items={[...prefix, { ...streaming, text: "typing" }]} />,
    );

    expect(container.querySelector(".toolblock")).toBe(toolBefore);
    expect(container.querySelector(".msg.bot")).toBe(neighbour);
    expect(container.textContent).toContain("typing");
  });
});
