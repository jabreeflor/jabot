//! Pull Requests, live from the host (#28).
//!
//! The shape of `crew.ts` and `schedules.ts`: the host serves facts and this
//! renames them into the props `PullRequestsView` already takes. What is *not*
//! a rename is the copy — the summary line, the detail bullets and the row's
//! buttons are presentation, and the host deliberately does not compose them
//! (a phone will phrase them differently, and none of it belongs in SQLite).
//!
//! Two calls, and the split is load-bearing. `pr/list` is a store read and
//! cannot fail on a machine with no GitHub login; `pr/refresh` is a subprocess
//! and a network round trip. So the board is drawn from the first and kept warm
//! by the second, and a refresh that could not reach GitHub leaves the rows
//! alone and reports why.
//!
//! `pullRequests` stays `null` until the host answers. Unlike the crew, an
//! empty answer is entirely legitimate — most people have no open PRs — so
//! `null` and `[]` are genuinely different and the view draws different things.

import { useCallback, useEffect, useRef, useState } from "react";

import type {
  HostClient,
  PrCheckView,
  PrUnavailable,
  PullRequestView,
} from "../host";
import type {
  NoticeAction,
  PrCheck,
  PullRequest,
} from "../components/types";

/**
 * How often to ask GitHub again, from `pr-linkage.md`'s table.
 *
 * A personal desktop app cannot be a webhook target — GitHub refuses a
 * `localhost` payload URL — so this is a poll, and the only honest way to make
 * a poll cheap is to slow it down when nothing is moving. Checks in flight is
 * the one case where seconds matter; everything else is a minute.
 */
export const POLL_WHILE_PENDING_MS = 15_000;
export const POLL_IDLE_MS = 60_000;

export interface PullRequests {
  /** `null` until the host answers; `[]` is a real, and common, answer. */
  pullRequests: PullRequest[] | null;
  /** Why the last refresh did not reach GitHub, if it did not. */
  unavailable: PrUnavailable | null;
  error: string | null;
  reload: () => void;
  /** Ask GitHub now. Never throws for "GitHub was unreachable". */
  refresh: () => Promise<void>;
}

export function usePullRequests(client: HostClient | null): PullRequests {
  const [pullRequests, setPullRequests] = useState<PullRequest[] | null>(null);
  const [unavailable, setUnavailable] = useState<PrUnavailable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  // The interval reads this rather than closing over the rows, so changing the
  // cadence does not mean tearing down and re-arming the timer on every poll.
  const pending = useRef(false);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `pr/list` — a unit test's stub, an older host — should leave the board
    // empty rather than take the render down.
    (async () => client.listPullRequests())()
      .then((listed) => {
        if (cancelled) return;
        const rows = listed.pullRequests.map(prRow);
        pending.current = rows.some((pr) => pr.checkState === "running");
        setPullRequests(rows);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const refresh = useCallback(async () => {
    if (!client) return;
    try {
      const refreshed = await client.refreshPullRequests();
      const rows = refreshed.pullRequests.map(prRow);
      pending.current = rows.some((pr) => pr.checkState === "running");
      setPullRequests(rows);
      // One host in MVP1, so the first entry is the answer. It is `null` on a
      // successful refresh, which is what clears a stale "gh is not installed".
      setUnavailable(refreshed.unavailable[0] ?? null);
      setError(null);
    } catch (err: unknown) {
      // A refresh that could not reach GitHub is not an error frame — this is
      // a genuine RPC failure, and the board stays as it was.
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [client]);

  // The poll. Re-armed whenever the cadence changes, which is whenever the
  // board crosses between "something is running" and "nothing is".
  const cadence = pullRequests?.some((pr) => pr.checkState === "running")
    ? POLL_WHILE_PENDING_MS
    : POLL_IDLE_MS;
  useEffect(() => {
    if (!client) return;
    const timer = setInterval(() => void refresh(), cadence);
    return () => clearInterval(timer);
  }, [client, refresh, cadence]);

  return { pullRequests, unavailable, error, reload, refresh };
}

/** One wire row as the props the prototype's card takes. */
export function prRow(pr: PullRequestView): PullRequest {
  return {
    id: pr.id,
    threadId: pr.threadId,
    provider: pr.provider,
    repo: pr.repo,
    number: pr.number,
    url: pr.url,
    // A row linked but never polled has no title yet — the URL is all the host
    // has been told. Printing an empty card would be worse than saying so.
    title: pr.title || `${pr.repo} #${pr.number}`,
    status: pr.status,
    checkState: pr.checkState ?? null,
    updatedAt: pr.updatedAt,
    additions: pr.additions,
    deletions: pr.deletions,
    headRef: pr.headRef,
    baseRef: pr.baseRef,
    filesChanged: pr.changedFiles || undefined,
    summary: summarize(pr),
    detail: detail(pr),
  };
}

/**
 * The one line under the title: why this row is on the board today.
 *
 * Ordered by what the human can act on. A review verdict outranks the checks,
 * because a person is waiting; the checks outrank the session, because a red
 * build is a job; the session is the fallback, because "from folded session" is
 * the prototype's own copy for a PR whose thread is still asleep.
 */
function summarize(pr: PullRequestView): string | undefined {
  if (pr.polledAt === undefined) {
    return "not checked with GitHub yet";
  }
  if (pr.status === "merged") return "merged";
  if (pr.reviewState === "changes_requested") return "changes requested";
  if (pr.reviewState === "approved") return "approved";
  if (pr.checkState === "failing") return failing(pr.checks);
  if (pr.checkState === "running") return progress(pr.checks);
  if (pr.threadState === "folded") return "from folded session";
  if (pr.checkState === "passing") return "checks green";
  return undefined;
}

/** "2 of 3 checks done" — the prototype's copy, from real counts. */
function progress(checks: PrCheckView[]): string {
  const done = checks.filter((check) => check.state !== "running").length;
  if (checks.length === 0) return "checks running";
  return `${done} of ${checks.length} checks done`;
}

function failing(checks: PrCheckView[]): string {
  const red = checks.filter((check) => check.state === "failing");
  if (red.length === 0) return "checks failed";
  if (red.length === 1) return `${red[0].label} failed`;
  return `${red.length} checks failed`;
}

/**
 * The expanded card. Only for a row there is something to expand *to*: a PR the
 * host has never polled has no checks, no review and no diffstat, and an empty
 * disclosure that opens onto nothing is worse than no disclosure.
 */
function detail(pr: PullRequestView): PullRequest["detail"] {
  if (pr.polledAt === undefined) return undefined;
  return {
    checks: pr.checks.map(
      (check): PrCheck => ({ label: check.label, state: check.state }),
    ),
    bullets: bullets(pr),
    actions: actions(pr),
  };
}

function bullets(pr: PullRequestView): string[] {
  const out: string[] = [];
  if (pr.threadTitle) {
    out.push(`Opened by “${pr.threadTitle}”`);
  }
  if (pr.reviewState === "changes_requested") {
    out.push("A reviewer has asked for changes");
  } else if (pr.reviewState === "review_required") {
    out.push("Waiting for a review");
  } else if (pr.reviewState === "approved") {
    out.push("Approved");
  }
  if (pr.checkState === null || pr.checkState === undefined) {
    // Distinct from green on purpose: nothing ran, so nothing passed.
    out.push("No checks configured for this repository");
  }
  return out;
}

/**
 * What the row's buttons do.
 *
 * Two, and neither needs a host method: `diff` opens the pull request on
 * GitHub, `reopen` is the thread link the whole table exists to preserve. There
 * is no Merge button — merging from JaBot is a host action nobody has built,
 * and a button that opens GitHub while claiming to merge would be worse than
 * the link that is honest about it (`pr-linkage.md` defers it).
 */
function actions(pr: PullRequestView): NoticeAction[] {
  const out: NoticeAction[] = [
    { id: "diff", label: "View on GitHub", primary: pr.status !== "merged" },
  ];
  if (pr.threadState !== "deleted") {
    out.push({ id: "reopen", label: "Reopen thread" });
  }
  return out;
}
