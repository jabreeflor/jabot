//! Self-update, renderer half. `src-tauri/src/updates.rs` checks the release
//! feed, downloads and verifies the update, and reports an `UpdateStatus`;
//! this file only listens and asks for the restart.
//!
//! Outside Tauri (the live loop, jsdom) there is no updater, so the status is
//! `disabled` — except `?update=ready`, which previews the banner for a
//! live.sh shot the way `?translucency=preview` previews the glass.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { isTauriRuntime } from "./windowChrome";

export const UPDATE_EVENT = "app-update";
export const UPDATE_PREVIEW_PARAM = "update";

/** Mirrors `updates::UpdateStatus` (serde `tag = "state"`). */
export type UpdateStatus =
  | { state: "disabled"; reason: string }
  | { state: "idle" }
  | { state: "checking"; manual: boolean }
  | { state: "current"; version: string; manual: boolean }
  | { state: "ready"; version: string; current: string }
  | { state: "failed"; message: string; manual: boolean };

const DISABLED: UpdateStatus = {
  state: "disabled",
  reason: "not running in the desktop app",
};

/** `?update=ready` / `?update=current` — a banner for a live.sh shot. */
export function previewUpdateStatus(
  search: string = typeof window === "undefined" ? "" : window.location.search,
): UpdateStatus | null {
  let value: string | null = null;
  try {
    value = new URLSearchParams(search).get(UPDATE_PREVIEW_PARAM);
  } catch {
    return null;
  }
  if (value === "ready") {
    return { state: "ready", version: "0.1.8", current: "0.1.7" };
  }
  if (value === "current") {
    return { state: "current", version: "0.1.7", manual: true };
  }
  return null;
}

/**
 * Report the current status, then every change. Returns the unsubscribe.
 * The initial `app_update_status` covers a renderer that mounted after the
 * event fired — a background check can finish before React does.
 */
export function subscribeUpdates(
  onStatus: (status: UpdateStatus) => void,
): () => void {
  const preview = previewUpdateStatus();
  if (preview) {
    onStatus(preview);
    return () => {};
  }
  if (!isTauriRuntime()) {
    onStatus(DISABLED);
    return () => {};
  }
  let closed = false;
  let unlisten: (() => void) | null = null;
  listen<UpdateStatus>(UPDATE_EVENT, (event) => {
    if (!closed) onStatus(event.payload);
  })
    .then((fn) => {
      if (closed) fn();
      else unlisten = fn;
    })
    .catch(() => {});
  invoke<UpdateStatus>("app_update_status")
    .then((status) => {
      if (!closed) onStatus(status);
    })
    .catch(() => {});
  return () => {
    closed = true;
    unlisten?.();
  };
}

/** Install the staged update and relaunch. Rejects when nothing is staged. */
export async function restartToUpdate(): Promise<void> {
  await invoke("app_update_restart");
}

/** What the banner says, or null when it should not be on screen. */
export function bannerFor(
  status: UpdateStatus,
): { tone: "ready" | "info" | "error"; text: string } | null {
  switch (status.state) {
    case "ready":
      return { tone: "ready", text: `JaBot ${status.version} is ready.` };
    case "current":
      return status.manual
        ? {
            tone: "info",
            text: `You’re on the latest JaBot (${status.version}).`,
          }
        : null;
    case "failed":
      return status.manual
        ? {
            tone: "error",
            text: `Couldn’t check for updates: ${status.message}`,
          }
        : null;
    case "checking":
      return status.manual
        ? { tone: "info", text: "Checking for updates…" }
        : null;
    default:
      return null;
  }
}
