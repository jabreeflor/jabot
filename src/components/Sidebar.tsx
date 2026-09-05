//! The left rail. Crew on top, code below, you at the bottom.
//!
//! Search filters the code half only. Six faces are found by looking; a dozen
//! thread titles across four repos are not, and hiding a bot you were about to
//! click would be worse than useless.

import { useState } from "react";

import { BotStrip } from "./BotStrip";
import { FolderList } from "./FolderList";
import { initials } from "./format";
import {
  ClockIcon,
  DeviceIcon,
  InboxIcon,
  NewChatIcon,
  PlusIcon,
  PullRequestIcon,
  SearchIcon,
  GearIcon,
} from "./Icon";
import type { MenuPosition } from "./ThreadContextMenu";
import type {
  Bot,
  FolderWithThreads,
  Selection,
  ThreadSummary,
} from "./types";

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
  onAddFolder,
  onFolderSettings,
  onSelectBot,
  onSelectThread,
  onOpenCrew,
  onOpenInbox,
  onOpenPullRequests,
  onOpenSchedules,
  onOpenDevices,
  onOpenSettings,
  onNewChat,
  onThreadMenu,
}: {
  bots: readonly Bot[];
  folders: readonly FolderWithThreads[];
  selection: Selection;
  inboxCount: number;
  openPrCount: number;
  userName: string;
  /** One line under the name: which host, or why there isn't one. */
  hostLine: string;
  hostOffline?: boolean;
  leavingThreadIds?: readonly string[];
  /** The host answered, and it has no folders yet — not the same as a host
      that has not answered, which keeps whatever is already on screen. */
  foldersEmpty?: boolean;
  /** Absent until the host can register one, which is what makes the ＋ in the
      CODE header appear at all. */
  onAddFolder?: () => void;
  /** Open a registered folder's settings (#16). Absent before a host has
      answered — a fixture folder has nothing the host could update. */
  onFolderSettings?: (folderId: string) => void;
  onSelectBot: (botId: string) => void;
  onSelectThread: (threadId: string) => void;
  onOpenCrew: () => void;
  onOpenInbox: () => void;
  onOpenPullRequests: () => void;
  onOpenSchedules: () => void;
  /** Paired devices (#19, #29). Absent before a host has answered, for the
      same reason as Settings: a preview build has nothing paired to it. */
  onOpenDevices?: () => void;
  /** App-wide preferences (#26). Absent before a host has answered: a preview
      build has nothing to set. */
  onOpenSettings?: () => void;
  /** null = ask which folder; a folder id = start there. */
  onNewChat: (folderId: string | null) => void;
  onThreadMenu: (thread: ThreadSummary, position: MenuPosition) => void;
}) {
  const [query, setQuery] = useState("");
  const visibleFolders = filterFolders(folders, query);

  return (
    <aside className="sidebar">
      <div className="sidebar-search">
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
      </div>

      <div className="sidebar-list">
        <div className="section-header">BOT CHATS</div>
        <BotStrip
          bots={bots}
          selection={selection}
          onSelectBot={onSelectBot}
          onOpenCrew={onOpenCrew}
        />

        <div className="sidebar-divider" />
        <div className="section-header">
          CODE
          {onAddFolder && (
            <button
              type="button"
              className="folder-add section-add"
              title="Add folder"
              aria-label="Add folder"
              onClick={onAddFolder}
            >
              <PlusIcon />
            </button>
          )}
        </div>

        <button
          type="button"
          className="nav-row"
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
            openPrCount > 0 ? `Pull Requests — ${openPrCount} open` : undefined
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

        {/* Only with a host, for the same reason as Settings below: what this
            lists is what the *host* is paired to, and a preview build is
            paired to nothing. */}
        {onOpenDevices && (
          <button
            type="button"
            className="nav-row"
            aria-current={selection.view === "devices"}
            onClick={onOpenDevices}
          >
            <span className="ic">
              <DeviceIcon />
            </span>
            Devices
          </button>
        )}

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

      <div className="me-row">
        <div className="me-face" aria-hidden="true">
          {initials(userName)}
        </div>
        <div className="who">
          <div className="name">{userName}</div>
          <div className={hostOffline ? "host bad" : "host"}>{hostLine}</div>
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
    </aside>
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
