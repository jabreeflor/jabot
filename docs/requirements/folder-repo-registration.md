# Folder/repo registration

**Issue:** #16
**Status:** Implemented — `src-tauri/src/host/repo/`, `src/components/AddFolderModal.tsx`, `src/components/FolderList.tsx`, `src/views/folders.ts`

## What it is

Letting a user register a local folder (typically a git repo) as a
working directory JaBot threads and bots can operate in, and surfacing
those folders in the Sidebar.

## Why

Every thread and every crew bot needs a `cwd` to run its harness in.
Folder registration is how that `cwd` gets chosen and remembered, and how
JaBot learns a folder is a git repo it can also branch/worktree/open PRs
against (see [worktree-manager.md](worktree-manager.md) and
[pull-requests.md](pull-requests.md)).

## Requirements

1. A user can add a folder via `AddFolderModal.tsx`; the host validates
   the path exists and is readable before registering it
   (`src-tauri/src/host/repo/mod.rs`).
2. Registered folders persist across restarts (stored via the data layer,
   see [data-layer-persistence.md](data-layer-persistence.md)) and are
   listed in the Sidebar via `FolderList.tsx`.
3. If a registered folder is a git repository, JaBot detects it
   (`src-tauri/src/host/repo/git.rs`) and records its origin
   (`origin.rs`) so PR linkage and worktrees can key off it.
4. `gh.rs` / `exec.rs` provide the host-side git/`gh` command execution
   used by folder detection and later by the worktree manager and PR
   view — command execution is centralized here rather than shelled out
   ad hoc from other modules.
5. Removing a registered folder does not delete the folder on disk; it
   only removes it from JaBot's list and detaches any threads that used
   it as their `cwd` is prevented or requires explicit confirmation
   (folders in active use by a non-archived thread are not silently
   unregistered).
6. New Chat's folder/harness picker only offers folders that are
   currently registered and still present on disk.
