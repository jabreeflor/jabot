//! Candidate 02, Hat crew: one body, and the thing on its head is who it is.
//!
//! Every other style tries to make the *body* the identity, which is the hard
//! way round: at 28px and in greyscale a body is a blob-sized smudge and the
//! differences between two of them are gone. A hat is a bump on the outside of
//! the silhouette, and the outline is the one channel that survives both. So
//! the body here is deliberately the same circle for everybody — twelve hats
//! do all the work, and adding a thirteenth is adding one path.
//!
//! Both judges picked this one, so it is ported as drawn rather than as it
//! might be improved. Three details in particular look like mistakes and are
//! not; each is a review round, and each has its own note below.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { dealIndex } from "./hash";
import { eyesFor, mouthFor } from "./face";

/**
 * The twelve. Order is the deal order and must not be rearranged: `dealIndex`
 * counts into this array, so moving an entry hands somebody else's hat to a
 * bot that has been wearing its own since the crew was made.
 *
 * Two rules run through the set. Anything meant to read as an object sitting
 * *on* the head is `litefill`, which is the lit paint with an ink outline, so
 * it separates from the body underneath it. And anything thin — a stem, a
 * band, the halo — is stroked twice, dark and wide underneath, coloured and
 * narrow on top. A single coloured stroke is the thing that broke on the light
 * theme: at 1.6 units of `--lite` on white it went to nearly nothing while
 * every filled neighbour still carried its dark edge, so the antenna crew
 * looked bald and the others did not.
 */
const HATS: readonly JSX.Element[] = [
  // ears. Two triangles rooted on the skull with a gap between them, not two
  // meeting at a point: the apex-to-apex version read as a ribbon on the light
  // theme, which is a bow, which is hat nine.
  <>
    <path className="litefill" d="M4.8 10.4 5.2 2.8l5 5.2z" />
    <path className="litefill" d="M19.2 10.4 18.8 2.8l-5 5.2z" />
  </>,
  // antenna
  <>
    <path
      d="M12 5.4V2.4"
      stroke="var(--on-color)"
      strokeWidth="3"
      strokeLinecap="round"
    />
    <path
      d="M12 5.4V2.4"
      stroke="var(--lite)"
      strokeWidth="1.6"
      strokeLinecap="round"
    />
    <circle className="litefill" cx="12" cy="1.6" r="1.9" />
  </>,
  // horn
  <path className="litefill" d="M12 9.6 8.9 5.4 12 .2l3.1 5.2z" />,
  // cap
  <>
    <path className="litefill" d="M4.6 6.4a7.4 6.2 0 0 1 14.8 0z" />
    <rect
      className="litefill"
      x="2.6"
      y="5.6"
      width="18.8"
      height="2.4"
      rx="1.2"
    />
  </>,
  // halo
  <>
    <ellipse
      cx="12"
      cy="2.6"
      rx="5.4"
      ry="2"
      fill="none"
      stroke="var(--on-color)"
      strokeWidth="3.4"
    />
    <ellipse
      cx="12"
      cy="2.6"
      rx="5.4"
      ry="2"
      fill="none"
      stroke="var(--lite)"
      strokeWidth="1.9"
    />
  </>,
  // cans
  <>
    <path
      d="M4.4 8.6a7.6 7.6 0 0 1 15.2 0"
      fill="none"
      stroke="var(--on-color)"
      strokeWidth="3.4"
    />
    <path
      d="M4.4 8.6a7.6 7.6 0 0 1 15.2 0"
      fill="none"
      stroke="var(--lite)"
      strokeWidth="1.9"
    />
    <rect
      className="litefill"
      x="2"
      y="6.4"
      width="4.2"
      height="5.2"
      rx="1.8"
    />
    <rect
      className="litefill"
      x="17.8"
      y="6.4"
      width="4.2"
      height="5.2"
      rx="1.8"
    />
  </>,
  // sprout
  <>
    <path
      d="M12 6.4V3"
      stroke="var(--on-color)"
      strokeWidth="3"
      strokeLinecap="round"
    />
    <path
      d="M12 6.4V3"
      stroke="var(--lite)"
      strokeWidth="1.6"
      strokeLinecap="round"
    />
    <path
      className="litefill"
      d="M12 3.6c0-2.6 2.4-3.6 4.2-3.2.2 2.2-1.8 3.8-4.2 3.2z"
    />
  </>,
  // tuft
  <path
    className="litefill"
    d="M5.8 9.2c0-2.4 1.5-3.6 2.9-3.4C8.9 3.4 10.4 2 12 2s3.1 1.4 3.3 3.8c1.4-.2 2.9 1 2.9 3.4z"
  />,
  // bow
  <>
    <path className="litefill" d="M11.2 6.2 6.6 2.9l-.8 5.9z" />
    <path className="litefill" d="M12.8 6.2l4.6-3.3.8 5.9z" />
    <circle className="litefill" cx="12" cy="6.4" r="1.7" />
  </>,
  // crown
  <path className="litefill" d="M4.8 8V1.6l3 2.4L12 .4l4.2 3.6 3-2.4V8z" />,
  // rotor
  <>
    <path
      d="M12 6.8V3.4"
      stroke="var(--on-color)"
      strokeWidth="3"
      strokeLinecap="round"
    />
    <path
      d="M12 6.8V3.4"
      stroke="var(--lite)"
      strokeWidth="1.6"
      strokeLinecap="round"
    />
    <path
      className="litefill"
      d="M12 3c-2.8-2.2-5.8-.9-5.8.6 2.8 1.5 4.8.5 5.8-.6zM12 3c2.8-2.2 5.8-.9 5.8.6-2.8 1.5-4.8.5-5.8-.6z"
    />
  </>,
  // bucket
  <>
    <path className="litefill" d="M6.9 8V4.6a5.1 5.1 0 0 1 10.2 0V8z" />
    <rect
      className="litefill"
      x="3.4"
      y="6.6"
      width="17.2"
      height="2.6"
      rx="1.3"
    />
  </>,
];

export function HatCrew(props: CrewRenderProps): JSX.Element {
  // Dealt, not hashed. The hat *is* the identity here, and the palette only
  // has eight colours, so a crew of twelve forces pairs onto one colour — the
  // exact pairs a person has to tell apart. A hash cannot see that two bots
  // are already hard to separate and will cheerfully give them the same hat as
  // well; the deal cannot repeat until it has run out of hats.
  const hat = HATS[dealIndex(props.id) % HATS.length];

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <g className="rig">
        {hat}
        <circle className="body" cx="12" cy="14.6" r="7.4" />
        <circle className="belly" cx="10.4" cy="12.8" r="5.2" opacity="0.5" />
        {/* The head sits low to leave the hat room, so the whole face moves
            down rather than every face path being renumbered for this one
            style. One group around both halves, and `.eyes` inside it rather
            than carrying the translate itself: the blink animates `transform`,
            and an animated `transform` replaces a `transform` *attribute* on
            the same element for the whole cycle — which left the eyes up by
            the crown while the mouth stayed put, and put them back only for
            people who had asked for less motion.

            `false` is the browless waiting face: raised brows under a brim
            fuse with it into a single black slab and the head loses its top
            half, so a hatted bot asks with its eyes alone. */}
        <g transform="translate(0 2.4)">
          <g className="eyes">{eyesFor(props, false)}</g>
          {mouthFor(props)}
        </g>
      </g>
    </svg>
  );
}
