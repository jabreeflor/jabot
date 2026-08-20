//! The left rail. Crew on top, code below, you at the bottom.
//!
//! Search filters the code half only. Six faces are found by looking; a dozen
//! thread titles across four repos are not, and hiding a bot you were about to
//! click would be worse than useless.

import { useState } from "react";

import { BotStrip } from "./BotStrip";
import { FolderList } from "./FolderList";
import { initials } from "./format";
import { InboxIcon, NewChatIcon, PullRequestIcon, SearchIcon } from "./Icon";
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
  onSelectBot,
  onSelectThread,
  onOpenCrew,
  onOpenInbox,
  onOpenPullRequests,
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
  onSelectBot: (botId: string) => void;
  onSelectThread: (threadId: string) => void;
  onOpenCrew: () => void;
  onOpenInbox: () => void;
  onOpenPullRequests: () => void;
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
              ＋
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

        <FolderList
          folders={visibleFolders}
          selection={selection}
          leavingThreadIds={leavingThreadIds}
          onSelectThread={onSelectThread}
          onNewThread={onNewChat}
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
        <div className="av" aria-hidden="true">
          {initials(userName)}
        </div>
        <div className="who">
          <div className="name">{userName}</div>
          <div className={hostOffline ? "host bad" : "host"}>{hostLine}</div>
        </div>
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
