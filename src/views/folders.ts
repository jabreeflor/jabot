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
