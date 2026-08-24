/**
 * The chassis and the six drawings that sit in it (#44).
 *
 * The per-style files next door check that a critter has a crest and a sprite
 * has lids. This one checks the things that have to be true of all six at
 * once, because those are what the switch is made of: a style that throws on
 * one of the eight colours, a bot that comes out different on a second render,
 * or a state that quietly draws the idle face are each invisible in a single
 * style's tests and fatal across the set.
 *
 * The crew below is the shape the palette forces. Eight colours and twelve
 * bots means four pairs share a colour, and those four pairs are the only ones
 * a person genuinely has to tell apart — a teal bot and a pink one are already
 * distinct at a glance. Nearly everything here is a claim about those pairs.
 */
import { cleanup, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BOT_COLORS, type BotColor } from "../../types";
import { Avatar, CrewAvatar } from "../Avatar";
import { CREW_STYLES, type AvatarState, type CrewStyle } from "../crew";
import { dealIndex } from "../hash";

const STYLES: readonly CrewStyle[] = CREW_STYLES.map((entry) => entry.id);
const STATES: readonly AvatarState[] = ["idle", "running", "waiting", "failed"];

/**
 * Twelve bots, coloured round-robin, so bots 0 and 8, 1 and 9, 2 and 10, 3 and
 * 11 are the four forced pairs.
 */
const CREW: readonly { id: string; name: string; color: BotColor }[] =
  Array.from({ length: 12 }, (_, i) => ({
    id: `bot.${i}`,
    name: `Bot ${i}`,
    color: BOT_COLORS[i % BOT_COLORS.length],
  }));

/** Every pair of crew members who were handed the same colour. */
const COLOUR_PAIRS: readonly (readonly [number, number])[] = CREW.flatMap(
  (bot, i) =>
    CREW.map((_, j) => [i, j] as const)
      .slice(i + 1)
      .filter(([, j]) => CREW[j].color === bot.color),
);

/**
 * `dealIndex` hands out places in the order the process first draws each bot,
 * so the hats and the sprites a crew wears depend on that order. Fixing it
 * here rather than letting whichever test runs first decide keeps the deal the
 * same whatever order the cases end up in.
 */
CREW.forEach((bot) => dealIndex(bot.id));

/** The drawing alone, without the wrapper's own state attributes. */
function draw(
  style: CrewStyle,
  bot: (typeof CREW)[number],
  state: AvatarState = "idle",
): string {
  const { container } = render(
    <Avatar {...bot} state={state} crewStyle={style} />,
  );
  const svg = container.querySelector("svg");
  if (!svg) throw new Error(`${style} drew no svg`);
  const markup = svg.outerHTML;
  cleanup();
  return markup;
}

/**
 * The drawing pulled apart into the three things a state may and may not
 * touch: the eyes, the rest of the face, and the creature underneath.
 *
 * Comparing whole SVGs turned out to be too coarse for the state cases, and
 * finding that out cost a mutation. With `eyesFor` rigged to fall through to
 * the idle eyes, a waiting bot still came out unlike an idle one — the mouth
 * had changed, and that alone was enough to pass. A face that half-answers is
 * exactly the bug these cases exist for, so the parts are compared apart.
 *
 * The split works by class, which is what the styles paint in: every face mark
 * in the shared vocabulary is `ink` or `inkstroke`, and a body is `body`,
 * `belly` or `litefill`. Classic and Pixel pets paint with literal fills
 * instead and are left out of the cases that use this.
 */
function parts(
  style: CrewStyle,
  bot: (typeof CREW)[number],
  state: AvatarState,
): { eyes: string; face: string; creature: string } {
  const { container } = render(
    <Avatar {...bot} state={state} crewStyle={style} />,
  );
  const svg = container.querySelector("svg");
  if (!svg) throw new Error(`${style} drew no svg`);

  const eyeGroup = svg.querySelector(".eyes");
  const eyes = eyeGroup?.innerHTML ?? "";
  eyeGroup?.remove();

  const ink = [...svg.querySelectorAll(".ink, .inkstroke")];
  const face = ink.map((el) => el.outerHTML).join("");
  ink.forEach((el) => el.remove());

  const creature = svg.innerHTML;
  cleanup();
  return { eyes, face, creature };
}

/** The wrapper, for the chrome tests. */
function avatar(props: Parameters<typeof Avatar>[0]): HTMLElement {
  const { container } = render(<Avatar {...props} />);
  return container.firstElementChild as HTMLElement;
}

describe("every style, every colour", () => {
  it("draws all six styles in all eight colours", () => {
    for (const style of STYLES) {
      for (const color of BOT_COLORS) {
        const el = avatar({
          id: `probe.${color}`,
          name: "Probe",
          color,
          crewStyle: style,
        });
        // The colour is a class and not paint: every fill in every drawing
        // resolves --lite/--deep off this one hook, so a style that forgot it
        // would come out unpainted rather than wrongly painted.
        expect(el).toHaveClass("av", style, color);
        expect(el.querySelector("svg")).not.toBeNull();
        cleanup();
      }
    }
  });

  it("draws every state of every style without a gap in the crew", () => {
    for (const style of STYLES) {
      for (const state of STATES) {
        for (const bot of CREW) {
          expect(draw(style, bot, state)).toContain("<svg");
        }
      }
    }
  });
});

describe("the drawing is a function of the id", () => {
  it("draws the same bot the same way twice", () => {
    for (const style of STYLES) {
      for (const bot of CREW) {
        expect(draw(style, bot)).toBe(draw(style, bot));
      }
    }
  });

  it("gives the four forced colour pairs different drawings", () => {
    // The property the whole module rests on. Classic is the counter-example
    // and gets its own case below.
    for (const style of STYLES.filter((s) => s !== "classic")) {
      for (const [a, b] of COLOUR_PAIRS) {
        expect(
          draw(style, CREW[a]),
          `${style} drew ${CREW[a].id} and ${CREW[b].id} identically`,
        ).not.toBe(draw(style, CREW[b]));
      }
    }
  });

  it("draws the classic blob from its colour alone, which is the complaint", () => {
    // Not a bug to fix here: #44 opens by saying the shipping blob is eight
    // pictures for any number of bots, and the control has to be honest about
    // that or the comparison is rigged. Pinned so nobody quietly improves it
    // and takes the baseline with them.
    expect(COLOUR_PAIRS.length).toBe(4);
    for (const [a, b] of COLOUR_PAIRS) {
      expect(draw("classic", CREW[a])).toBe(draw("classic", CREW[b]));
    }
  });

  it("does not draw a bot as the same creature in two styles", () => {
    // Moodblob and Watchers share a silhouette table, and a shared salt would
    // have made switching between them look like nothing happened.
    for (const bot of CREW) {
      const seen = new Set(STYLES.map((style) => draw(style, bot)));
      expect(seen.size).toBe(STYLES.length);
    }
  });

  it("ignores the name, so renaming a bot leaves its mark alone", () => {
    for (const style of STYLES) {
      const before = draw(style, CREW[0]);
      const after = draw(style, { ...CREW[0], name: "Something Else" });
      expect(after).toBe(before);
    }
  });
});

describe("the dealt marks", () => {
  // Hats, sprites and eye plans are dealt rather than hashed precisely because
  // a hash cannot see that two bots already share a colour. These cases are
  // what that buys.
  const DEALT: readonly CrewStyle[] = ["hats", "pixels", "watchers"];

  it("never hands a colour pair the same mark", () => {
    for (const style of DEALT) {
      for (const [a, b] of COLOUR_PAIRS) {
        expect(draw(style, CREW[a])).not.toBe(draw(style, CREW[b]));
      }
    }
  });

  it("gives a crew of twelve twelve different hats", () => {
    // The table is exactly twelve long, so twelve bots exhaust it without
    // repeating. A thirteenth would wrap, and that is the documented cost.
    const hats = new Set(CREW.map((bot) => draw("hats", bot)));
    expect(hats.size).toBe(12);
  });

  it("gives a crew of twelve twelve different sprites", () => {
    const sprites = new Set(CREW.map((bot) => draw("pixels", bot)));
    expect(sprites.size).toBe(12);
  });

  it("keeps a bot's place in the deal once it has one", () => {
    // The map is the thing that makes a mark stable across a session: a bot
    // redrawn after the whole crew has been through does not take a new hat.
    const first = draw("hats", CREW[0]);
    CREW.forEach((bot) => draw("hats", bot));
    expect(draw("hats", CREW[0])).toBe(first);
  });
});

describe("the state reaches the drawing", () => {
  // Three styles say everything with the face. Two say waiting and failed with
  // the face and running with the ring and the bob, and Classic says nothing
  // at all — that split is the prototype's and is checked rather than assumed.
  const FACED: readonly CrewStyle[] = ["moodblob", "hats", "critters"];
  const IN_MOTION: readonly CrewStyle[] = ["pixels", "watchers"];
  const SAMPLE = [CREW[0], CREW[4], CREW[9]];

  it("draws four different faces for the four states", () => {
    for (const style of FACED) {
      for (const bot of SAMPLE) {
        const faces = STATES.map((state) => draw(style, bot, state));
        expect(new Set(faces).size, `${style} ${bot.id}`).toBe(STATES.length);
      }
    }
  });

  it("changes both the eyes and the mouth, and never only one of them", () => {
    // The shared vocabulary says every state with both, so a style whose eyes
    // stopped answering would still look unlike idle on the strength of its
    // mouth alone. Compared apart, that is a failure rather than a pass.
    for (const style of FACED) {
      for (const bot of SAMPLE) {
        const drawn = STATES.map((state) => parts(style, bot, state));
        expect(
          new Set(drawn.map((d) => d.eyes)).size,
          `${style} ${bot.id} eyes`,
        ).toBe(STATES.length);
        expect(
          new Set(drawn.map((d) => d.face)).size,
          `${style} ${bot.id} mouth`,
        ).toBe(STATES.length);
      }
    }
  });

  it("leaves the creature alone when the face changes", () => {
    // A state must not be able to redraw the bot. The hat, the crest and the
    // silhouette are who it is, and one that turned into a different animal
    // when a run failed would be unreadable at exactly the moment it matters.
    for (const style of [...FACED, "watchers" as const]) {
      for (const bot of SAMPLE) {
        for (const state of STATES) {
          expect(
            parts(style, bot, state).creature,
            `${style} ${bot.id} ${state}`,
          ).toBe(parts(style, bot, "idle").creature);
        }
      }
    }
  });

  it("never lets waiting or failed fall back to the idle face", () => {
    // Both bugs this has caught live here: a state that silently renders as
    // idle, and two states that render as each other. Pixel pets is the one
    // that can fail this quietly — its lids are grown into whichever cell is
    // free, so a sprite with a crowded face can end up with the waiting lid
    // and the failed lid on the same cell.
    for (const style of [...FACED, ...IN_MOTION]) {
      for (const bot of CREW) {
        const idle = draw(style, bot);
        const waiting = draw(style, bot, "waiting");
        const failed = draw(style, bot, "failed");
        expect(waiting, `${style} ${bot.id} waiting`).not.toBe(idle);
        expect(failed, `${style} ${bot.id} failed`).not.toBe(idle);
        expect(failed, `${style} ${bot.id} failed vs waiting`).not.toBe(
          waiting,
        );
      }
    }
  });

  it("says running with the ring, in every style", () => {
    for (const style of STYLES) {
      const el = avatar({ ...CREW[0], state: "running", crewStyle: style });
      expect(el).toHaveAttribute("data-state", "running");
      expect(el.querySelector(".ring")).not.toBeNull();
      cleanup();
    }
  });

  it("gives the sprite and the watcher no running face, by design", () => {
    // Both were ported that way: an eight-by-eight grid has no room for a
    // squint, and a watcher's channel is where it is looking rather than what
    // its face is doing. The ring and base.css's bob carry it. Pinned so the
    // difference stays a decision instead of becoming an oversight.
    for (const style of IN_MOTION) {
      for (const bot of SAMPLE) {
        expect(draw(style, bot, "running")).toBe(draw(style, bot));
      }
    }
  });

  it("gives the classic blob no expression at all", () => {
    for (const state of STATES) {
      expect(draw("classic", CREW[0], state)).toBe(draw("classic", CREW[0]));
    }
  });

  it("puts the state on the wrapper whatever the drawing does with it", () => {
    for (const style of STYLES) {
      for (const state of STATES) {
        const el = avatar({ ...CREW[0], state, crewStyle: style });
        expect(el).toHaveAttribute("data-state", state);
        expect(el.querySelector(".ring") !== null).toBe(state === "running");
        cleanup();
      }
    }
  });
});

describe("the chrome the six share", () => {
  it("draws the unread dot only when there is something unread", () => {
    for (const style of STYLES) {
      render(<Avatar {...CREW[0]} unread crewStyle={style} />);
      expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
      cleanup();

      render(<Avatar {...CREW[0]} crewStyle={style} />);
      expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
      cleanup();
    }
  });

  it("keeps the dot outside the drawing, so no style can lose it", () => {
    const el = avatar({ ...CREW[0], unread: true });
    const dot = screen.getByTestId("unread-dot");
    expect(dot.parentElement).toBe(el);
    expect(dot.closest("svg")).toBeNull();
  });

  it("names the bot with a tooltip and stays out of the accessible name", () => {
    // Every call site already prints the name in text beside the avatar, so an
    // unconditional aria-label makes the sidebar button announce "Bot 0 Bot 0"
    // and breaks getByRole("button", { name }).
    const el = avatar({ ...CREW[0] });
    expect(el).toHaveAttribute("title", "Bot 0");
    expect(el).not.toHaveAttribute("aria-label");
    expect(el.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("takes a name when the avatar is the only thing in its control", () => {
    render(<Avatar {...CREW[0]} labelled />);
    expect(screen.getByRole("img", { name: "Bot 0" })).toBeInTheDocument();
  });

  it("gives each bot its own blink, and reshuffles them per style", () => {
    const offsets = (style: CrewStyle) =>
      CREW.map((bot) => {
        const el = avatar({ ...bot, crewStyle: style });
        const value = el.style.getPropertyValue("--blink");
        cleanup();
        return value;
      });

    // A crew blinking in unison reads as one animation rather than as a room
    // of separate creatures, so the offsets have to spread.
    const hats = offsets("hats");
    expect(new Set(hats).size).toBeGreaterThan(6);

    // Negative, so a bot is born into a cycle already running rather than
    // pausing before its first blink.
    for (const value of hats) expect(value).toMatch(/^-\d(\.\d)?s$/);

    // And salted with the style, so switching crews does not hand every bot
    // back the rhythm it already had.
    expect(offsets("critters")).not.toEqual(hats);
  });

  it("keeps any class the call site asks for", () => {
    const el = avatar({ ...CREW[0], className: "setup-cluster" });
    expect(el).toHaveClass("setup-cluster");
  });
});

describe("the crew's own avatar", () => {
  it("draws three marks in whichever style is on", () => {
    const { container } = render(<CrewAvatar />);
    const cluster = container.firstElementChild as HTMLElement;
    expect(cluster).toHaveClass("cluster", "av-cluster");
    expect(cluster).toHaveAttribute("aria-hidden", "true");

    const slots = cluster.querySelectorAll(":scope > i");
    expect(slots).toHaveLength(3);
    // The slot classes, not :nth-child. A drawing has children of its own, and
    // an index selector eventually matches one of them and rearranges it.
    expect([...slots].map((slot) => slot.className)).toEqual([
      "s1",
      "s2",
      "s3",
    ]);
  });

  it("draws three different colours, so the tile reads as a crew", () => {
    const { container } = render(<CrewAvatar />);
    const colours = [...container.querySelectorAll(".av")].map(
      (el) => [...el.classList].find((c) => c.startsWith("b-")) ?? "",
    );
    expect(new Set(colours).size).toBe(3);
  });
});
