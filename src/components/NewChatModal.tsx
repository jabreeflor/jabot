//! New Chat: the card that spawns a code thread.
//!
//! Harness is picked *per thread*, not per bot — Code owns many folder threads
//! and any one of them may run a different engine than Code's default (#6). The
//! folder is the repo the thread will work in; "No folder" is a scratch session
//! with no worktree.
//!
//! A folder thread always gets its own worktree. The Advanced disclosure that
//! offered the checkout opt-out and a base branch (#23, #92) is gone: a fresh
//! worktree per thread is what stops two threads in one repo standing on each
//! other's uncommitted work, and a card that shows the way out of that is a
//! card that invites somebody to take it. `thread/open` still accepts
//! `useCheckout` and `baseRef` — the Rust host honours both and
//! `tests/e2e/worktree.test.ts` drives them — but nothing the card sends sets
//! either, so the base ref is the host's default and the tree is always fresh.

import { useState } from "react";

import { FieldLabel, Modal } from "./Modal";
import { HarnessPicker } from "./HarnessPicker";
import { WorkspacePicker, type WorkspaceActions } from "./WorkspacePicker";
import type { Folder, HarnessCard, NewChatDraft } from "./types";

export function NewChatModal({
  harnesses,
  folders,
  defaultFolderId = null,
  defaultHarnessId,
  error = null,
  workspaceActions,
  onStart,
  onCancel,
}: {
  harnesses: readonly HarnessCard[];
  folders: readonly Folder[];
  defaultFolderId?: string | null;
  defaultHarnessId?: string;
  /** Why the last attempt did not start a session. The card stays open holding
      the draft, because a refused spawn is something to fix and retry. */
  error?: string | null;
  workspaceActions?: WorkspaceActions;
  onStart: (draft: NewChatDraft) => void | Promise<void>;
  onCancel: () => void;
}) {
  const [harnessId, setHarnessId] = useState(
    defaultHarnessId ?? harnesses[0]?.id ?? "",
  );
  const [folder, setFolder] = useState(defaultFolderId ?? "");
  const [busy, setBusy] = useState(false);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setWorkspaceError(null);
    try {
      await action();
    } catch (err) {
      setWorkspaceError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      title="New Chat"
      onClose={() => {
        if (!busy) onCancel();
      }}
    >
      <FieldLabel>HARNESS — BRING YOUR OWN</FieldLabel>
      <HarnessPicker
        harnesses={harnesses}
        value={harnessId}
        onChange={setHarnessId}
        label="Harness"
      />

      <WorkspacePicker
        folders={folders}
        value={folder}
        onChange={setFolder}
        actions={workspaceActions}
        busy={busy}
        run={run}
      />
      {busy && (
        <p className="workspace-hint" role="status">
          Preparing your workspace…
        </p>
      )}
      <p className="workspace-hint">
        Your selected harness opens a new session. Add your first message when
        you’re ready.
      </p>

      {(workspaceError || error) && (
        <p className="modal-error" role="alert">
          {workspaceError || error}
        </p>
      )}

      <div className="macts">
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={onCancel}
        >
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={busy || !harnessId}
          onClick={() =>
            void run(async () => {
              await onStart({
                harnessId,
                folderId: folder || null,
                task: "Untitled session",
              });
            })
          }
        >
          Start session
        </button>
      </div>
    </Modal>
  );
}
