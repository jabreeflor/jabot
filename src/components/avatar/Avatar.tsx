//! Bot icons use a solid fill in the bot’s colour, or an uploaded picture.
//! CSS controls sizing; unread dots and runtime state rings remain shared.

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
   * The bot's own picture, as a `data:` URL, or nothing for the solid colour.
   *
   * Checked rather than trusted: it goes straight into a `src`, and the value
   * has been through the host and back. A row carrying something else draws
   * the solid colour instead of fetching it.
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
        <ColorMark />
      )}
      {unread && <span className="dot" data-testid="unread-dot" />}
      {state !== "idle" && <span className="ring" data-testid="state-ring" />}
    </span>
  );
}

/** A flat colour tile shared by individual bots and the Crew cluster. */
function ColorMark() {
  return <span className="color-mark" aria-hidden="true" />;
}

/** Three colours identify the crew as a whole. Its control supplies the name. */
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
            <ColorMark />
          </span>
        </i>
      ))}
    </span>
  );
}
