//! The one component every call site uses, and the chrome all six styles share.
//!
//! The split is the whole point of the module: a renderer draws a creature and
//! nothing else, and everything that is true of *any* avatar — the box, the
//! colour, the unread dot, the running ring, the accessible name — lives here.
//! Six copies of the dot is six chances for one of them to sit a pixel off,
//! and the prototype had already settled where it goes.
//!
//! There is no `size` prop, deliberately. The app sizes avatars from CSS
//! already: the sidebar sets `--blob-size: 54px` on the chief tile, chat.css
//! sets 28px on the header, cards.css 38px on an Inbox row. A prop would mean
//! every one of those stylesheets had to be replaced with a threaded number,
//! and the drawings are 24-unit SVGs that scale to whatever box they land in,
//! so there is nothing a number would buy.

import type { CSSProperties, JSX } from "react";
import type { BotColor } from "../types";
import { useCrewStyle } from "./CrewStyleContext";
import type { AvatarState, CrewRenderProps, CrewStyle } from "./crew";
import { hash } from "./hash";
import { Classic } from "./Classic";
import { Moodblob } from "./Moodblob";
import { HatCrew } from "./HatCrew";
import { CritterKit } from "./CritterKit";
import { PixelPets } from "./PixelPets";
import { Watchers } from "./Watchers";

/**
 * Rendered as elements rather than called as functions, so a style is free to
 * hold state or read a context of its own — the watchers want a gaze, and a
 * plain function call would have made that a rewrite rather than an edit.
 */
const RENDERERS: Record<CrewStyle, (props: CrewRenderProps) => JSX.Element> = {
  classic: Classic,
  moodblob: Moodblob,
  hats: HatCrew,
  critters: CritterKit,
  pixels: PixelPets,
  watchers: Watchers,
};

export function Avatar({
  id,
  name,
  color,
  state = "idle",
  unread = false,
  labelled = false,
  className,
}: {
  id: string;
  name: string;
  color: BotColor;
  state?: AvatarState;
  /** The red dot: this bot's standing thread has something for you. */
  unread?: boolean;
  /**
   * Expose the name to assistive technology, not just as a tooltip.
   *
   * Off by default and that is not laziness. Every current call site puts the
   * bot's name in text right beside the avatar — `<small>` in the sidebar
   * tile, `<h2>` in the chat header, `.nm` on the crew card — and a control
   * takes its accessible name from its contents, so labelling the avatar too
   * makes the sidebar button announce "Mira Mira". Turn this on where the
   * avatar really is the only thing inside its control, which is the case #44
   * is actually complaining about.
   */
  labelled?: boolean;
  className?: string;
}) {
  const style = useCrewStyle();
  const Draw = RENDERERS[style];

  // Each bot blinks on its own clock, and switching styles reshuffles them:
  // a crew that blinks in unison reads as one animation rather than as a room
  // of separate creatures. Negative, so the offset is into a cycle already
  // running instead of a pause before the first blink.
  const blink = (hash(id + style) % 40) / 10;

  return (
    <span
      className={["av", style, color, className].filter(Boolean).join(" ")}
      data-state={state}
      title={name}
      style={{ "--blink": `-${blink}s` } as CSSProperties}
      {...(labelled ? { role: "img", "aria-label": name } : {})}
    >
      <Draw id={id} name={name} color={color} state={state} />
      {unread && <span className="dot" data-testid="unread-dot" />}
      {state === "running" && <span className="ring" />}
    </span>
  );
}

/**
 * The crew as a whole — the one avatar that is not a single bot.
 *
 * Built from three of the current style's own marks rather than from three
 * generic circles, because the cluster is where a person reads what the crew
 * *is*: three hats say "hats" at a glance in a way three teal blobs never
 * would. The three colours are the ones today's cluster already uses, so the
 * tile does not change hue when the style does.
 *
 * `aria-hidden` on the wrapper: all three marks carry a `title`, and the
 * controls this sits in ("Crew") are already named by their own text.
 */
const CREW_FACES: readonly { id: string; name: string; color: BotColor }[] = [
  { id: "crew.a", name: "Crew", color: "b-teal" },
  { id: "crew.b", name: "Crew", color: "b-purple" },
  { id: "crew.c", name: "Crew", color: "b-violet" },
];

export function CrewAvatar({ className }: { className?: string }) {
  return (
    <span
      className={["cluster", "av-cluster", className].filter(Boolean).join(" ")}
      aria-hidden="true"
    >
      {CREW_FACES.map((face, i) => (
        // The slot class, not `:nth-child`. blob.css already ships an
        // unscoped `.cluster i`, and the prototype hit the same trap from the
        // other side: a selector that is not pinned to the direct children
        // also reaches into the drawing and rearranges its parts.
        <i className={`s${i + 1}`} key={face.id}>
          <Avatar id={face.id} name={face.name} color={face.color} />
        </i>
      ))}
    </span>
  );
}
