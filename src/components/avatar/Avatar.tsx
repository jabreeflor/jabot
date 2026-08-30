//! A bot's icon: the JaBot mascot in that bot's colour, or the picture the user
//! gave it. The mascot is one product identity everywhere it appears; colour,
//! resting angle, glance direction, and animation timing distinguish the crew.
//!
//! An uploaded image remains an explicit user override. It replaces the mascot
//! entirely while keeping the unread dot and state ring, so the newer icon
//! editor remains intact after the mascot became the default.
//!
//! There is no `size` prop, deliberately. The app sizes avatars from CSS
//! already: the sidebar sets `--blob-size: 54px` on the chief tile, chat.css
//! sets 28px on the header, cards.css 38px on an Inbox row. A prop would mean
//! every one of those stylesheets had to be replaced with a threaded number,
//! and the mascot stage scales to whatever box it lands in.

import mascotSpritesheet from "../../assets/mascot-spritesheet.webp";
import type { BotColor } from "../types";
import type { AvatarState } from "./state";
import { isBotImage } from "./image";

export function Avatar({
  name,
  color,
  image,
  state = "idle",
  unread = false,
  labelled = false,
  titled = true,
  className,
}: {
  name: string;
  color: BotColor;
  /**
   * The bot's own picture, as a `data:` URL, or nothing for the mascot.
   *
   * Checked rather than trusted: it goes straight into a `src`, and the value
   * has been through the host and back. A row carrying something else draws
   * the mascot instead of fetching it.
   */
  image?: string | null;
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
  /**
   * Draw without the name tooltip, for a caller whose own `title` says
   * something the avatar's would hide — nested tooltips resolve innermost
   * first, and the drawing is usually the half a person points at.
   */
  titled?: boolean;
  className?: string;
}) {
  const picture = image && isBotImage(image) ? image : null;

  return (
    <span
      className={["av", color, className].filter(Boolean).join(" ")}
      data-state={state}
      title={titled ? name : undefined}
      {...(labelled ? { role: "img", "aria-label": name } : {})}
    >
      {picture ? (
        // Empty alt, not the name: the wrapper is what carries the accessible
        // name when there is one to carry, and a nested one would announce
        // the bot twice wherever `labelled` is on.
        <img className="pic" src={picture} alt="" draggable={false} />
      ) : (
        <MascotMark />
      )}
      {unread && <span className="dot" data-testid="unread-dot" />}
      {state !== "idle" && <span className="ring" data-testid="state-ring" />}
    </span>
  );
}

/**
 * The product mascot inside the bot's colour well. This is a real frame atlas,
 * not a still image translated around the icon: CSS selects the row that
 * matches the bot's state and advances through its rendered poses.
 */
function MascotMark() {
  return (
    <span className="mascot-stage" aria-hidden="true">
      <img
        className="mascot mascot-sheet"
        src={mascotSpritesheet}
        alt=""
        draggable={false}
      />
    </span>
  );
}

/**
 * The crew as a whole — the one avatar that is not a single bot.
 *
 * Three mascot portraits in the three colours today's cluster already uses,
 * so the tile reads as several bots while keeping the product identity.
 *
 * `aria-hidden` on the wrapper: the controls this sits in ("Crew") are already
 * named by their own text.
 */
const CREW_COLORS: readonly BotColor[] = ["b-teal", "b-purple", "b-violet"];

export function CrewAvatar({ className }: { className?: string }) {
  return (
    <span
      className={["cluster", "av-cluster", className].filter(Boolean).join(" ")}
      aria-hidden="true"
    >
      {CREW_COLORS.map((color, i) => (
        // The slot class, not `:nth-child`. The prototype positioned these by
        // index and hit the trap it sets: `:nth-child` on a cluster whose
        // children each contain a whole drawing eventually matches something
        // inside one of them and rearranges its parts.
        <i className={`s${i + 1}`} key={color}>
          <span className={`av ${color}`} data-state="idle">
            <MascotMark />
          </span>
        </i>
      ))}
    </span>
  );
}
