//! Registered folders, live from the host (#16).
//!
//! `folder/list` already returns each folder with the threads under it, so this
//! is a rename from wire shape to prop shape and nothing more — no assembling
//! a join in the renderer, no second source of truth for what a folder is.
//!
//! `folders` stays `null` until the host has answered. That is not the same as
//! "no folders": a preview build, a unit test, or a host that has not started
//! yet all have no answer, and the shell keeps rendering its own fixtures until
//! one arrives. An empty array *is* an answer — a fresh install with nothing
//! registered — and the sidebar says so.

import { useCallback, useEffect, useState } from "react";

import type {
  FolderListResult,
  FolderRegisterParams,
  FolderThreadView,
  FolderView,
  HostClient,
  ThreadOverlayState,
  ThreadStateResult,
} from "../host";
import type {
  FolderWithThreads,
  ThreadState,
  ThreadSummary,
} from "../components/types";

export function folderRows(result: FolderListResult): FolderWithThreads[] {
  return result.folders.map(folderRow);
}

export function folderRow(folder: FolderView): FolderWithThreads {
  return {
    id: folder.folderId,
    name: folder.name,
    path: folder.path,
    cwd: folder.cwd,
    isGit: folder.isGit,
    repo: folder.origin?.repo,
    threads: folder.threads.map(threadRow),
  };
}

export function threadRow(thread: FolderThreadView): ThreadSummary {
  return {
    id: thread.threadId,
    folderId: thread.folderId ?? null,
    botId: thread.botId ?? null,
    harnessId: thread.harnessId,
    title: thread.title,
    state: sidebarState(thread.state),
    foldPolicy: thread.foldPolicy,
    runState: thread.runState ?? null,
    preview: thread.preview,
  };
}

/** Every thread the sidebar is currently showing, flat — what the main pane
    looks a selected thread up in. */
export function allThreads(
  folders: readonly FolderWithThreads[],
): ThreadSummary[] {
  return folders.flatMap((folder) => folder.threads);
}

export interface RegisteredFolders {
  /** `null` until the host answers; `[]` means nothing is registered yet. */
  folders: FolderWithThreads[] | null;
  /** Why the last load failed, for the row that would otherwise be blank. */
  error: string | null;
  reload: () => void;
  /** Resolves with the new folder, or throws the host's error — the modal
      needs the difference between "already registered" and "no such path". */
  register: (params: FolderRegisterParams) => Promise<FolderView>;
}

export function useFolders(client: HostClient | null): RegisteredFolders {
  const [folders, setFolders] = useState<FolderWithThreads[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped to re-run the load: the sidebar has to redraw after a registration
  // or a new thread, and both happen outside this hook.
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // The whole call is guarded, including the method lookup: a transport that
    // predates this method should leave the shell on its fixtures rather than
    // take the render down.
    (async () => client.listFolders())()
      .then((result) => {
        if (cancelled) return;
        setFolders(folderRows(result));
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const register = useCallback(
    async (params: FolderRegisterParams) => {
      if (!client) throw new Error("No host connection.");
      const folder = await client.registerFolder(params);
      reload();
      return folder;
    },
    [client, reload],
  );

  return { folders, error, reload, register };
}

/** `folder/list` returns only the two states the sidebar draws. Anything else
    would be a host bug; showing the row as active is the harmless reading. */
function sidebarState(state: ThreadOverlayState): ThreadState {
  return state === "resurfaced" ? "resurfaced" : "active";
}

/**
 * A thread the folder list does not know about, resolved from the host.
 *
 * The shell decides "is this the host's thread?" by flattening `folder/list`,
 * and `folder_list` only walks folder rows — so a thread whose `folder_id` is
 * null is invisible to it. When that rule was written no such thread existed.
 * One does now: `open_standing_thread` gives every bot a standing thread with
 * no folder, and a folded one surfaces in the Inbox like any other.
 *
 * Clicking that card used to land on "That thread is gone. Check the Inbox.",
 * and folding or archiving it dispatched to the mock reducer instead of the
 * host — so the row moved on screen while the permissions, runs and process
 * behind it were untouched.
 *
 * Resolved one id at a time rather than by listing every bot's thread up
 * front: the id in hand is the one the user is looking at, and asking about it
 * costs one call the moment it is needed instead of a call per bot on every
 * load.
 *
 * `null` while it resolves and for anything the host does not know, which
 * leaves every existing fixture path exactly as it was.
 */
export function useHostThread(
  client: HostClient | null,
  threadId: string | null,
  known: boolean,
): ThreadSummary | null {
  const [thread, setThread] = useState<ThreadSummary | null>(null);

  useEffect(() => {
    // Nothing to ask about, or the folder list already has it.
    if (!client || !threadId || known) {
      setThread(null);
      return;
    }
    if (typeof client.threadState !== "function") return;
    let cancelled = false;
    client
      .threadState({ threadId })
      .then((state) => {
        if (!cancelled) setThread(threadRowFromState(state));
      })
      .catch(() => {
        // A thread the host does not have is a real answer, not a failure:
        // it is a fixture row, or one that has been deleted. Staying null
        // keeps the existing behaviour for both.
      });
    return () => {
      cancelled = true;
    };
  }, [client, threadId, known]);

  return thread;
}

/** `thread/state` in the shape the sidebar and the thread view already read. */
function threadRowFromState(state: ThreadStateResult): ThreadSummary {
  return {
    id: state.threadId,
    folderId: state.folderId ?? null,
    botId: state.botId ?? null,
    harnessId: state.harnessId,
    title: state.title,
    state: sidebarState(state.state),
    foldPolicy: state.foldPolicy,
    runState: state.latestRun?.state ?? null,
  };
}
