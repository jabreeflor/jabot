//! Candidate 01, Moodblob: the blob we already ship, with a face that answers
//! back and a bit of itself sticking out.
//!
//! This is the least expensive of the five to accept, because it changes the
//! smallest number of things. The mark stays a blob, so nothing a person has
//! learned about the sidebar stops being true; what it gains is an outline
//! that varies, a tuft, and a face that says what the bot is doing. Set it
//! next to Classic in the switch and the difference is entirely expression,
//! which is the argument #44 was making.
//!
//! Body and tuft are hashed off the id rather than dealt. A tuft is a detail
//! you notice about a bot you already know, not the thing you identify it by
//! at 28px — that job belongs to the colour and the face — so a collision
//! between two bots costs nothing, and hashing buys back the property dealing
//! gives up: a bot keeps its cowlick when the crew around it changes.
//!
//! Ported from `prototypes/jabot-avatars-characters.html`, renderer 01. The
//! path data is the prototype's, unedited.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { eyesFor, mouthFor } from "./face";
import { pick } from "./hash";

/**
 * Five postures. A blob's outline is the only thing it has, so it had better
 * be doing some work: tall, squat, one leaning each way, and a pear. The
 * prototype's comment above this table still says four, from before the pear
 * was added in review — the table is what the page was judged on, so it is
 * the table that is right.
 *
 * Order is load-bearing. `pick` indexes into this array, so an entry moved is
 * every bot's silhouette reassigned.
 */
const WOBBLE: readonly string[] = [
  // tall
  "M12 2.4c4.7 0 7.9 3.4 7.9 9.4s-2.9 9.8-7.9 9.8-7.9-3.8-7.9-9.8 3.2-9.4 7.9-9.4z",
  // squat
  "M12 5c6.2 0 9.4 3 9.4 8s-3.2 8.4-9.4 8.4S2.6 18 2.6 13 5.8 5 12 5z",
  // leaning left
  "M11.2 3c5.4 0 9 3.6 9 8.8s-3.8 9.6-9.4 9.6-7.5-4.2-7.5-9.4S5.8 3 11.2 3z",
  // leaning right
  "M12.8 3c5.4 0 8.1 4 8.1 9.2s-1.9 9.2-7.5 9.2-9.4-4.4-9.4-9.6S7.4 3 12.8 3z",
  // pear
  "M12 3c4.4 0 7 3 7 7.4 0 4-3 4.6-3 7.2 0 2.2-1.6 3.6-4 3.6s-4-1.4-4-3.6c0-2.6-3-3.2-3-7.2C5 6 7.6 3 12 3z",
];

/**
 * The bit sticking out: a cowlick, a curl, a spike of hair, a quiff, a pair
 * of nubs. All of them are `body` paint, not `litefill`, because a tuft is
 * part of the creature rather than a thing it is wearing — that distinction
 * is the whole line between this candidate and the hat crew.
 */
const TUFT: readonly JSX.Element[] = [
  <path
    className="body"
    d="M11.4 3.6c-.2-2 .8-3.2 2.2-3.4-.4 1.6.2 2.4 1 3z"
  />,
  <path
    className="body"
    d="M11.6 3.4c-1.4-1.4-1.2-2.8-.2-3.4.4 1.4 1.4 1.8 2.4 1.8-.6 1-1.4 1.5-2.2 1.6z"
  />,
  <path className="body" d="M9.6 4.2 10.4 1l2 2.2L14 .8l1 3.4z" />,
  <path
    className="body"
    d="M12 3.8c0-2.4 1.6-3.4 3.2-3-.2 1.4-.8 2.2-1.6 2.6.8.4 1.2 1 1.2 1.8z"
  />,
  <>
    <ellipse className="body" cx="7.4" cy="7.4" rx="2.4" ry="1.9" />
    <ellipse className="body" cx="16.6" cy="7.4" rx="2.4" ry="1.9" />
  </>,
];

export function Moodblob(props: CrewRenderProps): JSX.Element {
  const body = pick(WOBBLE, props.id, "w");
  const tuft = pick(TUFT, props.id, "t");

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <g className="rig">
        {/* Tuft first, so the body paints over its root. Drawn after, every
            one of them reads as a shape hovering just off the head instead of
            as something the blob grew. */}
        {tuft}
        <path className="body" d={body} />
        {/* The belly is the body again, shrunk about the centre and shifted
            up and left towards the light — the same offset the shipping
            gradient puts its highlight at, so a moodblob is lit from where a
            classic blob is lit. */}
        <path
          className="belly"
          d={body}
          opacity="0.55"
          transform="translate(-0.6 -0.9) scale(0.94)"
          transformOrigin="12 12"
        />
        <g className="eyes">{eyesFor(props)}</g>
        {mouthFor(props)}
      </g>
    </svg>
  );
}
