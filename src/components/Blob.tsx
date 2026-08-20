/**
 * Bot avatars. A blob is a bot's identity everywhere it appears — sidebar tile,
 * chat header, crew card, Inbox row — so size is a CSS variable set by the
 * container rather than a prop threaded through four call sites.
 */

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
      {unread && <span className="dot" data-testid="unread-dot" />}
    </span>
  );
}

/** The crew as a whole — three faces, no single identity. */
export function BlobCluster({ className }: { className?: string }) {
  return (
    <span className={["cluster", className].filter(Boolean).join(" ")}>
      <i />
      <i />
      <i />
    </span>
  );
}
