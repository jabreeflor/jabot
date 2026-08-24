//! The contract five drawing styles are written against.
//!
//! Issue #44 asked for something better than the blob and got back five
//! answers, none of which is obviously the winner until it has been lived
//! with in the real shell. So the choice is deferred into a setting and all
//! five ship behind it. That only works if a style is a *pure function of a
//! bot* — no style may need a prop the others do not have, or the switch
//! stops being a switch and becomes five call sites. `CrewRenderProps` is
//! that floor, and it is deliberately four fields wide.
//!
//! `id` is in there and `name` is not enough on its own: the hats, the
//! critters and the watchers all deal a mark from the identity, and a
//! renamed bot that silently becomes a different animal would be a bug the
//! user cannot explain. The id is the thing that does not change.

import type { BotColor, RunState, ThreadState } from "../types";

/**
 * What the face is doing. Four, not eight: `RunState` has more cases than a
 * drawing can distinguish at 28px, and collapsing them here rather than in
 * each renderer means five agents cannot disagree about what "lost" looks
 * like.
 */
export type AvatarState = "idle" | "running" | "waiting" | "failed";

/** Everything a drawing is allowed to know about the bot it is drawing. */
export interface CrewRenderProps {
  /** Stable identity. Every dealt or hashed feature keys off this, never the
      name, so renaming a bot leaves its mark alone. */
  id: string;
  name: string;
  color: BotColor;
  state: AvatarState;
}

/** The five candidates, plus what ships today so the switch has a control. */
export type CrewStyle =
  | "classic"
  | "moodblob"
  | "hats"
  | "critters"
  | "pixels"
  | "watchers";

export const CREW_STYLES: readonly {
  id: CrewStyle;
  label: string;
  blurb: string;
}[] = [
  {
    id: "classic",
    label: "Classic",
    blurb: "The blob that ships today: a slightly-wrong circle with two eyes.",
  },
  {
    id: "moodblob",
    label: "Moodblob",
    blurb: "The same blob, awake — a face that answers and a tuft of its own.",
  },
  {
    id: "hats",
    label: "Hat crew",
    blurb: "One body, and the thing on its head is who it is.",
  },
  {
    id: "critters",
    label: "Critter kit",
    blurb: "Body, crest and face dealt from the id, so every bot is an animal.",
  },
  {
    id: "pixels",
    label: "Pixel pets",
    blurb: "Eight by eight and hand-drawn, the one style designed for small.",
  },
  {
    id: "watchers",
    label: "Watchers",
    blurb: "One, two or three eyes, and a bot that needs you looks at you.",
  },
];

/** Both judges landed on the hats, so that is what an untouched install gets. */
export const DEFAULT_CREW_STYLE: CrewStyle = "hats";

export const CREW_STYLE_KEY = "jabot.crewStyle";

/**
 * The app's own vocabulary, mapped onto a face.
 *
 * This mirrors `threadStatus` rather than reinventing it, and for the same
 * reason: visibility wins over machine state (#5). A folded thread reads as
 * asleep in the row, so its bot must not be caught mid-squint in the sidebar
 * — two surfaces disagreeing about one thread is worse than either being
 * wrong. Everything the drawing cannot usefully distinguish falls to idle,
 * which is the bot's own face and therefore never a lie.
 */
export function avatarStateFor(
  runState: RunState | null,
  threadState?: ThreadState,
): AvatarState {
  if (threadState === "folded" || threadState === "archived") return "idle";

  switch (runState) {
    case "running":
    case "queued":
      return "running";
    case "needs_you":
      return "waiting";
    case "failed":
    case "timed_out":
    case "lost":
      return "failed";
    default:
      return "idle";
  }
}
