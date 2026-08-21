//! The client's copy of the approver allowlist, and how drift is caught.

import { describe, expect, it } from "vitest";

import {
  INBOX_LIST,
  PERMISSION_REPLY,
  SESSION_PROMPT,
  THREAD_DELETE,
  TOOLS_CONNECT,
  PAIRING_START,
  DEVICE_REVOKE,
} from "../../host/protocol";
import { allowedForApprover, APPROVER_METHODS, checkScope } from "../scope";

describe("what a phone may call", () => {
  it("covers reading the Inbox and answering an ask", () => {
    expect(allowedForApprover(INBOX_LIST)).toBe(true);
    expect(allowedForApprover(PERMISSION_REPLY)).toBe(true);
  });

  /// The list from `pairing-security-mobile.md`, one assertion each. A phone
  /// that can do any of these is the admin console.
  it("covers nothing that administers the host", () => {
    for (const method of [
      SESSION_PROMPT,
      THREAD_DELETE,
      TOOLS_CONNECT,
      PAIRING_START,
      DEVICE_REVOKE,
    ]) {
      expect(allowedForApprover(method)).toBe(false);
    }
  });

  it("is closed to a method nobody has scoped yet", () => {
    // Same property as the Rust allowlist: a host method added next week is
    // refused here until somebody decides otherwise.
    expect(allowedForApprover("some/methodAddedLater")).toBe(false);
  });

  it("reports drift in both directions", () => {
    expect(checkScope(APPROVER_METHODS)).toEqual({
      missingHere: [],
      refusedThere: [],
    });
    // The host opened something this client does not draw.
    expect(checkScope([...APPROVER_METHODS, "thread/fold"]).missingHere).toEqual([
      "thread/fold",
    ]);
    // The host took something away that this client still offers — the case
    // that turns into a button which always fails.
    expect(
      checkScope(APPROVER_METHODS.filter((m) => m !== PERMISSION_REPLY))
        .refusedThere,
    ).toEqual([PERMISSION_REPLY]);
  });
});
