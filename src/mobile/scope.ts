//! What a phone may call, mirrored from the host so the mismatch is visible.
//!
//! `src-tauri/src/host/pairing/scope.rs` is the enforcement. It runs in
//! `router::handle` on every request, reads the role off the `paired_devices`
//! row rather than off the wire, and is an allowlist — so a method added next
//! week is closed to an approver until somebody opens it. None of that is
//! weakened by anything in this file, and nothing here is a security boundary.
//!
//! This list exists for two smaller reasons:
//!
//! **A button that always fails is worse than no button.** The phone decides
//! what to render before it makes a call, so it needs the answer locally.
//!
//! **Drift has to be loud.** `host/hello` now answers with `scopedMethods` —
//! the host's own view of what *this* device may call. [`checkScope`] compares
//! the two, so a role narrowed on the Rust side shows up as a failing test and
//! a refused call rather than as a phone that quietly still draws Fold.

import {
  HOST_HEALTH,
  HOST_HELLO,
  INBOX_LIST,
  PERMISSION_PENDING,
  PERMISSION_REPLY,
  SESSION_CANCEL,
  SYNC_RESUME_FROM,
  THREAD_STATE,
  THREAD_TRANSCRIPT,
} from "../host/protocol";

/**
 * Everything a phone is for: see what needs you, read enough of the thread to
 * know what you are answering, answer it, and stop a turn you do not like.
 *
 * Mirrors `APPROVER_METHODS` in `host/pairing/scope.rs`, in the same order.
 */
export const APPROVER_METHODS: readonly string[] = [
  HOST_HELLO,
  HOST_HEALTH,
  INBOX_LIST,
  PERMISSION_PENDING,
  PERMISSION_REPLY,
  THREAD_STATE,
  THREAD_TRANSCRIPT,
  SESSION_CANCEL,
  SYNC_RESUME_FROM,
];

export function allowedForApprover(method: string): boolean {
  return APPROVER_METHODS.includes(method);
}

/**
 * Where this client and the host disagree about what the device may call.
 *
 * Returns the methods the host allows and this file does not, and the methods
 * this file offers and the host would refuse. Both directions matter: the
 * first is a feature the phone is not drawing, the second is a button that
 * would throw `DEVICE_SCOPE` when pressed.
 */
export function checkScope(hostScopedMethods: readonly string[]): {
  missingHere: string[];
  refusedThere: string[];
} {
  const host = new Set(hostScopedMethods);
  return {
    missingHere: hostScopedMethods.filter((m) => !APPROVER_METHODS.includes(m)),
    refusedThere: APPROVER_METHODS.filter((m) => !host.has(m)),
  };
}
