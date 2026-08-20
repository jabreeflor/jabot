/**
 * Right-click on a thread row.
 *
 * "Wait for Inbox" is the fold policy from #5, not a delete — the thread keeps
 * running, it just stops taking up sidebar space until it has something to say.
 * The menu names the thread so a mis-aimed right-click is obvious before the
 * destructive item is clicked.
 */

import { useEffect, useRef } from "react";

import { ArchiveIcon, InboxIcon, TrashIcon } from "./Icon";

/** Roughly the menu's own size; keeps it on screen near the window edges. */
const MENU_W = 195;
const MENU_H = 150;

export interface MenuPosition {
  x: number;
  y: number;
}

export function ThreadContextMenu({
  threadTitle,
  position,
  onWaitForInbox,
  onArchive,
  onDelete,
  onClose,
}: {
  threadTitle: string;
  position: MenuPosition;
  onWaitForInbox: () => void;
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
      <button type="button" role="menuitem" onClick={onWaitForInbox}>
        <InboxIcon /> Wait for Inbox
      </button>
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
