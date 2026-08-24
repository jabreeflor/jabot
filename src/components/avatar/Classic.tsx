//! The control. Today's blob, redrawn as an SVG so it can stand in the same
//! switch as the five candidates.
//!
//! A comparison is only worth anything if what ships now is in it, and it has
//! to be in it honestly: no blink, no state face, no tuft. The classic blob
//! is a gradient, a per-colour wobble, and two eyes that always look the same
//! whatever the bot is doing — which is exactly the complaint #44 opens with,
//! and the thing the other five are trying to beat.
//!
//! The port is from `src/styles/blob.css` and it is a port rather than a
//! rewrite. The wobble lives there as a `border-radius` with eight
//! percentages, so `wobble()` below does what the browser does with one:
//! four elliptical arcs, and the same overflow correction CSS applies when a
//! pair of adjacent radii adds up to more than the edge they share. Doing it
//! any other way would have produced a blob that is *nearly* the shipping
//! one, which is the worst possible control.

import type { JSX } from "react";
import type { BotColor } from "../types";
import type { CrewRenderProps } from "./crew";

/** Four percentages in CSS order: top-left, top-right, bottom-right, bottom-left. */
type Radii = readonly [number, number, number, number];

const BOX = 24;

function wobble(h: Radii, v: Radii): string {
  // CSS does not clamp radii individually. If any edge's two radii overrun
  // it, every radius on the box shrinks by one shared factor, so the corners
  // keep their proportions. b-yellow is the case that needs it: its right
  // edge asks for 102%.
  const f = Math.min(
    1,
    100 / (h[0] + h[1]),
    100 / (h[3] + h[2]),
    100 / (v[0] + v[3]),
    100 / (v[1] + v[2]),
  );
  const u = (p: number) => Number(((p * f * BOX) / 100).toFixed(2));
  const [x0, x1, x2, x3] = h.map(u);
  const [y0, y1, y2, y3] = v.map(u);

  return [
    `M${x0} 0`,
    `H${Number((BOX - x1).toFixed(2))}`,
    `A${x1} ${y1} 0 0 1 ${BOX} ${y1}`,
    `V${Number((BOX - y2).toFixed(2))}`,
    `A${x2} ${y2} 0 0 1 ${Number((BOX - x2).toFixed(2))} ${BOX}`,
    `H${x3}`,
    `A${x3} ${y3} 0 0 1 0 ${Number((BOX - y3).toFixed(2))}`,
    `V${y0}`,
    `A${x0} ${y0} 0 0 1 ${x0} 0`,
    "Z",
  ].join("");
}

/**
 * The five hand-tuned roundings from blob.css, and which colours wear them.
 * Three colours share the default, which is why the shipping blob reads as
 * six identical dots more often than the stylesheet's comment claims.
 */
const DEFAULT_WOBBLE = wobble([52, 48, 55, 45], [48, 54, 46, 52]);

const WOBBLE: Record<BotColor, string> = {
  "b-teal": DEFAULT_WOBBLE,
  "b-yellow": wobble([48, 52, 45, 55], [54, 46, 56, 44]),
  "b-purple": wobble([55, 45, 50, 50], [45, 55, 48, 52]),
  "b-violet": DEFAULT_WOBBLE,
  "b-blue": wobble([46, 54, 52, 48], [55, 45, 52, 48]),
  "b-orange": DEFAULT_WOBBLE,
  "b-pink": wobble([47, 53, 50, 50], [53, 47, 55, 45]),
  "b-green": wobble([53, 47, 46, 54], [47, 53, 50, 50]),
};

/**
 * `radial-gradient(circle at 32% 28%, lite, deep)` in SVG terms. The radius
 * is CSS's farthest-corner default measured on this box: from (7.68, 6.72)
 * to (24, 24).
 *
 * The id is keyed on the colour and not on the bot, so two teal blobs on one
 * screen produce two identical definitions rather than a collision that
 * matters. Keying it on the bot would multiply the defs by the roster for no
 * gain; keying it on nothing would let the first blob's `var(--lite)` — which
 * resolves against *its* element, not the referrer's — paint the whole crew
 * one colour.
 */
const GRADIENT_R = 23.77;

export function Classic(props: CrewRenderProps): JSX.Element {
  const fillId = `av-classic-${props.color}`;
  return (
    <svg viewBox={`0 0 ${BOX} ${BOX}`} aria-hidden="true">
      <defs>
        <radialGradient
          id={fillId}
          gradientUnits="userSpaceOnUse"
          cx="7.68"
          cy="6.72"
          r={GRADIENT_R}
        >
          <stop offset="0" style={{ stopColor: "var(--lite)" }} />
          <stop offset="1" style={{ stopColor: "var(--deep)" }} />
        </radialGradient>
      </defs>
      <path d={WOBBLE[props.color]} fill={`url(#${fillId})`} />
      {/* Two slivers, tilted towards each other. Not the shared face
          vocabulary on purpose: the shipping blob has no expression, and
          giving it one here would be arguing the candidates' case for them. */}
      <rect
        x="8.16"
        y="8.16"
        width="2.16"
        height="5.76"
        rx="1.05"
        transform="rotate(-8 9.24 11.04)"
        fill="rgba(20, 20, 22, 0.85)"
      />
      <rect
        x="12.48"
        y="8.16"
        width="2.16"
        height="5.76"
        rx="1.05"
        transform="rotate(6 13.56 11.04)"
        fill="rgba(20, 20, 22, 0.85)"
      />
    </svg>
  );
}
