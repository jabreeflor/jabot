//! The built-in bot catalog, drawn by open-bot-avatar
//! (https://github.com/DominikScholz/open-bot-avatar, MIT).
//!
//! Each entry is one avatar definition (a silhouette path in a 0..100 box and
//! where the eyes sit) plus the body color it is drawn in. The engine owns
//! volume, blinking, gaze and state motion; this file only picks the shapes.

import {
  getExample,
  renderAvatarSvg,
  type AvatarDefinition,
} from "open-bot-avatar";
import type { BotColor } from "../types";

/** The package's own starters, by id. Present in every release we pin. */
function example(id: string): AvatarDefinition {
  const definition = getExample(id);
  if (!definition) throw new Error(`open-bot-avatar has no "${id}" example`);
  return definition;
}

/* Copied verbatim from the project's `gallery/` (MIT): the package ships the
   starters but not the gallery. */
const CLOUD: AvatarDefinition = {
  version: 1,
  id: "cloud",
  name: "Cloud",
  body: {
    path: "M24 76C14 76 9 69 9 61C9 52 15 46 23 45.5C25 33 36 24 49 24C61 24 70 31 73 41C83 41 91 49 91 59C91 69 83 76 74 76Z",
  },
  eyes: { x: 50, y: 54, spacing: 17, scale: 1.15 },
};

const GHOST: AvatarDefinition = {
  version: 1,
  id: "ghost",
  name: "Ghost",
  body: {
    path: "M24 50C24 28 35 12 50 12C65 12 76 28 76 50L77 80C77 85 73 88 68 88C63 88 61 84 59 81C57 84 54 88 50 88C46 88 43 84 41 81C39 84 37 88 32 88C27 88 23 85 23 80Z",
  },
  eyes: { x: 50, y: 42, spacing: 16, scale: 1.15 },
};

const MAGNIFIER: AvatarDefinition = {
  version: 1,
  id: "magnifier",
  name: "Magnifier",
  body: {
    path: "M41 12A29 29 0 0 1 64.44 58.08L68.50 62.13A8 8 0 0 1 75.65 64.33L87.67 76.36A8 8 0 0 1 76.36 87.67L64.33 75.65A8 8 0 0 1 62.13 68.50L58.08 64.44A29 29 0 1 1 41 12Z",
  },
  eyes: { x: 41, y: 40, spacing: 17, scale: 1.15 },
};

const BRAIN: AvatarDefinition = {
  version: 1,
  id: "brain",
  name: "Brain",
  body: {
    path: "M14 50C10 42 14 32 22 30C24 21 34 17 41 21C46 15 56 15 61 20C68 16 78 20 79 28C87 30 91 39 88 47C92 54 89 64 81 66C80 71 76 74 71 73C67 75 64 74 62 72C61 74 61 76 61.5 78C62 81 59 82.5 57 81.5C55.5 80.5 55 78 54 76C53 74 50 72.5 47 72.5C42 74 38 72 36 70C30 73 20 71 18 64C12 62 10 56 14 50Z",
  },
  eyes: { x: 50, y: 45, spacing: 17, scale: 1.15 },
};

export interface BotIcon {
  /** The persisted appearance id (the host's `bots.color` slot). */
  readonly value: BotColor;
  /** The drawing's id; also the `data-character` on the rendered mark. */
  readonly id: string;
  readonly name: string;
  /** Body color, `#rrggbb`. The color the slot was always named after. */
  readonly color: string;
  readonly definition: AvatarDefinition;
}

/** Stable appearance IDs retain the host's existing `color` wire/storage slot.
 * Do not reorder the mapping: saved bots and older clients must round-trip it.
 * This catalog is vendored into the web companion by `sync:ui`.
 */
export const BOT_ICONS: readonly BotIcon[] = [
  {
    value: "b-teal",
    id: "cloud",
    name: "Cloud",
    color: "#14b3a5",
    definition: CLOUD,
  },
  {
    value: "b-yellow",
    id: "lightbulb",
    name: "Bulb",
    color: "#f8a01c",
    definition: example("lightbulb"),
  },
  {
    value: "b-purple",
    id: "chat",
    name: "Chat",
    color: "#8b5cf6",
    definition: example("chat"),
  },
  {
    value: "b-violet",
    id: "ghost",
    name: "Ghost",
    color: "#6366f1",
    definition: GHOST,
  },
  {
    value: "b-blue",
    id: "magnifier",
    name: "Seeker",
    color: "#1f7cf2",
    definition: MAGNIFIER,
  },
  {
    value: "b-orange",
    id: "controller",
    name: "Player",
    color: "#ff6a1a",
    definition: example("controller"),
  },
  {
    value: "b-pink",
    id: "brain",
    name: "Brain",
    color: "#ec4899",
    definition: BRAIN,
  },
  {
    value: "b-green",
    id: "sprout",
    name: "Sprout",
    color: "#1fbf68",
    definition: example("sprout"),
  },
];

export function botIcon(value: string): BotIcon {
  return BOT_ICONS.find((entry) => entry.value === value) ?? BOT_ICONS[0];
}

/** A still frame of the bot as an SVG string, for surfaces without React.
 *  Every value comes from the closed catalog above, never user input. */
export function botIconSvg(value: string, size = 96): string {
  const b = botIcon(value);
  const svg = renderAvatarSvg(b.definition, { color: b.color, size });
  return `<span class="bot-mark" data-character="${b.id}" aria-hidden="true">${svg}</span>`;
}
