//! Turning a bot id into a drawing, two ways.
//!
//! Both are here because they fail differently and the styles need both.

/**
 * FNV-1a, 32-bit. Not chosen for its statistics — chosen because it is six
 * lines, has no dependency, and gives the same answer in this process as it
 * did in the prototype, which is what makes the port checkable by eye.
 *
 * `Math.imul` is the load-bearing part: a plain `h * 16777619` leaves the
 * float range and the low bits stop being the ones that vary.
 */
export function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/**
 * One feature off a list. The salt is what lets a single id choose eyes, a
 * mouth and a tuft independently instead of three features that move
 * together.
 */
export function pick<T>(arr: readonly T[], id: string, salt: string): T {
  return arr[hash(id + salt) % arr.length];
}

/**
 * Deal a list round-robin across a known roster, rather than hashing each id
 * into it.
 *
 * Hashing is fine when the feature is a detail nobody would compare, and
 * wrong when the feature *is* the identity. The palette only has eight
 * colours, so a crew of twelve forces four pairs onto one colour; those pairs
 * are the ones a person actually has to tell apart, and a hash has no way of
 * knowing that two bots share a colour, so it will happily give the pair the
 * same hat as well. Dealing looks at the roster as a whole and cannot: every
 * bot gets a different mark until the list runs out, and only then does it
 * wrap. The cost is that a mark is no longer a property of the bot alone —
 * add a bot in the middle of the roster and the ones after it shift — which
 * is why the styles whose mark is a small talking point (moodblob's tuft,
 * critter mouths) still hash.
 */
export function dealt<T>(
  items: readonly T[],
  keys: readonly string[],
): Record<string, T> {
  const out: Record<string, T> = {};
  keys.forEach((key, i) => {
    out[key] = items[i % items.length];
  });
  return out;
}

/** Every id this process has drawn, in the order it first drew them. */
const dealOrder = new Map<string, number>();

/**
 * A bot's place in the deal, when there is no roster to hand.
 *
 * The prototype could deal against a `BOTS` constant. A renderer here cannot:
 * it is given one bot and the crew is whatever the host happens to have, so
 * the roster does not exist at the point the mark has to be chosen. This is
 * the same deal done incrementally — first bot drawn takes the first mark,
 * and an id already seen keeps the number it was given.
 *
 * Two consequences worth knowing before using it. The order is the order the
 * app first painted each bot, which in practice is the sidebar's roster
 * order, so it is stable across a session and across restarts of an unchanged
 * crew. And deleting a bot shifts everyone drawn after it — the cost the
 * `dealt` comment names, paid here too. Use it for the feature that *is* the
 * identity, and keep hashing the details.
 */
export function dealIndex(id: string): number {
  const seen = dealOrder.get(id);
  if (seen !== undefined) return seen;
  const next = dealOrder.size;
  dealOrder.set(id, next);
  return next;
}
