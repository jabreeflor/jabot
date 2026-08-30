//! Bot avatars. The shared mascot is staged with each bot's accent colour and
//! motion signature everywhere it appears — sidebar tile, chat header, crew
//! card, Inbox row — while the caller still owns its size through one CSS var.

import mascotAvatar from "../assets/mascot-avatar.png";
import type { BotColor } from "./types";

export function Blob({
  color,
  unread = false,
  className,
}: {
  color: BotColor;
  /** The red dot: this bot's standing thread has something for you. */
  unread?: boolean;
  className?: string;
}) {
  return (
    <span className={["blob", color, className].filter(Boolean).join(" ")}>
      <span className="blob-stage" aria-hidden="true">
        <img src={mascotAvatar} alt="" draggable={false} />
      </span>
      {unread && <span className="dot" data-testid="unread-dot" />}
    </span>
  );
}

/** The crew as a whole — three mascot variations, no single identity. */
export function BlobCluster({ className }: { className?: string }) {
  return (
    <span className={["cluster", className].filter(Boolean).join(" ")}>
      <i className="b-teal">
        <img src={mascotAvatar} alt="" draggable={false} />
      </i>
      <i className="b-purple">
        <img src={mascotAvatar} alt="" draggable={false} />
      </i>
      <i className="b-violet">
        <img src={mascotAvatar} alt="" draggable={false} />
      </i>
    </span>
  );
}
