import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CritterKit } from "../CritterKit";

const bot = { name: "Mira", color: "b-teal" } as const;

describe("critter kit", () => {
  it("draws the crest before the body, so the body covers the join", () => {
    const { container } = render(
      <CritterKit id="bot.mira" {...bot} state="idle" />,
    );
    const kids = Array.from(container.querySelector(".rig")!.children);
    const bodyGroup = kids.findIndex((el) => el.tagName === "g");
    // Whatever the deal handed this bot, the crest is painted first: a crest
    // drawn after the body would show its base sitting inside the head.
    expect(bodyGroup).toBeGreaterThan(0);
  });

  it("gives different ids different creatures, and one id the same one twice", () => {
    const draw = (id: string) =>
      render(<CritterKit id={id} {...bot} state="idle" />).container.innerHTML;
    expect(draw("bot.mira")).toBe(draw("bot.mira"));
    const crowd = new Set(
      Array.from({ length: 12 }, (_, i) => draw(`bot.${i}`)),
    );
    expect(crowd.size).toBeGreaterThan(6);
  });

  it("lets the state take the face over", () => {
    const failed = render(
      <CritterKit id="bot.mira" {...bot} state="failed" />,
    ).container;
    const idle = render(
      <CritterKit id="bot.mira" {...bot} state="idle" />,
    ).container;
    expect(failed.querySelector(".eyes")!.innerHTML).not.toBe(
      idle.querySelector(".eyes")!.innerHTML,
    );
    // the body underneath is still this bot's own
    expect(failed.querySelector("g.body")!.innerHTML).toBe(
      idle.querySelector("g.body")!.innerHTML,
    );
  });
});
