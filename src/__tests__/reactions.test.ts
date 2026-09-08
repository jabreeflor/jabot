import { describe, expect, it } from "vitest";

import { reactionName, toggleReaction } from "../components/reactions";

describe("toggleReaction", () => {
  it("adds a missing mark and drops a present one", () => {
    expect(toggleReaction(undefined, "👍")).toEqual(["👍"]);
    expect(toggleReaction(["👍"], "👍")).toEqual([]);
    expect(toggleReaction(["👍"], "🎉")).toEqual(["👍", "🎉"]);
    expect(toggleReaction(["👍", "🎉"], "👍")).toEqual(["🎉"]);
  });

  it("does not mint a duplicate of a mark that is already there", () => {
    const once = toggleReaction(["👍"], "👍");
    expect(once.filter((emoji) => emoji === "👍")).toHaveLength(0);
    const twice = toggleReaction(["🎉", "👍"], "👍");
    expect(twice.filter((emoji) => emoji === "👍")).toHaveLength(0);
  });

  it("names a palette mark for an accessible label", () => {
    expect(reactionName("👍")).toBe("thumbs up");
    expect(reactionName("🛸")).toBe("🛸");
  });
});
