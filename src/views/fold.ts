//! Fold, wired to a real session (#26).
//!
//! Fold is the one gesture in JaBot that is *only* about visibility. The row
//! leaves the sidebar and the subprocess carries on, so the call this makes
//! must never be mistaken for a stop: `thread/fold` writes the overlay state
//! and the policy, and the host keeps the adapter exactly as it found it.
//!
//! Two things make this more than a one-line RPC.
//!
//! **"Disappear until done" sends no policy at all.** `state-machine.md` gives
//! the in-chat card that gesture and says it *keeps* the thread's current
//! `foldPolicy`; only Wait for Inbox changes it. Sending `default` would
//! quietly undo a quieter policy the user chose earlier, which is the opposite
//! of what the menu item says.
//!
//! **A refused fold has to put the row back.** The transition table forbids
//! folding a thread that has already come back to you, and the shell animates
//! the row out before the call lands. So the reload runs whether the host took
//! the fold or not — an error must leave the sidebar telling the truth, not
//! holding a row that is still active but no longer drawn.

import { useCallback, useState } from "react";

import {
  HostRpcError,
  RPC_ERROR,
  type FoldPolicy,
  type HostClient,
  type ThreadStateResult,
} from "../host";

export interface FoldRequest {
  threadId: string;
  /** Omit for "Disappear until done" — the thread keeps the policy it has. */
  policy?: FoldPolicy;
}

export interface FoldThread {
  /** Resolves with the folded thread, or `null` when the host refused it. */
  fold: (request: FoldRequest) => Promise<ThreadStateResult | null>;
  /** The last refusal, in the user's words rather than the wire's. */
  error: string | null;
  clearError: () => void;
}

/**
 * @param onSettled Re-read whatever lists the fold moved a row between. Called
 * on success *and* on failure, for the reason in the module docs.
 */
export function useFoldThread(
  client: HostClient | null,
  onSettled?: () => void,
): FoldThread {
  const [error, setError] = useState<string | null>(null);

  const fold = useCallback(
    async (request: FoldRequest) => {
      if (!client) return null;
      try {
        const result = await client.fold(
          request.policy
            ? { threadId: request.threadId, policy: request.policy }
            : { threadId: request.threadId },
        );
        setError(null);
        return result;
      } catch (err: unknown) {
        setError(foldMessage(err));
        return null;
      } finally {
        onSettled?.();
      }
    },
    [client, onSettled],
  );

  const clearError = useCallback(() => setError(null), []);

  return { fold, error, clearError };
}

/**
 * The two refusals a user can actually cause, said in terms of what they did.
 *
 * `IllegalTransition` on a fold means the thread has already resurfaced — it
 * is back in front of you, and the way to send it away again is to open it
 * first. Telling them "illegal transition" would describe our state machine
 * instead of their thread.
 */
function foldMessage(err: unknown): string {
  if (err instanceof HostRpcError) {
    if (err.code === RPC_ERROR.ILLEGAL_TRANSITION) {
      return "That thread has already come back to you. Open it, then fold it again.";
    }
    if (err.code === RPC_ERROR.THREAD_NOT_FOUND) {
      return "That thread is gone.";
    }
    return err.message;
  }
  return err instanceof Error ? err.message : String(err);
}
