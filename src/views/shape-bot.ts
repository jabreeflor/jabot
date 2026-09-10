//! Chat-first bot identity: a bot starts as a conversation, not a form.
//!
//! Create writes a blank crew row (name + colour + harness) and opens its
//! standing thread. The first thing the user says becomes the persona — name
//! when they offer one, instructions always — and `crew/update` persists it.
//! Colour IDs stay the host's existing slot; this file never invents a new one.

import {
  BOT_COLORS,
  type Bot,
  type BotColor,
  type BotDraft,
  type BotTemplate,
  type HarnessCard,
} from "../components/types";

/** Placeholder until the conversation names the bot. */
export const UNFORMED_BOT_NAME = "New bot";

export const SHAPING_WELCOME =
  "I'm new. Tell me who I should be — a name, what I do, how I should work. I'll take shape from this conversation.";

/** Empty instructions are the unformed signal. Existing crew always has some. */
export function isUnformedBot(bot: Pick<Bot, "instructions">): boolean {
  return !bot.instructions.trim();
}

/** The host keeps `bots.color` inside a closed list. Unknown → green. */
export function asBotColor(color: string): BotColor {
  return (BOT_COLORS as readonly string[]).includes(color)
    ? (color as BotColor)
    : "b-green";
}

/** First unused colour id, then wrap. Seeded Chief is teal; blank create is green. */
export function nextBotColor(bots: readonly Pick<Bot, "color">[]): BotColor {
  const used = new Set(bots.map((bot) => bot.color));
  return (
    BOT_COLORS.find((color) => !used.has(color)) ??
    BOT_COLORS[bots.length % BOT_COLORS.length]
  );
}

export function blankBotDraft(
  bots: readonly Pick<Bot, "color">[],
  harnesses: readonly Pick<HarnessCard, "id">[],
): BotDraft {
  return {
    name: UNFORMED_BOT_NAME,
    color: nextBotColor(bots),
    instructions: "",
    tools: [],
    harnessId: harnesses[0]?.id ?? "",
  };
}

export function draftFromTemplate(template: BotTemplate): BotDraft {
  return {
    name: template.name,
    color: template.color,
    instructions: template.instructions,
    tools: [...template.tools],
    harnessId: template.harnessId,
    templateId: template.templateId,
  };
}

const EXPLICIT_NAME =
  /^(?:you(?:'re| are)|your name is|call (?:you|yourself)|this is)\s+["']?([A-Za-z][\w' -]{0,39}?)(?=[,.:!?]|\s+(?:a|an|who|that|to|for|and|—|-)\b|$)/i;

const NAME_FIELD = /^name:\s*["']?([A-Za-z][\w' -]{0,39}?)["']?\s*$/im;

const NOT_A_NAME =
  /^(please|help|i|i'm|im|can|could|would|hey|hi|hello|this|that|the|a|an)$/i;

function tidyName(raw: string): string {
  return raw
    .replace(/\s+/g, " ")
    .replace(/[.,:;!?]+$/g, "")
    .trim();
}

/**
 * Pull a display name out of the first shaping message when the user offers
 * one. The whole message is always the instructions — the chat *is* the spec.
 */
export function shapeFromMessage(
  text: string,
  current: Pick<Bot, "name">,
): Pick<BotDraft, "name" | "instructions"> {
  const instructions = text.trim();
  const explicit = instructions.match(EXPLICIT_NAME);
  if (explicit?.[1]) {
    return { name: tidyName(explicit[1]), instructions };
  }
  const named = instructions.match(NAME_FIELD);
  if (named?.[1]) {
    return { name: tidyName(named[1]), instructions };
  }
  const first = (instructions.split(/[.!?\n]/, 1)[0] ?? "").trim();
  const words = first.split(/\s+/).filter(Boolean);
  if (
    words.length >= 1 &&
    words.length <= 3 &&
    first.length <= 32 &&
    /^[A-Z]/.test(words[0]) &&
    !NOT_A_NAME.test(words[0])
  ) {
    return { name: tidyName(first), instructions };
  }
  return {
    name: current.name.trim() || UNFORMED_BOT_NAME,
    instructions,
  };
}

export type MentionPart =
  { type: "text"; value: string } | { type: "mention"; bot: Bot };

/** Longest name first so `@Bot Recruiter` wins over a shorter prefix. */
export function mentionPattern(bots: readonly Bot[]): RegExp | null {
  const names = [...bots]
    .map((bot) => bot.name.trim())
    .filter((name) => name.length > 0)
    .sort((a, b) => b.length - a.length);
  if (names.length === 0) return null;
  const body = names.map(escapeRegExp).join("|");
  return new RegExp(`@(${body})(?=\\s|[.,:;!?]|$)`, "gi");
}

export function splitMentions(
  text: string,
  bots: readonly Bot[],
): MentionPart[] {
  const pattern = mentionPattern(bots);
  if (!pattern) return [{ type: "text", value: text }];
  const parts: MentionPart[] = [];
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      parts.push({ type: "text", value: text.slice(cursor, index) });
    }
    const name = match[1] ?? "";
    const bot = bots.find(
      (row) => row.name.toLowerCase() === name.toLowerCase(),
    );
    if (bot) parts.push({ type: "mention", bot });
    else parts.push({ type: "text", value: match[0] });
    cursor = index + match[0].length;
  }
  if (cursor < text.length) {
    parts.push({ type: "text", value: text.slice(cursor) });
  }
  return parts.length > 0 ? parts : [{ type: "text", value: text }];
}

export function mentionedBots(text: string, bots: readonly Bot[]): Bot[] {
  const seen = new Set<string>();
  const out: Bot[] = [];
  for (const part of splitMentions(text, bots)) {
    if (part.type !== "mention" || seen.has(part.bot.id)) continue;
    seen.add(part.bot.id);
    out.push(part.bot);
  }
  return out;
}

/** Trailing `@query` in the composer, or null when a mention is not open. */
export function mentionQuery(text: string): string | null {
  const match = text.match(/(?:^|\s)@([^\s@]*)$/);
  return match ? (match[1] ?? "") : null;
}

export function insertMention(text: string, name: string): string {
  return text.replace(/(?:^|\s)@[^\s@]*$/, (chunk) => {
    const lead = chunk.startsWith("@") ? "" : (chunk[0] ?? "");
    return `${lead}@${name} `;
  });
}

export function filterMentionBots(bots: readonly Bot[], query: string): Bot[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...bots];
  return bots.filter((bot) => bot.name.toLowerCase().includes(needle));
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
