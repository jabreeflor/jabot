//! A bot's name, reduced to the one or two letters that fit inside its mark.
//!
//! This is the whole answer to the half of #44 that colour could not reach.
//! Eight colours means a ninth bot repeats one, and a repeated colour used to
//! be the end of the signal — two teal discs and nothing else to go on. The
//! initials are a second channel that costs nothing, survives greyscale, and
//! keeps working at 28px where a drawn face does not.

/**
 * "Expense Manager" → "EM", "Chief" → "C", "  " → "?".
 *
 * First and *last* word rather than the first two, because a three-word name
 * is usually role-then-qualifier ("Pull Request Watcher" → "PW") and the
 * middle word is the one carrying least. One letter for a one-word name: "CH"
 * for Chief reads as an abbreviation of something rather than as an initial.
 *
 * Split by code point, not by `charAt`, so a name that opens with an emoji or
 * an astral-plane character yields that character instead of half of it.
 */
export function monogram(name: string): string {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const first = firstCharacter(words[0]);
  const last = words.length > 1 ? firstCharacter(words[words.length - 1]) : "";
  // `toLocaleUpperCase` and not `toUpperCase`: a Turkish "işler" starts with a
  // dotted capital in its own locale and a dotless one in English, and the
  // name is the user's, not the program's.
  return (first + last).toLocaleUpperCase();
}

function firstCharacter(word: string): string {
  return Array.from(word)[0] ?? "";
}
