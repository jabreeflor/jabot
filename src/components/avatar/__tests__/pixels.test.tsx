import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PixelPets } from "../PixelPets";

const bot = { name: "Mira", color: "b-teal" } as const;

/** The twelve ids are drawn in this order, so `dealIndex` hands out 0..11 and
    the sprite at each position is the one the table lists there. */
const CREW = Array.from({ length: 12 }, (_, i) => `bot.${i}`);

const draw = (id: string, state: "idle" | "waiting" | "failed") =>
  render(<PixelPets id={id} {...bot} state={state} />).container;

const cells = (el: HTMLElement) => el.querySelectorAll("rect").length;

describe("pixel pets", () => {
  it("gives every eye of every sprite a lid, in both states", () => {
    // The interesting failure is silent: `grow` finding no free cell leaves
    // the face saying "idle" while the bot is waiting. Both states put exactly
    // one lid per eye, so the two deltas agree, and twenty-two is the crew's
    // eye count — two each except the fish and the whale, drawn in profile.
    let failedLids = 0;
    let waitingLids = 0;
    for (const id of CREW) {
      const idle = cells(draw(id, "idle"));
      const failed = cells(draw(id, "failed")) - idle;
      const waiting = cells(draw(id, "waiting")) - idle;
      expect(failed).toBeGreaterThan(0);
      expect(waiting).toBe(failed);
      failedLids += failed;
      waitingLids += waiting;
    }
    expect(failedLids).toBe(22);
    expect(waitingLids).toBe(22);
  });

  it("keeps a lid off the robot's grille", () => {
    // Fourth in the deal. Waiting prefers the cell below the eye, which here
    // is the row above the grille: taking it would weld eye to grille and turn
    // the face into one continuous well, so the lid goes above instead.
    const robot = draw(CREW[3], "waiting");
    expect(robot.querySelectorAll('rect[x="2"][y="1"]')).toHaveLength(2);
    expect(robot.querySelectorAll('rect[x="2"][y="3"]')).toHaveLength(1);
  });

  it("draws no tile behind the sprite", () => {
    // A sprite on the ground is a creature; a sprite in a box is an app icon.
    const all = Array.from(draw(CREW[0], "idle").querySelectorAll("rect"));
    expect(all.length).toBeGreaterThan(20);
    expect(
      all.every(
        (r) =>
          r.getAttribute("width") === "1" && r.getAttribute("height") === "1",
      ),
    ).toBe(true);
  });

  it("gives one id the same creature twice, and the crew twelve different ones", () => {
    const html = (id: string) => draw(id, "idle").innerHTML;
    expect(html(CREW[0])).toBe(html(CREW[0]));
    expect(new Set(CREW.map(html)).size).toBe(12);
  });
});
