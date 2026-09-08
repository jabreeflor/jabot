/**
 * Initials that sit on a bot mark when eight colours are not enough (#44).
 *
 * First and last word, one letter for a one-word name, "?" when there is
 * nothing to take a letter from. Split by code point so an emoji is a
 * character, not half of one.
 */
import { describe, expect, it } from "vitest";

import { monogram } from "../monogram";

describe("monogram", () => {
  it("takes the first and last word of a multi-word name", () => {
    expect(monogram("Expense Manager")).toBe("EM");
    expect(monogram("Pull Request Watcher")).toBe("PW");
  });

  it("uses one letter for a one-word name", () => {
    expect(monogram("Chief")).toBe("C");
  });

  it("is a question mark when there is no word", () => {
    expect(monogram("")).toBe("?");
    expect(monogram("  ")).toBe("?");
  });

  it("keeps an emoji or other non-BMP first character intact", () => {
    expect(monogram("🤖 Bot")).toBe("🤖B");
  });
});
