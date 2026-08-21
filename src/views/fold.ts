//! Fold, Archive and Delete, wired to a real session (#26).
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
//!
//! Archive and Delete sit here for the same reason and with more at stake.
//! They are the other two items on the row's menu, they are the same
//! animate-then-call ordering, and unlike a fold they are not only about
//! visibility: `thread/archive` and `thread/delete` withdraw the outstanding
//! permissions, drain the queued prompts, close the open run and release the
//! worktree (#20, #23). A menu item that moved a fixture instead would leave
//! all of that running behind a row that is no longer drawn.

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

export interface ThreadActions {
  /** Resolves with the folded thread, or `null` when the host refused it. */
  fold: (request: FoldRequest) => Promise<ThreadStateResult | null>;
  /** Close the thread out: the overlay keeps the transcript, the process and
      the worktree go. `null` when the host refused it. */
  archive: (threadId: string) => Promise<ThreadStateResult | null>;
  /** Tombstone the thread. `null` when the host refused it. */
  remove: (threadId: string) => Promise<ThreadStateResult | null>;
  /** The last refusal, in the user's words rather than the wire's. */
  error: string | null;
  clearError: () => void;
}

/**
 * @param onSettled Re-read whatever lists the fold moved a row between. Called
 * on success *and* on failure, for the reason in the module docs.
 */
export function useThreadActions(
  client: HostClient | null,
  onSettled?: () => void,
): ThreadActions {
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

  const archive = useCallback(
    async (threadId: string) => {
      if (!client) return null;
      try {
        const result = await client.archiveThread({ threadId });
        setError(null);
        return result;
      } catch (err: unknown) {
        setError(actionMessage(err, "already archived"));
        return null;
      } finally {
        onSettled?.();
      }
    },
    [client, onSettled],
  );

  const remove = useCallback(
    async (threadId: string) => {
      if (!client) return null;
      try {
        const result = await client.deleteThread({ threadId });
        setError(null);
        return result;
      } catch (err: unknown) {
        setError(actionMessage(err, "already gone"));
        return null;
      } finally {
        onSettled?.();
      }
    },
    [client, onSettled],
  );

  const clearError = useCallback(() => setError(null), []);

  return { fold, archive, remove, error, clearError };
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

/**
 * The same translation for the two destructive verbs.
 *
 * The only illegal transition either of them has is doing it twice, so
 * `already` says which of the two it was rather than naming a state machine
 * the user has never been shown.
 */
function actionMessage(err: unknown, already: string): string {
  if (err instanceof HostRpcError) {
    if (err.code === RPC_ERROR.ILLEGAL_TRANSITION) {
      return `That thread is ${already}.`;
    }
    if (err.code === RPC_ERROR.THREAD_NOT_FOUND) {
      return "That thread is gone.";
    }
    return err.message;
  }
  return err instanceof Error ? err.message : String(err);
}
