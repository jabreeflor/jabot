/**
 * The catalog is exactly open-bot-avatar's avatars, every one valid, each in
 * upstream's default color, and every stored appearance ID is one the host
 * accepts.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { examples, validateAvatar } from "open-bot-avatar";
import { describe, expect, it } from "vitest";

import { BOT_COLORS } from "../../types";
import { BOT_ICONS, botIcon } from "../bots";
import DEFAULT_COLORS from "../gallery/colors.json";

const UPSTREAM_IDS = Object.keys(DEFAULT_COLORS);

describe("the bot catalog", () => {
  it("is every starter and every gallery avatar, starters first", () => {
    expect(BOT_ICONS.map((b) => b.id).sort()).toEqual([...UPSTREAM_IDS].sort());
    expect(BOT_ICONS.slice(0, examples.length).map((b) => b.id)).toEqual(
      examples.map((e) => e.id),
    );
  });

  it("draws each avatar unchanged, in upstream's own name and color", () => {
    for (const bot of BOT_ICONS) {
      expect(validateAvatar(bot.definition).ok, bot.id).toBe(true);
      expect(bot.name).toBe(bot.definition.name);
      expect(bot.color).toBe(
        (DEFAULT_COLORS as Record<string, string>)[bot.id],
      );
    }
  });

  it("gives every avatar its own stored ID, and every ID an avatar", () => {
    expect(BOT_ICONS.map((b) => b.value).sort()).toEqual(
      [...BOT_COLORS].sort(),
    );
  });

  it("keeps the original eight slots on the avatars they already draw", () => {
    expect(
      Object.fromEntries(
        BOT_COLORS.slice(0, 8).map((value) => [value, botIcon(value).id]),
      ),
    ).toEqual({
      "b-teal": "cloud",
      "b-yellow": "lightbulb",
      "b-purple": "chat",
      "b-violet": "ghost",
      "b-blue": "magnifier",
      "b-orange": "controller",
      "b-pink": "brain",
      "b-green": "sprout",
    });
  });

  it("draws an unknown value as the host's default slot", () => {
    expect(botIcon("beige").value).toBe("b-teal");
  });

  it("matches the host's BOT_COLORS, in order", () => {
    const rust = readFileSync(
      resolve(__dirname, "../../../../src-tauri/src/host/crew/mod.rs"),
      "utf8",
    );
    const block = /pub const BOT_COLORS: &\[&str\] = &\[([^\]]*)\]/.exec(rust);
    expect(block, "BOT_COLORS in crew/mod.rs").not.toBeNull();
    const hostColors = [...block![1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(hostColors).toEqual([...BOT_COLORS]);
  });
});
