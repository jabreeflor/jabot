//! Compact agent presence: BotMark (or uploaded picture) plus the name.
//!
//! A pill is how an agent shows up *in* a chat — the header, a mention, a
//! composer suggestion — rather than only as a sidebar row or a crew card.
//! Colour IDs still select the monochrome silhouette; this is not a colour
//! circle and not the old spritesheet.

import { Avatar, type AvatarState } from "./avatar";
import type { Bot } from "./types";

export type AgentPillVariant = "title" | "presence" | "mention" | "starter";

export function AgentPill({
  bot,
  variant = "presence",
  state = "idle",
  selected = false,
  actionLabel,
  onClick,
}: {
  bot: Pick<Bot, "name" | "color" | "image">;
  variant?: AgentPillVariant;
  state?: AvatarState;
  selected?: boolean;
  /** Accessible name when the visible label would collide with another control. */
  actionLabel?: string;
  onClick?: () => void;
}) {
  const interactive = Boolean(onClick);
  const className = [
    "agent-pill",
    `agent-pill-${variant}`,
    selected ? "is-selected" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const inner = (
    <>
      <Avatar
        name={bot.name}
        color={bot.color}
        image={bot.image}
        state={state}
        titled={false}
      />
      <span className="agent-pill-name">{bot.name}</span>
    </>
  );

  if (variant === "title") {
    return <h2 className={className}>{inner}</h2>;
  }

  if (interactive) {
    return (
      <button
        type="button"
        className={className}
        aria-current={selected || undefined}
        aria-label={
          actionLabel ??
          (variant === "starter"
            ? `Start as ${bot.name}`
            : `Open chat with ${bot.name}`)
        }
        onClick={onClick}
      >
        {inner}
      </button>
    );
  }

  return <span className={className}>{inner}</span>;
}
