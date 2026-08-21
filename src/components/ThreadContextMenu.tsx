//! Right-click on a thread row.
//!
//! Neither fold item is a delete — the thread keeps running, it just stops
//! taking up sidebar space until it has something to say (#5). They differ only
//! in the policy they leave behind: "Disappear until done" keeps whatever the
//! thread already had, and "Wait for Inbox" is the quieter one that lets the
//! host answer reads while nobody is watching (#26). Both are hidden for a row
//! that cannot be folded, because the transition table refuses to re-sleep a
//! thread that has already come back to you.
//!
//! The menu names the thread so a mis-aimed right-click is obvious before the
//! destructive item is clicked.

import { useEffect, useRef } from "react";

import { canFold } from "./FoldButton";
import { ArchiveIcon, InboxIcon, TrashIcon } from "./Icon";
import type { FoldPolicy, ThreadState } from "./types";

/** Roughly the menu's own size; keeps it on screen near the window edges. */
const MENU_W = 195;
const MENU_H = 190;

export interface MenuPosition {
  x: number;
  y: number;
}

export function ThreadContextMenu({
  threadTitle,
  threadState,
  position,
  onFold,
  onArchive,
  onDelete,
  onClose,
}: {
  threadTitle: string;
  /** Decides whether the fold items are offered at all. */
  threadState: ThreadState;
  position: MenuPosition;
  /** `undefined` policy is "Disappear until done" — keep the current one. */
  onFold: (policy?: FoldPolicy) => void;
  onArchive: () => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    menuRef.current?.querySelector("button")?.focus();
  }, []);

  useEffect(() => {
    function onPointerDown(event: MouseEvent) {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  return (
    <div
      className="ctx-menu"
      role="menu"
      aria-label={threadTitle}
      ref={menuRef}
      style={{
        left: Math.min(position.x, window.innerWidth - MENU_W),
        top: Math.min(position.y, window.innerHeight - MENU_H),
      }}
    >
      {canFold(threadState) && (
        <>
          <button type="button" role="menuitem" onClick={() => onFold()}>
            <InboxIcon /> Disappear until done
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => onFold("wait_for_inbox")}
          >
            <InboxIcon /> Wait for Inbox
          </button>
        </>
      )}
      <button type="button" role="menuitem" onClick={onArchive}>
        <ArchiveIcon /> Archive
      </button>
      <div className="sep" />
      <button type="button" role="menuitem" className="danger" onClick={onDelete}>
        <TrashIcon /> Delete
      </button>
    </div>
  );
}
