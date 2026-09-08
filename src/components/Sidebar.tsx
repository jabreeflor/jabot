//! The left rail. Crew on top, code below, you at the bottom.
//!
//! Search filters the code half only. Six faces are found by looking; a dozen
//! thread titles across four repos are not, and hiding a bot you were about to
//! click would be worse than useless.
//!
//! The rail can close. A closed sidebar is a strip wide enough for the traffic
//! lights and the toggle that opens it again — not gone, because overlay
//! chrome still has to sit on something, and because a control that vanished
//! with the thing it reveals is a trap. After that toggle is focused or
//! hovered, moving into the rail's full width peeks the list open as a
//! flyout; the pin (`open`, localStorage, ⌘B) does not change.

import { useEffect, useRef, useState } from "react";

import { BotStrip } from "./BotStrip";
import { FolderList } from "./FolderList";
import { initials } from "./format";
import {
  ClockIcon,
  InboxIcon,
  NewChatIcon,
  PullRequestIcon,
  SearchIcon,
  GearIcon,
  SidebarIcon,
} from "./Icon";
import type { MenuPosition } from "./ThreadContextMenu";
import type { Bot, FolderWithThreads, Selection, ThreadSummary } from "./types";

/** Matches `--sidebar-w`. jsdom has no stylesheet, so the peek hit-region
    falls back to this rather than treating a missing variable as zero. */
const SIDEBAR_PEEK_W = 310;

function peekWidthPx(node: HTMLElement): number {
  const raw = getComputedStyle(node).getPropertyValue("--sidebar-w").trim();
  const px = Number.parseFloat(raw);
  return Number.isFinite(px) && px > 0 ? px : SIDEBAR_PEEK_W;
}

/** The full rail, not the collapsed strip: after the toggle is armed, a
    pointer in this rectangle slides the list open. jsdom reports a zero
    rect, so a missing height is treated as unbounded rather than as
    "the pointer is never inside". */
function pointerInSidebarRegion(
  event: { clientX: number; clientY: number },
  rail: HTMLElement,
  width: number,
): boolean {
  const rect = rail.getBoundingClientRect();
  const bottom = rect.height > 0 ? rect.bottom : Number.POSITIVE_INFINITY;
  return (
    event.clientX >= rect.left &&
    event.clientX < rect.left + width &&
    event.clientY >= rect.top &&
    event.clientY <= bottom
  );
}

export function Sidebar({
  bots,
  folders,
  selection,
  inboxCount,
  openPrCount,
  userName,
  hostLine,
  hostOffline = false,
  leavingThreadIds,
  foldersEmpty = false,
  onFolderSettings,
  onSelectBot,
  onSelectThread,
  onOpenCrew,
  onOpenInbox,
  onOpenPullRequests,
  onOpenSchedules,
  onOpenSettings,
  onReconnect,
  onNewChat,
  onThreadMenu,
  open = true,
  onToggle,
}: {
  bots: readonly Bot[];
  folders: readonly FolderWithThreads[];
  selection: Selection;
  inboxCount: number;
  openPrCount: number;
  userName: string;
  /** Transient connection status. Empty once the host is healthy. */
  hostLine: string;
  hostOffline?: boolean;
  leavingThreadIds?: readonly string[];
  /** The host answered, and it has no folders yet — not the same as a host
      that has not answered, which keeps whatever is already on screen. */
  foldersEmpty?: boolean;
  /** Open a registered folder's settings (#16). Absent before a host has
      answered — a fixture folder has nothing the host could update. */
  onFolderSettings?: (folderId: string) => void;
  onSelectBot: (botId: string) => void;
  onSelectThread: (threadId: string) => void;
  onOpenCrew: () => void;
  onOpenInbox: () => void;
  onOpenPullRequests: () => void;
  onOpenSchedules: () => void;
  /** Retry the host handshake after a mid-session disconnect. */
  onReconnect?: () => void;
  /** App-wide preferences and paired devices (#26, #19, #29). Absent before a
      host has answered: a preview build has nothing to set or revoke. */
  onOpenSettings?: () => void;
  /** null = ask which folder; a folder id = start there. */
  onNewChat: (folderId: string | null) => void;
  onThreadMenu: (thread: ThreadSummary, position: MenuPosition) => void;
  /** Whether the rail is showing its list. Default open: a closed sidebar
      on first launch would hide the only way to pick a conversation. */
  open?: boolean;
  /** Hide or show the list. Absent in a story that is not the shell — the
      fixture tests still render a rail, they just do not fold it. */
  onToggle?: () => void;
}) {
  const [query, setQuery] = useState("");
  const visibleFolders = filterFolders(folders, query);
  const railRef = useRef<HTMLElement>(null);
  // After a hide click the pointer is still on the toggle. Peeking from
  // that, or from the leave/enter the layout shift synthesizes, would
  // undo the click. Hold until the pointer actually moves.
  const holdPeekAt = useRef<{ x: number; y: number } | null>(null);
  const [armed, setArmed] = useState(false);
  const [peeked, setPeeked] = useState(false);
  // Peek is a flyout, not a pin. `open` is what localStorage and ⌘B own;
  // this only mounts the list over the chat until the pointer leaves.
  const shown = open || peeked;

  useEffect(() => {
    if (!open) return;
    setPeeked(false);
    setArmed(false);
    holdPeekAt.current = null;
  }, [open]);

  function releasedFromHold(event: {
    clientX: number;
    clientY: number;
  }): boolean {
    const hold = holdPeekAt.current;
    if (!hold) return true;
    if (Math.hypot(event.clientX - hold.x, event.clientY - hold.y) < 12) {
      return false;
    }
    holdPeekAt.current = null;
    return true;
  }

  useEffect(() => {
    if (open || !armed) return;

    function onPointerMove(event: PointerEvent) {
      if (!releasedFromHold(event)) return;
      const rail = railRef.current;
      if (!rail) return;
      setPeeked(pointerInSidebarRegion(event, rail, peekWidthPx(rail)));
    }

    window.addEventListener("pointermove", onPointerMove);
    return () => window.removeEventListener("pointermove", onPointerMove);
  }, [open, armed]);

  function armFromAffordance() {
    if (!open) setArmed(true);
  }

  function handleToggle(event: { clientX: number; clientY: number }) {
    if (open) {
      holdPeekAt.current = { x: event.clientX, y: event.clientY };
      setArmed(true);
      setPeeked(false);
    } else {
      setPeeked(false);
      setArmed(false);
      holdPeekAt.current = null;
    }
    onToggle?.();
  }

  const railClass = [
    "sidebar",
    !open ? "is-collapsed" : "",
    peeked ? "is-peeking" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={open ? "sidebar-slot" : "sidebar-slot is-collapsed"}>
      <aside
        ref={railRef}
        className={railClass}
        onPointerLeave={() => {
          if (open) return;
          setPeeked(false);
          const active = document.activeElement;
          if (railRef.current?.contains(active)) return;
          setArmed(false);
        }}
      >
        <div className="sidebar-search">
          {onToggle && (
            <button
              type="button"
              className="sidebar-toggle"
              aria-expanded={open}
              aria-label={open ? "Hide sidebar" : "Show sidebar"}
              title={open ? "Hide sidebar" : "Show sidebar"}
              onClick={handleToggle}
              onFocus={armFromAffordance}
              onPointerEnter={(event) => {
                if (open || !releasedFromHold(event)) return;
                setArmed(true);
                setPeeked(true);
              }}
            >
              <SidebarIcon />
            </button>
          )}
          {shown && (
            <div className="field">
              <SearchIcon />
              <input
                type="search"
                value={query}
                placeholder="Search"
                aria-label="Search threads"
                onChange={(event) => setQuery(event.target.value)}
              />
            </div>
          )}
        </div>

        {shown && (
          <div className="sidebar-list">
            <div className="section-header">BOT CHATS</div>
            <BotStrip
              bots={bots}
              selection={selection}
              onSelectBot={onSelectBot}
              onOpenCrew={onOpenCrew}
            />

            <div className="section-header">CODE</div>

            <button
              type="button"
              className="nav-row"
              aria-current={selection.view === "new-chat"}
              onClick={() => onNewChat(null)}
            >
              <span className="ic">
                <NewChatIcon />
              </span>
              New Chat
            </button>

            {/* The counts are folded into the label rather than left as loose
            numerals, so "Inbox — 2 waiting" is what gets announced. */}
            <button
              type="button"
              className="nav-row"
              aria-current={selection.view === "prs"}
              aria-label={
                openPrCount > 0
                  ? `Pull Requests — ${openPrCount} open`
                  : undefined
              }
              onClick={onOpenPullRequests}
            >
              <span className="ic">
                <PullRequestIcon />
              </span>
              Pull Requests
              {openPrCount > 0 && (
                <span className="count" aria-hidden="true">
                  {openPrCount}
                </span>
              )}
            </button>

            <button
              type="button"
              className="nav-row"
              aria-current={selection.view === "inbox"}
              aria-label={
                inboxCount > 0 ? `Inbox — ${inboxCount} waiting` : undefined
              }
              onClick={onOpenInbox}
            >
              <span className="ic">
                <InboxIcon />
              </span>
              Inbox
              {inboxCount > 0 && (
                <span className="badge" aria-hidden="true">
                  {inboxCount}
                </span>
              )}
            </button>

            {/* Under the Inbox on purpose: a schedule's whole output *is* an
            Inbox card, so the two belong next to each other. */}
            <button
              type="button"
              className="nav-row"
              aria-current={selection.view === "schedules"}
              onClick={onOpenSchedules}
            >
              <span className="ic">
                <ClockIcon />
              </span>
              Schedules
            </button>

            <FolderList
              folders={visibleFolders}
              selection={selection}
              leavingThreadIds={leavingThreadIds}
              onSelectThread={onSelectThread}
              onNewThread={onNewChat}
              onFolderSettings={onFolderSettings}
              onThreadMenu={onThreadMenu}
            />
            {query && visibleFolders.length === 0 && (
              <div className="page-empty">No threads match “{query}”.</div>
            )}
            {!query && foldersEmpty && (
              <div className="page-empty">
                No folders yet. Add one to start a code thread in it.
              </div>
            )}
          </div>
        )}

        {shown && (
          <div className="me-row">
            <div className="me-face" aria-hidden="true">
              {initials(userName)}
            </div>
            <div className="who">
              <div className="name">{userName}</div>
              {hostLine && (
                <div className={hostOffline ? "host bad" : "host"}>
                  <span>{hostLine}</span>
                  {hostOffline && onReconnect && (
                    <button
                      type="button"
                      className="host-reconnect"
                      onClick={onReconnect}
                    >
                      Reconnect
                    </button>
                  )}
                </div>
              )}
            </div>
            {onOpenSettings && (
              <button
                type="button"
                className="me-settings"
                aria-label="Settings"
                title="Settings"
                aria-current={selection.view === "settings"}
                onClick={onOpenSettings}
              >
                <GearIcon />
              </button>
            )}
          </div>
        )}
      </aside>
    </div>
  );
}

/** A folder survives if it matches, or if any of its threads do. */
function filterFolders(
  folders: readonly FolderWithThreads[],
  query: string,
): FolderWithThreads[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...folders];

  const matches: FolderWithThreads[] = [];
  for (const folder of folders) {
    if (folder.name.toLowerCase().includes(needle)) {
      matches.push(folder);
      continue;
    }
    const threads = folder.threads.filter((thread) =>
      thread.title.toLowerCase().includes(needle),
    );
    if (threads.length > 0) matches.push({ ...folder, threads });
  }
  return matches;
}

/**
 * The shell's memory of whether the rail is open. Only an explicit "0" hides
 * it: a missing key, private mode, or garbage must not launch someone into a
 * window with no navigation.
 */
export const SIDEBAR_OPEN_KEY = "jabot.sidebarOpen";

export function loadSidebarOpen(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_OPEN_KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveSidebarOpen(open: boolean): void {
  try {
    window.localStorage.setItem(SIDEBAR_OPEN_KEY, open ? "1" : "0");
  } catch {
    // Quota / private mode: the session still toggles; the next launch will
    // open, which is the same default as a first run.
  }
}
