//! The built-in bot catalog: exactly open-bot-avatar's avatars
//! (https://github.com/DominikScholz/open-bot-avatar, MIT) — the starters the
//! package ships, then its gallery (vendored in ./gallery), each in the default
//! color upstream gives it. The engine owns volume, blinking, gaze and state
//! motion; this file only maps a bot's stored appearance ID to an avatar.

import {
  examples,
  renderAvatarSvg,
  validateAvatar,
  type AvatarDefinition,
} from "open-bot-avatar";
import { BOT_COLORS, type BotColor } from "../types";
import DEFAULT_COLORS from "./gallery/colors.json";

/** Upstream's gallery, checked like any import and sorted by name — the same
    list and order the open-bot-avatar editor shows after its starters. */
const files = import.meta.glob<unknown>(
  ["./gallery/*.json", "!./gallery/colors.json"],
  { eager: true, import: "default" },
);

const GALLERY: AvatarDefinition[] = Object.values(files)
  .flatMap((raw) => {
    const result = validateAvatar(raw);
    return result.ok && !examples.some((e) => e.id === result.value.id)
      ? [result.value]
      : [];
  })
  .sort((a, b) => a.name.localeCompare(b.name));

/**
 * The original eight appearance slots, and the avatar each now draws. Bots
 * saved before this catalog existed carry these values, and older clients only
 * know these, so they keep their names. Every other avatar is `oba-<id>`.
 * Never change an entry: that would silently redraw someone's bot.
 */
const LEGACY_SLOTS: Readonly<Record<string, string>> = {
  cloud: "b-teal",
  lightbulb: "b-yellow",
  chat: "b-purple",
  ghost: "b-violet",
  magnifier: "b-blue",
  controller: "b-orange",
  brain: "b-pink",
  sprout: "b-green",
};

export interface BotIcon {
  /** The persisted appearance id (the host's `bots.color` slot). */
  readonly value: BotColor;
  /** The avatar's id; also the `data-character` on the rendered mark. */
  readonly id: string;
  readonly name: string;
  /** Body color, `#rrggbb`: upstream's default for this avatar. */
  readonly color: string;
  readonly definition: AvatarDefinition;
}

function appearanceId(id: string): BotColor {
  const value = LEGACY_SLOTS[id] ?? `oba-${id}`;
  if (!(BOT_COLORS as readonly string[]).includes(value)) {
    throw new Error(`open-bot-avatar "${id}" has no slot in BOT_COLORS`);
  }
  return value as BotColor;
}

/** Every avatar, in upstream's order: starters, then the gallery by name.
 *  This catalog is vendored into the web companion by `sync:ui`. */
export const BOT_ICONS: readonly BotIcon[] = [...examples, ...GALLERY].map(
  (definition) => ({
    value: appearanceId(definition.id),
    id: definition.id,
    name: definition.name,
    color:
      (DEFAULT_COLORS as Record<string, string>)[definition.id] ?? "#ff6a1a",
    definition,
  }),
);

/** What a value nothing recognises draws: the host's default slot. */
const FALLBACK = BOT_ICONS.find((entry) => entry.value === "b-teal")!;

export function botIcon(value: string): BotIcon {
  return BOT_ICONS.find((entry) => entry.value === value) ?? FALLBACK;
}

/** A still frame of the bot as an SVG string, for surfaces without React.
 *  Every value comes from the closed catalog above, never user input. */
export function botIconSvg(value: string, size = 96): string {
  const b = botIcon(value);
  const svg = renderAvatarSvg(b.definition, { color: b.color, size });
  return `<span class="bot-mark" data-character="${b.id}" aria-hidden="true">${svg}</span>`;
}
