/**
 * The deal: which bot wears which mark, and who is allowed to take one (#44).
 *
 * Three styles deal a mark from a list rather than hashing one, because a hash
 * cannot see that two bots already share a colour. That buys a crew where
 * every mark is different, and it costs a shared counter — which is exactly
 * the kind of thing that works in a test and then quietly does not work in an
 * app. Both ways it broke are pinned here: an avatar that is not a bot taking
 * a place off the top of the deck, and paint order rather than roster order
 * deciding who gets what.
 *
 * Its own file because the counter is per-process: a case sharing a file with
 * the drawing tests would be dealing from wherever those had left it.
 */
import { cleanup, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Avatar, CrewAvatar } from "../Avatar";
import { dealIndex, reserveDeal, seedDealOrder } from "../hash";

/** The hat a bot is wearing, as markup — the deal is not visible any other way. */
function hat(id: string): string {
  const { container } = render(
    <Avatar id={id} name="Bot" color="b-teal" crewStyle="hats" />,
  );
  const markup = container.querySelector("svg")!.outerHTML;
  cleanup();
  return markup;
}

describe("the deal", () => {
  it("hands out places in the order the roster was seeded, not the order it painted", () => {
    // The shell seeds above everything that draws a bot, so a pane that
    // happens to paint first cannot renumber the crew. Chief is drawn here
    // before the roster arrives and still ends up where the roster puts it.
    dealIndex("crew.chief");
    seedDealOrder(["crew.chief", "crew.code", "crew.writer"]);

    expect(dealIndex("crew.chief")).toBe(0);
    expect(dealIndex("crew.code")).toBe(1);
    expect(dealIndex("crew.writer")).toBe(2);
  });

  it("keeps a place once a bot has one, however often the roster is seeded", () => {
    // A mark that moved under a person mid-session would read as a different
    // bot arriving, so seeding only ever adds.
    seedDealOrder(["crew.writer", "crew.chief"]);
    expect(dealIndex("crew.chief")).toBe(0);
    expect(dealIndex("crew.writer")).toBe(2);
  });

  it("does not spend a place on an avatar that is not a bot", () => {
    // The bug this exists for: the crew cluster and the editor's eight colour
    // swatches were taking eleven places between them, so the first real bot
    // drawn after the editor had been opened once was dealt a hat another bot
    // was already wearing.
    const before = dealIndex("crew.probe");

    reserveDeal("swatch.b-pink", 6);
    render(<CrewAvatar />);
    cleanup();

    expect(dealIndex("crew.after")).toBe(before + 1);
  });

  it("gives twelve bots twelve hats with a cluster on the page", () => {
    render(<CrewAvatar />);
    cleanup();
    const crowd = Array.from({ length: 12 }, (_, i) => `crowd.${i}`);
    expect(new Set(crowd.map(hat)).size).toBe(12);
  });

  it("draws the cluster from the first, third and fourth marks", () => {
    // The prototype's cluster was BOTS[0], BOTS[2], BOTS[3], and the tile is
    // where a person reads what the crew *is*, so it wears the same three.
    expect(dealIndex("crew.a")).toBe(0);
    expect(dealIndex("crew.b")).toBe(2);
    expect(dealIndex("crew.c")).toBe(3);
  });
});
