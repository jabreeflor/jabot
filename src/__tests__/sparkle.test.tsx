/**
 * The sparkle is a 3×3 field. Running twinkles; everything else sits still.
 * The word the row used to print is on the control, not in the drawing.
 */
import { cleanup, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Sparkle } from "../components/Sparkle";
import type { StatusTone } from "../components/status";

function spark(tone: StatusTone, seed = "auth") {
  cleanup();
  const { container } = render(
    <Sparkle tone={tone} title={tone} seed={seed} />,
  );
  return container.firstElementChild as HTMLElement;
}

describe("Sparkle", () => {
  it("is a 3×3 field of cells", () => {
    const el = spark("running");
    expect(el.children).toHaveLength(9);
    expect(el).toHaveAttribute("aria-hidden", "true");
  });

  it("twinkles only while the machine is working", () => {
    expect(spark("running")).toHaveClass("live");
    expect(spark("ok")).not.toHaveClass("live");
    expect(spark("bad")).not.toHaveClass("live");
    expect(spark("quiet")).not.toHaveClass("live");
  });

  it("carries the tone the row used to colour a pip with", () => {
    expect(spark("ok")).toHaveAttribute("data-tone", "ok");
    expect(spark("bad")).toHaveAttribute("data-tone", "bad");
    expect(spark("quiet")).toHaveAttribute("data-tone", "quiet");
  });

  it("keeps the status word as a tooltip, not as printed text", () => {
    render(<Sparkle tone="running" title="running" />);
    expect(screen.getByTitle("running")).toBeInTheDocument();
    expect(screen.getByTestId("sparkle")).not.toHaveTextContent("running");
  });

  it("offsets two seeds so live rows do not share a clock", () => {
    const aBase = spark("running", "auth").style.getPropertyValue(
      "--spark-base",
    );
    const bBase = spark("running", "retry").style.getPropertyValue(
      "--spark-base",
    );
    expect(aBase).not.toBe(bBase);
  });
});
