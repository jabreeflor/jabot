//! The two free-text folder fields, and the words a folder error is said in.
//!
//! Lifted out of `AddFolderModal` rather than copied when `FolderSettingsModal`
//! needed the same pair (#16, #23). One reading of "what does a comma-separated
//! files-to-copy list mean", not two that could drift.

import { HostRpcError, RPC_ERROR } from "../host";

/** Comma or newline separated, because both are what people paste. */
export function splitFiles(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((file) => file.trim())
    .filter((file) => file.length > 0);
}

/** Back the other way, for a form seeded from a folder the host already has. */
export function joinFiles(files: readonly string[] | undefined): string {
  return (files ?? []).join(", ");
}

/** The host's own words, except where it has a code that means something
    specific enough to say better. */
export function describe(err: unknown): string {
  if (err instanceof HostRpcError && err.code === RPC_ERROR.FOLDER_EXISTS) {
    return "That checkout is already a folder.";
  }
  if (err instanceof Error) return err.message;
  return String(err);
}
