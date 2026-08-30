# Worktree manager

**Issue:** #23
**Status:** Implemented — `src-tauri/src/host/git/worktree.rs`, `src-tauri/src/host/git/setup.rs`

## What it is

A host-owned manager that gives each concurrent thread on a registered
git repo its own `git worktree`, so multiple bots/threads can work on the
same repo at once without clobbering each other's checked-out branch or
uncommitted changes.

## Why

Two threads sharing a single working directory means two agents racing
on the same files and index — one thread's `git checkout` breaking
another's in-flight edits. Per-thread worktrees make concurrency safe by
construction instead of by convention.

## Requirements

1. When a thread starts against a registered repo (see
   [folder-repo-registration.md](folder-repo-registration.md)) that
   already has another live thread, the host creates a **new git
   worktree** for the new thread rather than reusing the same checkout
   (`worktree.rs`).
2. Each worktree is created on its own branch (derived from the thread,
   e.g. a generated branch name) so commits from one thread don't land
   on a branch another thread is also using.
3. `setup.rs` handles first-time repo setup needed before worktrees can
   be created (e.g. ensuring the repo isn't bare/uninitialized, base
   branch exists).
4. Worktrees are cleaned up when their owning thread is archived — an
   archived thread does not leave an orphaned worktree directory behind
   indefinitely.
5. Worktree paths are host-managed (not user-chosen) and never collide
   across concurrently active threads on the same repo.
6. If worktree creation fails (e.g. dirty base checkout the host can't
   safely branch from), the thread start fails with a clear, actionable
   error rather than silently falling back to the shared checkout.
7. Worktrees integrate with the Pull Requests feature: a thread's PR
   (see [pull-requests.md](pull-requests.md)) is opened from its
   worktree's branch.
