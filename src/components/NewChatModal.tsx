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
import { Select } from "./Select";
import type { Folder, HarnessCard, NewChatDraft } from "./types";

export function NewChatModal({
  harnesses,
  folders,
  defaultFolderId = null,
  defaultHarnessId,
  error = null,
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
  onStart: (draft: NewChatDraft) => void;
  onCancel: () => void;
}) {
  const folderId = useId();
  const taskId = useId();
  const checkoutId = useId();
  const baseRefId = useId();
  const [harnessId, setHarnessId] = useState(
    defaultHarnessId ?? harnesses[0]?.id ?? "",
  );
  const [folder, setFolder] = useState(defaultFolderId ?? "");
  const [task, setTask] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [useCheckout, setUseCheckout] = useState(false);
  const [baseRef, setBaseRef] = useState("");
  // Neither control means anything without a repo: "No folder" is a scratch
  // session, which has no checkout to work in and no branch to fork from.
  const hasFolder = folder !== "";

  return (
    <Modal title="New Chat" onClose={onCancel}>
      <FieldLabel>HARNESS — BRING YOUR OWN</FieldLabel>
      <HarnessPicker
        harnesses={harnesses}
        value={harnessId}
        onChange={setHarnessId}
        label="Harness"
      />

      <FieldLabel htmlFor={folderId}>FOLDER</FieldLabel>
      <Select
        id={folderId}
        value={folder}
        onChange={setFolder}
        options={[
          ...folders.map((f) => ({ value: f.id, label: f.name })),
          { value: "", label: "No folder" },
        ]}
      />

      <FieldLabel htmlFor={taskId}>WHAT SHOULD IT DO?</FieldLabel>
      <input
        id={taskId}
        type="text"
        value={task}
        placeholder="e.g. Add dark mode to settings"
        onChange={(event) => setTask(event.target.value)}
      />

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

      {error && (
        <p className="modal-error" role="alert">
          {error}
        </p>
      )}

      <div className="macts">
        <button type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          onClick={() =>
            onStart({
              harnessId,
              folderId: folder || null,
              task: task.trim() || "Untitled session",
              // Omitted rather than sent as `false` / `""`: the ordinary
              // request on the wire has to stay exactly what it was, and the
              // host's own default for a base ref is not the empty string.
              ...(hasFolder && useCheckout ? { useCheckout: true } : {}),
              ...(hasFolder && !useCheckout && baseRef.trim()
                ? { baseRef: baseRef.trim() }
                : {}),
            })
          }
        >
          Start session
        </button>
      </div>
    </Modal>
  );
}
