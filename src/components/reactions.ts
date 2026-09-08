//! Emoji reactions on an agent's reply (#265).
//!
//! A single-user app: one person, one set of marks, no counts. Toggling the
//! same emoji off is what keeps a double-click from minting a duplicate.

export interface ReactionChoice {
  emoji: string;
  /** Spoken name — the accessible label, never shown as chrome. */
  name: string;
}

/** The discoverable palette. Short on purpose: a picker is a choice, not a
    keyboard. */
export const REACTION_CHOICES: readonly ReactionChoice[] = [
  { emoji: "👍", name: "thumbs up" },
  { emoji: "❤️", name: "heart" },
  { emoji: "😂", name: "joy" },
  { emoji: "🎉", name: "celebration" },
  { emoji: "🤔", name: "thinking" },
  { emoji: "👀", name: "eyes" },
  { emoji: "🚀", name: "rocket" },
  { emoji: "✅", name: "check mark" },
];

export function reactionName(emoji: string): string {
  return (
    REACTION_CHOICES.find((choice) => choice.emoji === emoji)?.name ?? emoji
  );
}

/** Add `emoji` if it is missing, drop it if it is already there. Order is
    insertion order so a badge does not jump when a neighbour is toggled. */
export function toggleReaction(
  current: readonly string[] | undefined,
  emoji: string,
): string[] {
  const list = current ?? [];
  return list.includes(emoji)
    ? list.filter((item) => item !== emoji)
    : [...list, emoji];
}
