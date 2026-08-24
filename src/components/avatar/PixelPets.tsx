//! Candidate 04, Pixel pets: eight by eight, hand-drawn, two paints and an ink.
//!
//! Every other style here is a vector drawing that has to survive being shrunk
//! to 28px. A sprite is the one format that was designed for that size in the
//! first place — at 28px each cell is three and a half real pixels, which is
//! roughly what an 8x8 sprite always was — so this candidate answers the size
//! rung by not having a size problem. The cost is real and worth saying out
//! loud: someone has to be the pixel artist, forever, and a thirteenth bot
//! means drawing a thirteenth creature by hand.
//!
//! Two decisions in here look like limitations and are the design. There is no
//! tile behind the sprite: a sprite on the ground is a creature, a sprite in a
//! box is an app icon, and #44 asked for a crew rather than a launcher. And the
//! state faces are not drawn from `face.tsx` like the other styles' — an eight
//! by eight grid has no room for an X-ed out eye, so shut is a lid *beside* the
//! eye and wide awake is a second cell under it, which are the conventions the
//! format itself settled decades ago.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { dealIndex } from "./hash";

/**
 * Twelve creatures, drawn by hand.
 *
 * Legend: `X` body, `D` shade, `o` ink, `e` an eye, `.` nothing. An eye gets
 * its own letter rather than being ink like everything else because the state
 * faces have to *find* the eyes, and an antenna is ink too — a y-coordinate
 * heuristic that assumed "the ink near the top is the face" put a shut lid on
 * the moth's antennae.
 *
 * Order matters. The sprite is dealt by position, so moving a row here
 * silently gives half the crew a different animal.
 */
const SPRITES: readonly (readonly string[])[] = [
  // cat: ears out at the corners
  [
    "XX....XX",
    "XXX..XXX",
    ".XXXXXX.",
    "XeXXXXeX",
    "XXXXXXXX",
    "XXXooXXX",
    "DXXXXXXD",
    ".XX..XX.",
  ],
  // bird: crest, and a beak that leaves the body
  [
    "...X....",
    "..XXX...",
    ".XXXXX..",
    "XeXXXeX.",
    "XXXXXXDD",
    ".XXXXXX.",
    ".DXXXXD.",
    "..X..X..",
  ],
  // moth: two antennae in body shade so they stay attached
  [
    "D......D",
    ".D....D.",
    "..XXXX..",
    ".XeXXeX.",
    "XXXXXXXX",
    "XXXXXXXX",
    "DXX..XXD",
    ".X....X.",
  ],
  // robot: flat head, one aerial, a grille
  [
    "..D..D..",
    "XXXXXXXX",
    "XXeXXeXX",
    "XXXXXXXX",
    "XXooooXX",
    "XXXXXXXX",
    ".XXXXXX.",
    ".X....X.",
  ],
  // star
  [
    "...XX...",
    "..XXXX..",
    "XXXXXXXX",
    ".XeXXeX.",
    ".XXXXXX.",
    "XXXXXXXX",
    "XX.XX.XX",
    "X......X",
  ],
  // flame
  [
    "....X...",
    "...XX...",
    "..XXXX..",
    ".XeXXeX.",
    "XXXXXXXX",
    "XXXooXXX",
    "DXXXXXXD",
    ".XXXXXX.",
  ],
  // ghost: no feet, a wavy hem
  [
    "..XXXX..",
    ".XXXXXX.",
    "XXXXXXXX",
    "XeXXXXeX",
    "XXXXXXXX",
    "XXXooXXX",
    "XXXXXXXX",
    "X.XX.XX.",
  ],
  // mushroom: wide cap, narrow stem
  [
    "..XXXX..",
    ".XXXXXX.",
    "XXXXXXXX",
    "XXeXXeXX",
    ".DXXXXD.",
    "..XXXX..",
    "..XooX..",
    "..XXXX..",
  ],
  // fish: one eye, tail off the right edge
  [
    "..XXX...",
    ".XXXXX.X",
    "XeXXXXXX",
    "XXXXXXXX",
    "XXXXXXXX",
    ".XXXXX.X",
    "..XXX...",
    "........",
  ],
  // crab: claws high and wide, legs below
  [
    "X......X",
    "XX....XX",
    ".XXXXXX.",
    ".XeXXeX.",
    "XXXXXXXX",
    "XXXooXXX",
    "XX.XX.XX",
    "X.X..X.X",
  ],
  // owl: tufts and two big eyes
  [
    "X......X",
    "XXXXXXXX",
    "XeXXXXeX",
    "XXXXXXXX",
    "XXXooXXX",
    "XXXXXXXX",
    "DXXXXXXD",
    ".XX..XX.",
  ],
  // whale: spout and a fluke
  [
    "..D.....",
    "..D....D",
    ".XXXXXXD",
    "XeXXXXXD",
    "XXXXXXXX",
    ".XXXXXX.",
    "..XXXX..",
    "........",
  ],
];

/**
 * Three paints and no near-black.
 *
 * Ink is the bot's own darkest tone rather than `--on-color`, which is what
 * every other style uses for a mark. That is a fix, not an oversight: ink at
 * #14141a on the #151516 chat background dissolved into the page and took the
 * eye with it, because eleven of the twelve sprites put a shut lid on an edge
 * cell where there is no body behind it.
 */
const PAINT: Record<string, string> = {
  X: "var(--lite)",
  D: "var(--deep)",
  o: "var(--deeper)",
  e: "var(--deeper)",
};

type Cell = readonly [number, number];

function put(x: number, y: number, paint: string, key: string): JSX.Element {
  return <rect key={key} x={x} y={y} width="1" height="1" fill={paint} />;
}

export function PixelPets({ id, state }: CrewRenderProps): JSX.Element {
  // Dealt rather than hashed, for the reason `dealt` gives: the creature *is*
  // the identity here, and the two bots forced onto one colour by an
  // eight-colour palette are exactly the pair that must not also be the same
  // animal.
  const rows = SPRITES[dealIndex(id) % SPRITES.length];
  const at = (x: number, y: number): string => (rows[y] || "")[x] || ".";

  // Where the eyes are is a property of the sprite, not a constant: read them
  // back out of the drawing so a state face lands on the right cells whatever
  // creature this is.
  const eyes: Cell[] = [];
  rows.forEach((row, y) => {
    [...row].forEach((ch, x) => {
      if (ch === "e") eyes.push([x, y]);
    });
  });

  const cells: JSX.Element[] = [];
  rows.forEach((row, y) => {
    [...row].forEach((ch, x) => {
      if (ch !== ".") cells.push(put(x, y, PAINT[ch], `c${x}.${y}`));
    });
  });

  const solid = (x: number, y: number): boolean => "XDeo".includes(at(x, y));
  const isInk = (x: number, y: number): boolean => "oe".includes(at(x, y));

  // A lid lands on body, and touches no ink but the eye it belongs to. Without
  // the second half it welds the eye to the nearest mark — on the robot, to its
  // own grille, and the face becomes one continuous well with no eyes in it.
  const free = (x: number, y: number, ex: number, ey: number): boolean => {
    if (!solid(x, y) || isInk(x, y)) return false;
    const around: Cell[] = [
      [x - 1, y],
      [x + 1, y],
      [x, y - 1],
      [x, y + 1],
    ];
    return around.every(
      ([nx, ny]) => (nx === ex && ny === ey) || !isInk(nx, ny),
    );
  };

  // Refusing to draw is worse than drawing in second place: a state that
  // silently declines leaves the face saying "idle" while the bot is waiting.
  // Each state has an order of preference and takes the first free cell.
  const grow = (x: number, y: number, order: Cell[]): JSX.Element | null => {
    for (const [dx, dy] of order)
      if (free(x + dx, y + dy, x, y))
        return put(x + dx, y + dy, PAINT.o, `l${x}.${y}`);
    return null;
  };

  const face: JSX.Element[] = [];
  if (state === "failed")
    eyes.forEach(([x, y]) => {
      // Outward first: two lids growing towards each other meet in the middle
      // and censor the face.
      const out = x < 4 ? -1 : 1;
      const lid = grow(x, y, [
        [out, 0],
        [0, 1],
        [-out, 0],
      ]);
      if (lid) face.push(lid);
    });
  if (state === "waiting")
    eyes.forEach(([x, y]) => {
      // A taller eye is how a sprite says it is looking at you.
      const lid = grow(x, y, [
        [0, 1],
        [0, -1],
        [x < 4 ? -1 : 1, 0],
      ]);
      if (lid) face.push(lid);
    });

  // The half-unit margin is what lets a cell on the outer ring keep its edge
  // instead of being clipped by the box it is drawn in.
  return (
    <svg viewBox="-0.5 -0.5 9 9" aria-hidden="true">
      <g className="rig">
        {cells}
        {face}
      </g>
    </svg>
  );
}
