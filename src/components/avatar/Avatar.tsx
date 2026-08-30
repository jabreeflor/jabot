//! A bot's icon: a colour, its initials, or the picture the user gave it.
//!
//! #44 asked what should replace the blob and shipped six answers behind a
//! switch so they could be lived with. The answer that came back is that a
//! bot's identity is not something the app should be inventing for it — a
//! dealt hat is a stranger's face, and no generated creature is ever the one
//! the user had in mind. So the app draws the plainest thing that is still
//! legible, and gets out of the way of anyone who wants to say who a bot is:
//!
//!   * **A colour and a monogram.** Flat, one disc, the bot's initials in it.
//!     Legible at 28px, legible in greyscale, and — unlike a palette of eight
//!     — it does not run out at the ninth bot, which was #44's first
//!     complaint.
//!   * **An uploaded image**, when there is one. It replaces the disc
//!     entirely; the chrome around it does not change.
//!
//! State still has somewhere to live, which was #44's last requirement: the
//! ring. It is the same ring the running state always drew, now in three
//! colours, because a face that can squint is exactly what this change gave
//! up and something has to say "this one needs you" at a glance.
//!
//! There is no `size` prop, deliberately. The app sizes avatars from CSS
//! already: the sidebar sets `--blob-size: 54px` on the chief tile, chat.css
//! sets 28px on the header, cards.css 38px on an Inbox row. A prop would mean
//! every one of those stylesheets had to be replaced with a threaded number,
//! and the disc is a 24-unit SVG that scales to whatever box it lands in.

import type { BotColor } from "../types";
import type { AvatarState } from "./state";
import { isBotImage } from "./image";
import { monogram } from "./monogram";

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
   * The bot's own picture, as a `data:` URL, or nothing for the colour mark.
   *
   * Checked rather than trusted: it goes straight into a `src`, and the value
   * has been through the host and back. A row carrying something else draws
   * the monogram instead of fetching it.
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
        <Mark name={name} />
      )}
      {unread && <span className="dot" data-testid="unread-dot" />}
      {state !== "idle" && <span className="ring" data-testid="state-ring" />}
    </span>
  );
}

/**
 * The colour mark: a disc, and one or two letters.
 *
 * A 24-unit SVG rather than a styled `<div>` with text in it, because that is
 * what makes the initials the same shape at 54px and at 28px — an SVG glyph
 * scales with the box, where a font-size in a shrinking box has to be
 * recomputed at every call site and rounds to something slightly wrong at each
 * of them.
 */
function Mark({ name }: { name: string }) {
  const letters = monogram(name);
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {/* Inset by the stroke's own half-width, so the rim is drawn inside the
          box and not clipped in half by it. */}
      <circle className="disc" cx="12" cy="12" r="11.2" />
      <text
        className="initials"
        x="12"
        y="12"
        // The two-letter form is set smaller because it is nearly twice as
        // wide; both are chosen to sit inside the disc rather than to fill it.
        fontSize={letters.length > 1 ? 10 : 13}
      >
        {letters}
      </text>
    </svg>
  );
}

/**
 * The crew as a whole — the one avatar that is not a single bot.
 *
 * Three discs in the three colours today's cluster already uses, so the tile
 * does not change hue. They are marks rather than members: no monogram, since
 * "C" three times says nothing, and the shape alone is what reads as "several
 * bots" beside the single disc of one.
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
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <circle className="disc" cx="12" cy="12" r="11.2" />
            </svg>
          </span>
        </i>
      ))}
    </span>
  );
}
