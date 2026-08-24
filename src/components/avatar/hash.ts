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

/**
 * Where each id sits in the deal, and how many places have been handed out.
 *
 * The count is separate from the map because not every entry consumes a place:
 * a reserved id is written straight into the map and the counter never hears
 * about it. See `reserveDeal`.
 */
const dealOrder = new Map<string, number>();
let handedOut = 0;

/**
 * A bot's place in the deal, when there is no roster to hand.
 *
 * The prototype could deal against a `BOTS` constant. A renderer here cannot:
 * it is given one bot and the crew is whatever the host happens to have, so
 * the roster does not exist at the point the mark has to be chosen. This is
 * the same deal done incrementally — first bot drawn takes the first mark,
 * and an id already seen keeps the number it was given.
 *
 * On its own that made the mark a property of *paint order*, which is not a
 * property of the bot: a fresh install painted setup's Chief before the
 * sidebar's, and a decorative avatar burned a place that a real bot then never
 * got, so the same crew could come back from a restart wearing different hats.
 * `seedDealOrder` and `reserveDeal` are the two halves of the fix — seed the
 * roster the app already holds before anything paints, and give the avatars
 * that are not bots a fixed place instead of a place off the top of the deck.
 * Deleting a bot still shifts everyone dealt after it: that is the cost the
 * `dealt` comment names, paid here too.
 */
export function dealIndex(id: string): number {
  const seen = dealOrder.get(id);
  if (seen !== undefined) return seen;
  const next = handedOut++;
  dealOrder.set(id, next);
  return next;
}

/**
 * Deal the roster in roster order, which is what the prototype did.
 *
 * Idempotent, and only ever *adds*: an id that has already been drawn keeps
 * the place it was given, because a mark that changed under a person mid
 * session would be a different bot arriving. Call it as early as the roster is
 * known — the shell does, above the tree that draws any of it — and paint
 * order stops deciding anything.
 */
export function seedDealOrder(ids: readonly string[]): void {
  ids.forEach((id) => dealIndex(id));
}

/**
 * Pin an id to a place without taking one off the deck.
 *
 * For the avatars that are not bots: the crew cluster's three marks and the
 * bot editor's colour swatches. They have to draw *something*, and whatever
 * they draw has to be distinct from its neighbours, but they are not members
 * of the crew and a place spent on one is a place a real bot does not get —
 * eight swatches were enough on their own to push a seven-bot crew into
 * wearing a hat twice. Reserving is not sharing a bot's mark by accident: the
 * cluster deliberately wears the first, third and fourth, exactly as the
 * prototype's did.
 */
export function reserveDeal(id: string, index: number): void {
  dealOrder.set(id, index);
}
