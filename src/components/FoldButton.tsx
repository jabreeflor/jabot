//! "Disappear until done" in the thread header (#26).
//!
//! Two items, not one, because fold carries a choice the user cannot make
//! anywhere else. Both hide the row and leave the agent working; they differ
//! only in what the host is allowed to answer while nobody is watching
//! (`state-machine.md`, "Right-click actions"):
//!
//! - **Disappear until done** keeps whatever policy the thread already has.
//!   That is the settled behaviour, not an oversight — it is the plain
//!   hide-and-keep-working gesture, and it must not quietly undo a quieter
//!   policy the user picked earlier.
//! - **Wait for Inbox** sets `fold_policy = wait_for_inbox`, which lets the
//!   host answer *reads* on their behalf. Never an execute, never a delete;
//!   the subtitles say so, because a permission policy the user cannot read is
//!   one they cannot consent to.
//!
//! The button is only rendered for a thread that can actually be folded. A
//! resurfaced thread is already back in front of you, and the transition table
//! refuses to re-sleep it — offering the gesture anyway would be an error
//! message where an affordance should have been.

import { useEffect, useId, useRef, useState } from "react";

import { InboxIcon } from "./Icon";
import type { FoldPolicy, ThreadState } from "./types";

/** Fold is legal from `active` only; everything else has to be reopened. */
export function canFold(state: ThreadState): boolean {
  return state === "active";
}

export function FoldButton({
  onFold,
  busy = false,
}: {
  /** `undefined` policy is "Disappear until done" — keep the current one. */
  onFold: (policy?: FoldPolicy) => void;
  busy?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  function choose(policy?: FoldPolicy) {
    setOpen(false);
    onFold(policy);
  }

  return (
    <div className="fold-control" ref={rootRef}>
      <button
        type="button"
        className="fold-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        disabled={busy}
        onClick={() => setOpen((was) => !was)}
      >
        <InboxIcon /> Fold
      </button>
      {open && (
        <div className="fold-menu" id={menuId} role="menu" aria-label="Fold">
          <button type="button" role="menuitem" onClick={() => choose()}>
            <span>Disappear until done</span>
            <small>
              Keeps working. Comes back to the Inbox when it finishes, fails, or
              needs you.
            </small>
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => choose("wait_for_inbox")}
          >
            <span>Wait for Inbox</span>
            <small>
              Quieter: reads are allowed while you are away. Never an execute or
              a delete.
            </small>
          </button>
        </div>
      )}
    </div>
  );
}
