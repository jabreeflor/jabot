//! Register a folder: point JaBot at one local directory (#16).
//!
//! The host does the deciding. This card collects a path and hands it over;
//! whether it is a git checkout, what its remote is, and whether some other
//! folder already claims it are all questions only the host can answer, and it
//! answers them by probing the directory rather than by trusting this form.
//!
//! Setup script and files-to-copy are stored here and used later: a worktree
//! (#23) starts with no `node_modules` and no `.env`, and the folder is the
//! only place that knows what makes this repo runnable.

import { useId, useState } from "react";

import { FieldLabel, Modal } from "./Modal";
import { HostRpcError, RPC_ERROR, type FolderRegisterParams } from "../host";

export function AddFolderModal({
  onRegister,
  onCancel,
}: {
  /** Resolves once the folder is registered; rejects with the host's error. */
  onRegister: (params: FolderRegisterParams) => Promise<unknown>;
  onCancel: () => void;
}) {
  const pathId = useId();
  const nameId = useId();
  const setupId = useId();
  const filesId = useId();
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [setupCommand, setSetupCommand] = useState("");
  const [filesToCopy, setFilesToCopy] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit() {
    if (!path.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onRegister({
        path: path.trim(),
        name: name.trim() || undefined,
        setupCommand: setupCommand.trim() || undefined,
        filesToCopy: splitFiles(filesToCopy),
      });
      onCancel();
    } catch (err) {
      setError(describe(err));
      setBusy(false);
    }
  }

  return (
    <Modal title="Add folder" onClose={onCancel}>
      <FieldLabel htmlFor={pathId}>FOLDER — ONE REPO</FieldLabel>
      <input
        id={pathId}
        type="text"
        value={path}
        placeholder="~/code/jabot"
        onChange={(event) => setPath(event.target.value)}
      />

      <FieldLabel htmlFor={nameId}>DISPLAY NAME — OPTIONAL</FieldLabel>
      <input
        id={nameId}
        type="text"
        value={name}
        placeholder="Defaults to the repo's own name"
        onChange={(event) => setName(event.target.value)}
      />

      <FieldLabel htmlFor={setupId}>SETUP SCRIPT — OPTIONAL</FieldLabel>
      <input
        id={setupId}
        type="text"
        value={setupCommand}
        placeholder="npm ci"
        onChange={(event) => setSetupCommand(event.target.value)}
      />

      <FieldLabel htmlFor={filesId}>FILES TO COPY — OPTIONAL</FieldLabel>
      <input
        id={filesId}
        type="text"
        value={filesToCopy}
        placeholder=".env, .env.local"
        onChange={(event) => setFilesToCopy(event.target.value)}
      />

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
          disabled={!path.trim() || busy}
          onClick={submit}
        >
          {busy ? "Checking…" : "Add folder"}
        </button>
      </div>
    </Modal>
  );
}

/** Comma or newline separated, because both are what people paste. */
function splitFiles(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((file) => file.trim())
    .filter((file) => file.length > 0);
}

/** The host's own words, except where it has a code that means something
    specific enough to say better. */
function describe(err: unknown): string {
  if (err instanceof HostRpcError && err.code === RPC_ERROR.FOLDER_EXISTS) {
    return "That checkout is already a folder.";
  }
  if (err instanceof Error) return err.message;
  return String(err);
}
