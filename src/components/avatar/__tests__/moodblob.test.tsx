import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Moodblob } from "../Moodblob";

describe("moodblob", () => {
  it("draws a tuft, a body, a belly and a face", () => {
    const { container } = render(
      <Moodblob id="bot.mira" name="Mira" color="b-teal" state="idle" />,
    );
    const svg = container.querySelector("svg")!;
    expect(svg.querySelectorAll(".body").length).toBeGreaterThanOrEqual(2);
    const belly = svg.querySelector(".belly")!;
    expect(belly.getAttribute("transform")).toContain("scale(0.94)");
    expect(belly.getAttribute("d")).toBe(
      Array.from(svg.querySelectorAll("path.body")).pop()!.getAttribute("d"),
    );
    expect(svg.querySelector(".eyes")).not.toBeNull();
    // the same id always draws the same creature
    const again = render(
      <Moodblob id="bot.mira" name="Mira" color="b-teal" state="idle" />,
    );
    expect(again.container.innerHTML).toBe(container.innerHTML);
  });
});
