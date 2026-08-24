/**
 * `avatarStateFor`, the one place the app's vocabulary becomes a face.
 *
 * `RunState` has eight cases and a drawing at 28px can carry four, so
 * something has to collapse them, and it matters that it happens once rather
 * than in each of six renderers. The rule it collapses by is #5's: a thread's
 * state says whether you can see it, its latest run says what the machine is
 * doing, and when the two disagree visibility wins. A folded thread reads as
 * asleep in its row, so its bot must not be caught mid-squint in the sidebar —
 * two surfaces disagreeing about one thread is worse than either being wrong.
 */
import { describe, expect, it } from "vitest";

import { avatarStateFor } from "../crew";
import type { RunState, ThreadState } from "../../types";

describe("avatarStateFor", () => {
  it("squints while the work is in flight, queued or running", () => {
    expect(avatarStateFor("running")).toBe("running");
    // Queued is running on purpose: from the outside a turn that is about to
    // start and one that has started are the same wait.
    expect(avatarStateFor("queued")).toBe("running");
  });

  it("looks up when the run wants something from you", () => {
    expect(avatarStateFor("needs_you")).toBe("waiting");
  });

  it("winces at all three ways a run can end badly", () => {
    for (const run of ["failed", "timed_out", "lost"] as const) {
      expect(avatarStateFor(run)).toBe("failed");
    }
  });

  it("wears its own face for everything a drawing cannot usefully say", () => {
    // Succeeded and cancelled are both "nothing is happening now", and idle is
    // the bot's own face, so falling to it is never a lie.
    expect(avatarStateFor(null)).toBe("idle");
    expect(avatarStateFor("succeeded")).toBe("idle");
    expect(avatarStateFor("cancelled")).toBe("idle");
  });

  it("goes idle for a folded or archived thread whatever the run is doing", () => {
    const RUNS: readonly RunState[] = [
      "queued",
      "running",
      "needs_you",
      "failed",
      "timed_out",
      "lost",
    ];
    for (const hidden of ["folded", "archived"] as const) {
      for (const run of RUNS) {
        expect(avatarStateFor(run, hidden), `${run} + ${hidden}`).toBe("idle");
      }
    }
  });

  it("leaves an active or resurfaced thread's run to speak", () => {
    // The other half of the same rule: only the two states that hide a thread
    // override it. Resurfaced is visible, so a resurfaced thread that needs
    // you still shows a bot that needs you.
    const VISIBLE: readonly ThreadState[] = ["active", "resurfaced"];
    for (const state of VISIBLE) {
      expect(avatarStateFor("needs_you", state)).toBe("waiting");
      expect(avatarStateFor("running", state)).toBe("running");
      expect(avatarStateFor("lost", state)).toBe("failed");
    }
  });

  it("is idle for a thread with no run at all, however it is filed", () => {
    for (const state of [
      "active",
      "folded",
      "resurfaced",
      "archived",
    ] as const) {
      expect(avatarStateFor(null, state)).toBe("idle");
    }
  });
});
