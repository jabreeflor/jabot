//! New Chat: the card that spawns a code thread.
//!
//! Harness is picked *per thread*, not per bot — Code owns many folder threads
//! and any one of them may run a different engine than Code's default (#6). The
//! folder is the repo the thread will work in; "No folder" is a scratch session
//! with no worktree.

import { useId, useState } from "react";

import { FieldLabel, Modal } from "./Modal";
import { HarnessPicker } from "./HarnessPicker";
import type { Folder, HarnessCard, NewChatDraft } from "./types";

export function NewChatModal({
  harnesses,
  folders,
  defaultFolderId = null,
  defaultHarnessId,
  onStart,
  onCancel,
}: {
  harnesses: readonly HarnessCard[];
  folders: readonly Folder[];
  defaultFolderId?: string | null;
  defaultHarnessId?: string;
  onStart: (draft: NewChatDraft) => void;
  onCancel: () => void;
}) {
  const folderId = useId();
  const taskId = useId();
  const [harnessId, setHarnessId] = useState(
    defaultHarnessId ?? harnesses[0]?.id ?? "",
  );
  const [folder, setFolder] = useState(defaultFolderId ?? "");
  const [task, setTask] = useState("");

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
      <select
        id={folderId}
        value={folder}
        onChange={(event) => setFolder(event.target.value)}
      >
        {folders.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
        <option value="">No folder</option>
      </select>

      <FieldLabel htmlFor={taskId}>WHAT SHOULD IT DO?</FieldLabel>
      <input
        id={taskId}
        type="text"
        value={task}
        placeholder="e.g. Add dark mode to settings"
        onChange={(event) => setTask(event.target.value)}
      />

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
            })
          }
        >
          Start session
        </button>
      </div>
    </Modal>
  );
}
