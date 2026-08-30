//! Folders and their code threads.
//!
//! A folder is a registered repo (#16) and the container is literal: threads
//! live *inside* the folder card, because a code thread only means something
//! relative to its checkout. The ＋ in the header starts a thread already
//! pointed at that repo, which is the common case — the sidebar's New Chat row
//! is the one that has to ask.
//!
//! Folded threads are not listed at all. That is the promise fold makes: the
//! row goes away and comes back through the Inbox.
//!
//! A folder whose directory is not a git checkout is badged rather than hidden:
//! it runs threads perfectly well and only the PR view has nothing to say about
//! it (folders-and-auth.md).

import { useState } from "react";

import { ChevronDownIcon, DotIcon, FolderIcon, PlusIcon, RingIcon } from "./Icon";
import { threadStatus } from "./status";
import type { FolderWithThreads, Selection, ThreadSummary } from "./types";
import type { MenuPosition } from "./ThreadContextMenu";

export function FolderList({
  folders,
  selection,
  leavingThreadIds = [],
  onSelectThread,
  onNewThread,
  onThreadMenu,
}: {
  folders: readonly FolderWithThreads[];
  selection: Selection;
  /** Rows on their way out — animated, then dropped by the caller. */
  leavingThreadIds?: readonly string[];
  onSelectThread: (threadId: string) => void;
  onNewThread: (folderId: string) => void;
  onThreadMenu: (thread: ThreadSummary, position: MenuPosition) => void;
}) {
  const [collapsed, setCollapsed] = useState<readonly string[]>([]);
  const selectedThreadId =
    selection.view === "thread" ? selection.threadId : null;

  return (
    <>
      {folders.map((folder) => {
        const open = !collapsed.includes(folder.id);
        return (
          <div className="folder" key={folder.id}>
            <div className="folder-head">
              <button
                type="button"
                className="folder-toggle"
                aria-expanded={open}
                // The registered directory, and the repo it turned out to be —
                // the two things a folder row cannot show but a user picking
                // between two checkouts of the same project needs.
                title={folder.repo ? `${folder.path} · ${folder.repo}` : folder.path}
                onClick={() =>
                  setCollapsed((current) =>
                    open
                      ? [...current, folder.id]
                      : current.filter((id) => id !== folder.id),
                  )
                }
              >
                <ChevronDownIcon className="chev" />
                {/* The glyph carries the same state as the chevron: an open
                    folder for an expanded one. */}
                <FolderIcon open={open} />
                <span className="name">{folder.name}</span>
                {/* Only when the host has actually looked: `undefined` is "not
                    asked yet", and a badge for that would be a lie. */}
                {folder.isGit === false && (
                  <span className="folder-badge" title="Not a git repo — threads run here, pull requests do not">
                    no git
                  </span>
                )}
                {!open && <span className="count">{folder.threads.length}</span>}
              </button>
              <button
                type="button"
                className="folder-add"
                title={`New thread in ${folder.name}`}
                aria-label={`New thread in ${folder.name}`}
                onClick={() => onNewThread(folder.id)}
              >
                <PlusIcon />
              </button>
            </div>
            {open &&
              folder.threads.map((thread) => (
                <ThreadRow
                  key={thread.id}
                  thread={thread}
                  selected={selectedThreadId === thread.id}
                  leaving={leavingThreadIds.includes(thread.id)}
                  onSelect={onSelectThread}
                  onMenu={onThreadMenu}
                />
              ))}
          </div>
        );
      })}
    </>
  );
}

function ThreadRow({
  thread,
  selected,
  leaving,
  onSelect,
  onMenu,
}: {
  thread: ThreadSummary;
  selected: boolean;
  leaving: boolean;
  onSelect: (threadId: string) => void;
  onMenu: (thread: ThreadSummary, position: MenuPosition) => void;
}) {
  const status = threadStatus(thread);
  return (
    <button
      type="button"
      className={leaving ? "thread-row leaving" : "thread-row"}
      aria-current={selected}
      onClick={() => onSelect(thread.id)}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(thread, { x: event.clientX, y: event.clientY });
      }}
    >
      <span className={`pip ${status.tone}`} aria-hidden="true">
        {status.tone === "quiet" ? <RingIcon /> : <DotIcon />}
      </span>
      <span className="title">{thread.title}</span>
      <span className="state">{status.label}</span>
    </button>
  );
}
