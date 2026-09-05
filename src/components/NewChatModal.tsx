//! New Chat: the card that spawns a code thread.
//!
//! Harness is picked *per thread*, not per bot — Code owns many folder threads
//! and any one of them may run a different engine than Code's default (#6). The
//! folder is the repo the thread will work in; "No folder" is a scratch session
//! with no worktree.
//!
//! The two worktree controls (#23) are behind an Advanced disclosure and shut
//! by default. `thread/open` has accepted `useCheckout` and `baseRef` since
//! #23 and nothing ever set them, so the opt-out and the base branch were
//! reachable only by a caller writing JSON-RPC by hand. They are advanced on
//! purpose and stay that way: a fresh worktree per thread is what stops two
//! threads in one repo standing on each other's uncommitted work, and the card
//! should not invite anybody to turn it off casually.

import { useId, useState } from "react";

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
  const folderId = useId();
  const checkoutId = useId();
  const baseRefId = useId();
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
  const [advanced, setAdvanced] = useState(false);
  const [useCheckout, setUseCheckout] = useState(false);
  const [baseRef, setBaseRef] = useState("");
  // Neither control means anything without a repo: "No folder" is a scratch
  // session, which has no checkout to work in and no branch to fork from.
  const hasFolder = folder !== "";

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
        id={folderId}
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

      {hasFolder && (
        <div className="advanced">
          <button
            type="button"
            className="advanced-toggle"
            aria-expanded={advanced}
            onClick={() => setAdvanced((was) => !was)}
          >
            Advanced
          </button>
          {advanced && (
            <>
              <label className="checkline" htmlFor={checkoutId}>
                <input
                  id={checkoutId}
                  type="checkbox"
                  checked={useCheckout}
                  onChange={(event) => setUseCheckout(event.target.checked)}
                />
                <span>
                  Work in my current folder — no separate worktree
                  <small>
                    Two threads in this repo will then share one checkout, and
                    one will be editing the other's uncommitted work.
                  </small>
                </span>
              </label>

              <FieldLabel htmlFor={baseRefId}>BASE BRANCH</FieldLabel>
              <input
                id={baseRefId}
                type="text"
                value={baseRef}
                placeholder="origin/main"
                // Nothing to fork from when the thread is working in the
                // folder's own checkout: it starts on whatever is checked out.
                disabled={useCheckout}
                onChange={(event) => setBaseRef(event.target.value)}
              />
            </>
          )}
        </div>
      )}

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
                // Omitted rather than sent as `false` / `""`: the ordinary
                // request on the wire has to stay exactly what it was, and the
                // host's own default for a base ref is not the empty string.
                ...(hasFolder && useCheckout ? { useCheckout: true } : {}),
                ...(hasFolder && !useCheckout && baseRef.trim()
                  ? { baseRef: baseRef.trim() }
                  : {}),
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
