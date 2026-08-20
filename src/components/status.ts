//! What a thread row says about itself.
//!
//! Two layers decide this and they are not the same question (#5): `state` is
//! whether the human can see the thread, `runState` is whether the machine is
//! working. Visibility wins — a folded thread reads "sleeping" even while its
//! run is still going, because that is the promise fold makes.

import type { InboxKind, PullRequest, ThreadSummary } from "./types";

/** Drives the pip colour: amber wants you, green is finished, red went wrong. */
export type StatusTone = "running" | "ok" | "bad" | "quiet";

export interface ThreadStatus {
  label: string;
  tone: StatusTone;
}

export function threadStatus(thread: ThreadSummary): ThreadStatus {
  if (thread.state === "folded") {
    return { label: "sleeping", tone: "quiet" };
  }
  if (thread.state === "archived") {
    return { label: "archived", tone: "quiet" };
  }

  switch (thread.runState) {
    case "queued":
      return { label: "queued", tone: "running" };
    case "running":
      return { label: "running", tone: "running" };
    case "needs_you":
      return { label: "needs you", tone: "running" };
    case "succeeded":
      return { label: "done", tone: "ok" };
    case "failed":
      return { label: "failed", tone: "bad" };
    case "timed_out":
      return { label: "timed out", tone: "bad" };
    case "lost":
      return { label: "lost", tone: "bad" };
    case "cancelled":
      return { label: "cancelled", tone: "quiet" };
    case null:
    case undefined:
      return { label: "idle", tone: "quiet" };
  }
}

/** Pill colours on Inbox and PR rows. */
export type PillTone = "done" | "needs" | "merged" | "bad" | "quiet";

export interface Tag {
  label: string;
  tone: PillTone;
}

/**
 * An Inbox card's pill. Amber for anything that is waiting on the human, red
 * for anything that went wrong on its own, grey for work still asleep.
 */
export function inboxTag(kind: InboxKind): Tag {
  switch (kind) {
    case "done":
      return { label: "DONE", tone: "done" };
    case "needs_you":
      return { label: "NEEDS YOU", tone: "needs" };
    case "judgment_call":
      return { label: "JUDGMENT CALL", tone: "needs" };
    case "permission":
      return { label: "PERMISSION", tone: "needs" };
    case "stuck":
      return { label: "STUCK", tone: "needs" };
    case "failed":
      return { label: "FAILED", tone: "bad" };
    case "lost":
      return { label: "LOST", tone: "bad" };
    case "folded":
      return { label: "SLEEPING", tone: "quiet" };
  }
}

/**
 * A pull request's pill. An open PR whose checks are still going is not yet
 * asking to be reviewed, so it says so rather than claiming your attention.
 */
export function prTag(pr: PullRequest): Tag {
  switch (pr.status) {
    case "merged":
      return { label: "MERGED", tone: "merged" };
    case "draft":
      return { label: "DRAFT", tone: "quiet" };
    case "closed":
      return { label: "CLOSED", tone: "quiet" };
    case "open":
      break;
  }
  switch (pr.checkState) {
    case "running":
      return { label: "CHECKS RUNNING", tone: "needs" };
    case "failing":
      return { label: "CHECKS FAILED", tone: "bad" };
    default:
      return { label: "NEEDS REVIEW", tone: "needs" };
  }
}
