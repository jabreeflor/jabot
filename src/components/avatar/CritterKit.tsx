//! Candidate 03, Critter kit: body, crest, eyes and mouth all dealt from the
//! id, so every bot is a different little animal and nobody picks anything.
//!
//! This is the candidate that answers "beyond eight" by arithmetic rather than
//! by anybody drawing more: five bodies times six crests times six eyes times
//! three mouths is 540 creatures before colour has been asked for anything, so
//! a roster outgrows the palette long before it outgrows the kit. The cost is
//! the same fact from the other side — generated means no two are wrong in the
//! same way, and it also means none of them is anyone's favourite.
//!
//! Everything hashes here, unlike the hat crew. A critter's identity is the
//! whole silhouette rather than one mark, so two bots colliding on a body
//! still differ by crest, and the page's own greyscale check found exactly one
//! full collision in twelve bots. Dealing would trade that for a mark that
//! moves whenever the roster is edited, which is the worse bargain when the
//! feature is a body rather than a hat.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { pick } from "./hash";
import { eyesFor, mouthFor } from "./face";

/**
 * Five bodies. Order is `pick` order and must not be rearranged: moving an
 * entry hands somebody else's shape to a bot that has had its own since the
 * crew was made.
 *
 * They are shapes rather than species — a round one, an upright one, a
 * capsule, a teardrop and a wide-shouldered one — because the crest is what
 * says cat or bird or bug, and a body that also tried to would fight it. All
 * five sit low in the box, centred somewhere around y = 14, which is what
 * leaves the top third of the square free for a crest to occupy.
 */
const BODIES: readonly JSX.Element[] = [
  // round
  <ellipse cx="12" cy="14" rx="8" ry="7.6" />,
  // upright
  <ellipse cx="12" cy="13.6" rx="6.8" ry="8.4" />,
  // capsule
  <rect x="4.2" y="7.4" width="15.6" height="13.8" rx="6.6" />,
  // teardrop
  <path d="M12 5.6c3.6 3.8 7.6 5.6 7.6 9.2A7.6 7.6 0 0 1 4.4 15c0-3.6 4-5.4 7.6-9.4z" />,
  // wide-shouldered
  <path d="M12 6c4.6 0 7.8 2.6 7.8 7.6S16.4 21.6 12 21.6 4.2 18.6 4.2 13.6 7.4 6 12 6z" />,
];

/**
 * Six crests, same rule about order.
 *
 * Every one of them has its base pushed down to somewhere around y = 10.4,
 * which is well inside whichever body it lands on and looks wrong in
 * isolation. It is the fix for the bug the first draft had. The crest is
 * drawn *before* the body, so the body paints over the overlap and the join
 * disappears; a crest that stopped at the crown of the body it was drawn with
 * hovered clear of the head as soon as the deal paired it with a shorter one.
 * The tallest body crowns at y = 5.2, so a base at 10.4 is buried under every
 * body in the table and not merely under the pairing you happen to be looking
 * at. Shorten one of these and you reintroduce the bug for four bodies out of
 * five.
 *
 * They are `body` paint rather than `litefill`, again unlike the hats: a crest
 * is part of the animal, not an object sitting on it, so it should read as one
 * silhouette with the body rather than separate from it.
 */
const CRESTS: readonly JSX.Element[] = [
  // mohawk
  <path
    className="body"
    d="M8.4 10.8c0-3.4 1.4-4.6 2.6-4.4C11.2 3.6 11.6 2.4 12 2.4s.8 1.2 1 4c1.2-.2 2.6 1 2.6 4.4z"
  />,
  // ears
  <>
    <path className="body" d="M5.4 11.2 7 3.4l4.8 4.6z" />
    <path className="body" d="M18.6 11.2 17 3.4l-4.8 4.6z" />
  </>,
  // antenna. The stem is `--deep` rather than ink, so it belongs to the
  // creature; a dark stem would have read as a hat's wire.
  <>
    <path
      d="M12 10V3.2"
      stroke="var(--deep)"
      strokeWidth="1.6"
      strokeLinecap="round"
    />
    <circle className="body" cx="12" cy="2.4" r="1.7" />
  </>,
  // spike
  <path className="body" d="M12 10.4 9.9 5.6 12 1.8l2.1 3.8z" />,
  // fan
  <path
    className="body"
    d="M7.6 10.6 9.4 3.6l1.4 2.2L12 2.6l1.2 3.2 1.4-2.2 1.8 7z"
  />,
  // side ears
  <>
    <ellipse className="body" cx="4.4" cy="12.4" rx="2.4" ry="3.4" />
    <ellipse className="body" cx="19.6" cy="12.4" rx="2.4" ry="3.4" />
  </>,
];

export function CritterKit(props: CrewRenderProps): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {/* One rig, so the bob and the lean move the creature whole — a crest
          that stayed still while its head breathed would come off. */}
      <g className="rig">
        {pick(CRESTS, props.id, "c")}
        <g className="body">{pick(BODIES, props.id, "b")}</g>
        <ellipse
          className="belly"
          cx="12"
          cy="15.6"
          rx="4.6"
          ry="4"
          opacity="0.55"
        />
        {/* The face vocabulary is drawn on the y = 11.4 line, and a critter's
            head sits lower than a blob's, so the whole face moves down rather
            than every path in face.tsx being renumbered for one style. The
            translate is on a group *around* `.eyes` and not on `.eyes` itself:
            the blink animates `transform`, and an animated `transform` beats a
            `transform` attribute on the same element for the whole cycle, so
            the eyes drew unmoved while the mouth sat 1.6 lower. */}
        <g transform="translate(0 1.6)">
          <g className="eyes">{eyesFor(props)}</g>
          {mouthFor(props)}
        </g>
      </g>
    </svg>
  );
}
