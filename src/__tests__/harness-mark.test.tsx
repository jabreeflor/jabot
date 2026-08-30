/**
 * The mark on a harness card and on the header chip.
 *
 * It replaced a coloured dot, and the reason it is worth a test is that the
 * dot could not fail: every id drew the same circle. A mark is per-engine, so
 * the thing to hold is that *every* id draws one — including an id this
 * renderer has never heard of, which is every tier-3 harness a user brings
 * (#13) and every harness a newer host adds.
 *
 * It is also decorative. The label is right beside it in both places, and a
 * mark that announced itself would make every card read its name twice.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { HarnessChip } from "../components/HarnessChip";
import { HarnessMark } from "../components/HarnessIcon";
import { HarnessPicker } from "../components/HarnessPicker";
import { HARNESSES } from "../views/mock-host";

function marksIn(container: HTMLElement): SVGElement[] {
  return [...container.querySelectorAll<SVGElement>("svg.hmark")];
}

describe("the harness mark", () => {
  it("draws a distinct one for every harness in the picker", () => {
    const { container } = render(
      <HarnessPicker
        harnesses={HARNESSES}
        value="claude"
        onChange={() => {}}
        label="Harness"
      />,
    );

    const marks = marksIn(container);
    expect(marks).toHaveLength(HARNESSES.length);
    // Distinct: five cards that drew the same glyph would be the dot again
    // with extra steps.
    const shapes = new Set(marks.map((mark) => mark.innerHTML));
    expect(shapes.size).toBe(HARNESSES.length);
    // The card still says which engine it is — the mark is not the label.
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
  });

  it("is decorative, because the label is already there", () => {
    const { container } = render(
      <HarnessChip harnessId="codex" harnesses={HARNESSES} />,
    );
    const [mark] = marksIn(container);
    expect(mark).toBeDefined();
    expect(mark.getAttribute("aria-hidden")).toBe("true");
    expect(screen.getByText("Codex")).toBeInTheDocument();
  });

  it("still draws something for a harness it has never heard of", () => {
    // A tier-3 harness a user brought. There is no mark to know, so it gets
    // the terminal the harness is — never nothing.
    const { container } = render(<HarnessMark harnessId="my-own-agent" />);
    const [mark] = marksIn(container);
    expect(mark).toBeDefined();
    expect(mark.querySelector("path")).not.toBeNull();
    // And it is not a vendor's mark wearing a stranger's name.
    const { container: claude } = render(<HarnessMark harnessId="claude" />);
    expect(mark.innerHTML).not.toBe(marksIn(claude)[0].innerHTML);
  });

  it("takes its colour from the harness accent the card sets", () => {
    const { container } = render(
      <HarnessChip harnessId="pi" harnesses={HARNESSES} />,
    );
    // The chip sets `--dot`; the mark's stylesheet reads it. What is asserted
    // here is the handoff — the accent reaches the element that draws.
    const chip = container.querySelector<HTMLElement>(".harness-chip");
    expect(chip?.style.getPropertyValue("--dot")).toBe("var(--h-pi)");
    expect(marksIn(container)).toHaveLength(1);
  });
});
