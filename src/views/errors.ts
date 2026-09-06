//! What a refused host call should say to a person.
//!
//! Most errors are already a sentence and the view shows `err.message`. One is
//! not: `HARNESS_UNAVAILABLE` names a binary and stops
//! (`Harness unavailable: claude-agent-acp`), which tells a user what is
//! missing and nothing about how to get it. The remedy exists — the catalog
//! carries an install hint per card and the host puts it in the error's `data`
//! (`protocol/error.rs`) — it was simply never read on this side, so the one
//! line that would unblock someone was computed and dropped.

import { HostRpcError, RPC_ERROR } from "../host";

/**
 * The install hint the host attached to an unavailable-harness error.
 *
 * Read defensively: `data` is `unknown` on purpose, and a host that answered
 * without a hint (a custom harness whose file gave none) still has a message
 * worth showing.
 */
function installHint(err: HostRpcError): string | null {
  if (err.code !== RPC_ERROR.HARNESS_UNAVAILABLE) return null;
  if (typeof err.data !== "object" || err.data === null) return null;
  const hint = (err.data as { installHint?: unknown }).installHint;
  return typeof hint === "string" && hint.trim() !== "" ? hint.trim() : null;
}

/**
 * One error, as much of it as is useful.
 *
 * The hint is appended rather than replacing the message because the two say
 * different things: which command is missing, and what to install to get it.
 * A user who has already installed one adapter needs the first half to know
 * the app is looking for the other name.
 */
export function hostErrorText(err: unknown): string {
  if (err instanceof HostRpcError) {
    const hint = installHint(err);
    return hint ? `${err.message} — ${hint}` : err.message;
  }
  return err instanceof Error ? err.message : String(err);
}
