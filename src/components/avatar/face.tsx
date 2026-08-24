//! The face, shared by every style that has one.
//!
//! This is the argument the character page made: state does not have to be a
//! badge stuck on the rim. A badge is what you draw when the avatar cannot
//! emote, and these can, so waiting and failed are said with the eyes and the
//! corner stays free for the unread dot — which then keeps one meaning
//! instead of competing with a second coloured pip.
//!
//! Three of the five styles pull from here rather than each drawing their
//! own, because the part that varies between them is what the face is
//! attached to, not the face. That also fixes the thing a shared vocabulary
//! is actually for: "failed" looks the same on a moodblob, a critter and a
//! hatted head, so a person learns it once.
//!
//! Every path is drawn in the same 24-unit box the bodies use, with the eyes
//! on the y = 11.4 line. A style whose head sits lower translates the whole
//! face down rather than nudging the numbers — that is why the hat crew and
//! the critters wrap these in a `<g transform="translate(...)">`.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { pick } from "./hash";

/**
 * Six pairs of idle eyes. Order matters and must not be rearranged: `pick`
 * indexes into this array, so moving an entry silently reassigns every bot's
 * face.
 */
const EYES: readonly JSX.Element[] = [
  // round
  <>
    <circle cx="8.6" cy="11.4" r="2.2" />
    <circle cx="15.4" cy="11.4" r="2.2" />
  </>,
  // tall
  <>
    <ellipse cx="8.7" cy="11.4" rx="1.7" ry="2.8" />
    <ellipse cx="15.3" cy="11.4" rx="1.7" ry="2.8" />
  </>,
  // beady
  <>
    <circle cx="7.7" cy="11.2" r="1.4" />
    <circle cx="16.3" cy="11.2" r="1.4" />
  </>,
  // sleepy
  <>
    <path d="M6.3 11.8a2.3 2.3 0 0 1 4.6 0z" />
    <path d="M13.1 11.8a2.3 2.3 0 0 1 4.6 0z" />
  </>,
  // wide
  <>
    <circle cx="8.4" cy="11.2" r="3" />
    <circle cx="15.6" cy="11.2" r="3" />
  </>,
  // oval
  <>
    <ellipse cx="8.5" cy="11.4" rx="2.5" ry="1.9" />
    <ellipse cx="15.5" cy="11.4" rx="2.5" ry="1.9" />
  </>,
];

/** Three idle mouths. Small talk next to the eyes, so these hash. */
const MOUTHS: readonly JSX.Element[] = [
  <path className="inkstroke" d="M10.6 16q1.4 1.1 2.8 0" />,
  <path className="inkstroke" d="M10.8 16.2h2.4" />,
  <path className="inkstroke" d="M10.6 16.4q1.4-1 2.8 0" />,
];

/**
 * The eyes. Idle is the bot's own pair, dealt off its id; every other state
 * takes the face over, because a face is worth having precisely because it
 * can say the thing the rim cannot.
 *
 * `browed` is the hat crew's escape hatch. Raised brows are the clearest tell
 * that a bot wants something, but under a brim the brow and the hat fuse into
 * one black slab and the head loses its top half, so a hatted face raises its
 * eyes and leaves the brows off.
 */
export function eyesFor(
  props: CrewRenderProps,
  browed: boolean = true,
): JSX.Element {
  if (props.state === "running") {
    // Squinting at the work.
    return (
      <>
        <path className="inkstroke" d="M6.4 12.2q2.2-2.6 4.4 0" />
        <path className="inkstroke" d="M13.2 12.2q2.2-2.6 4.4 0" />
      </>
    );
  }
  if (props.state === "failed") {
    return (
      <>
        <path className="inkstroke" d="M6.6 9.8l3.6 3.4M10.2 9.8l-3.6 3.4" />
        <path className="inkstroke" d="M13.8 9.8l3.6 3.4M17.4 9.8l-3.6 3.4" />
      </>
    );
  }
  if (props.state === "waiting") {
    return (
      <>
        <circle className="ink" cx="8.4" cy="11.4" r="2.9" />
        <circle className="ink" cx="15.6" cy="11.4" r="2.9" />
        {browed && browsFor()}
      </>
    );
  }
  return <g className="ink">{pick(EYES, props.id, "e")}</g>;
}

/**
 * The raised pair, on their own. `eyesFor` already draws these for a waiting
 * face; they are exported so a style that wants a brow over eyes it drew
 * itself — the watchers, whose eyes are spheres rather than ink — can reuse
 * the same two strokes instead of approximating them.
 */
export function browsFor(): JSX.Element {
  return (
    <>
      <path className="inkstroke" d="M6.4 8.2q2.3-1.3 4.6-.3" />
      <path className="inkstroke" d="M17.6 8.2q-2.3-1.3-4.6-.3" />
    </>
  );
}

/** The mouth. Same rule: idle is the bot, the rest is the state. */
export function mouthFor(props: CrewRenderProps): JSX.Element {
  if (props.state === "running") {
    return <path className="inkstroke" d="M10.4 16.2q1.6 1.4 3.2 0" />;
  }
  if (props.state === "waiting") {
    return <circle className="ink" cx="12" cy="16.6" r="1.5" />;
  }
  if (props.state === "failed") {
    return <path className="inkstroke" d="M10.2 17.2q1.8-1.8 3.6 0" />;
  }
  return pick(MOUTHS, props.id, "m");
}
