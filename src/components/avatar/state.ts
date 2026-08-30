//! What an avatar is doing, in the four cases a mark can actually show.

import type { RunState, ThreadState } from "../types";

/**
 * Four, not eight: `RunState` has more cases than a ring can distinguish at
 * 28px, and collapsing them once here rather than at each of the five call
 * sites means the sidebar and the chat header cannot disagree about what
 * "lost" looks like.
 */
export type AvatarState = "idle" | "running" | "waiting" | "failed";

/**
 * The app's own vocabulary, mapped onto an avatar.
 *
 * This mirrors `threadStatus` rather than reinventing it, and for the same
 * reason: visibility wins over machine state (#5). A folded thread reads as
 * asleep in the row, so its bot must not be ringed as busy in the sidebar —
 * two surfaces disagreeing about one thread is worse than either being wrong.
 * Everything the mark cannot usefully distinguish falls to idle, which is the
 * bot's plain icon and therefore never a lie.
 */
export function avatarStateFor(
  runState: RunState | null,
  threadState?: ThreadState,
): AvatarState {
  if (threadState === "folded" || threadState === "archived") return "idle";

  switch (runState) {
    case "running":
    case "queued":
      return "running";
    case "needs_you":
      return "waiting";
    case "failed":
    case "timed_out":
    case "lost":
      return "failed";
    default:
      return "idle";
  }
}
