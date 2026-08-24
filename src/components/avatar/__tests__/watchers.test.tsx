import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Watchers } from "../Watchers";

describe("watchers", () => {
  it("draws a sclera and a pupil for every eye in the plan", () => {
    const { container } = render(
      <Watchers id="bot.mira" name="Mira" color="b-teal" state="idle" />,
    );
    const svg = container.querySelector("svg")!;
    const eyes = svg.querySelectorAll(".sclera");
    expect(eyes.length).toBeGreaterThanOrEqual(1);
    expect(svg.querySelectorAll(".pupil").length).toBe(eyes.length);
    // The pupil is concentric with its eye until the gaze moves it, and the
    // gaze is a CSS transform rather than an attribute.
    expect(svg.querySelector(".pupil")!.getAttribute("cx")).toBe(
      eyes[0].getAttribute("cx"),
    );
  });

  it("gives two bots on one colour different eyes", () => {
    const a = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    const b = render(
      <Watchers id="bot.two" name="Two" color="b-teal" state="idle" />,
    );
    expect(a.container.innerHTML).not.toBe(b.container.innerHTML);
  });

  it("shuts its eyes and turns its mouth down when it has failed", () => {
    const { container } = render(
      <Watchers id="bot.mira" name="Mira" color="b-teal" state="failed" />,
    );
    expect(container.querySelector(".sclera")).toBeNull();
    // Shut arcs alone read as asleep; the mouth is what makes it a wince.
    expect(container.querySelector('path[d="M9.8 18.4q2.2-2 4.4 0"]')).not.toBeNull();
  });

  it("raises one brow per eye when it needs you", () => {
    const { container } = render(
      <Watchers id="bot.mira" name="Mira" color="b-teal" state="waiting" />,
    );
    const eyes = container.querySelectorAll(".sclera").length;
    const strokes = container.querySelectorAll("path.inkstroke").length;
    expect(strokes).toBe(eyes);
  });

  it("installs the page's listeners once and removes them on the last unmount", () => {
    const add = vi.spyOn(window, "addEventListener");
    const remove = vi.spyOn(window, "removeEventListener");

    const one = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    const two = render(
      <Watchers id="bot.two" name="Two" color="b-pink" state="idle" />,
    );
    const pointerAdds = add.mock.calls.filter(
      ([type]) => type === "pointermove",
    );
    expect(pointerAdds).toHaveLength(1);

    one.unmount();
    expect(
      remove.mock.calls.filter(([type]) => type === "pointermove"),
    ).toHaveLength(0);

    two.unmount();
    expect(
      remove.mock.calls.filter(([type]) => type === "pointermove"),
    ).toHaveLength(1);
    expect(document.documentElement.style.getPropertyValue("--gx")).toBe("");

    add.mockRestore();
    remove.mockRestore();
  });
});
