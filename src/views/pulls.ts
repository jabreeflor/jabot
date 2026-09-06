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
//!
//! **Two boards, one list.** `pr/list` and `pr/refresh` answer *linkage*: what
//! a session on this Mac opened, which needs no login. `pr/mine` answers the
//! person: every open pull request they wrote, wherever it lives, which needs
//! one. A signed-in user wants to see both at once and does not care which
//! call a row came from, so [`mergeBoards`] folds them on `(provider, repo,
//! number)` — the same identity the host dedupes linkage on. Signed out,
//! nothing is lost: the linked board is exactly what it was.

import { useCallback, useEffect, useMemo, useState } from "react";

import type {
  GithubPullRequestView,
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
  /** `null` until the host answers; `[]` is a real, and common, answer.
      Signed in, this is the linked board and the user's own GitHub pull
      requests folded together. */
  pullRequests: PullRequest[] | null;
  /** Who GitHub answered `pr/mine` as, when it was asked and answered. */
  account: string | null;
  /** Why the last refresh did not reach GitHub, if it did not. */
  unavailable: PrUnavailable | null;
  error: string | null;
  reload: () => void;
  /** Ask GitHub now. Never throws for "GitHub was unreachable". */
  refresh: () => Promise<void>;
}

/**
 * @param signedIn whether GitHub can be asked at all. `pr/mine` is the one PR
 * call that is useless without a login — asking anyway would spend a
 * subprocess and a timeout every minute to be told the same thing the sign-in
 * strip is already saying.
 */
export function usePullRequests(
  client: HostClient | null,
  signedIn = false,
): PullRequests {
  const [linked, setLinked] = useState<PullRequest[] | null>(null);
  const [mine, setMine] = useState<PullRequest[] | null>(null);
  const [account, setAccount] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState<PrUnavailable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `pr/list` — a unit test's stub, an older host — should leave the board
    // empty rather than take the render down.
    (async () => client.listPullRequests())()
      .then((listed) => {
        if (cancelled) return;
        setLinked(listed.pullRequests.map(prRow));
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

  // The user's own board. Only asked for when there is a login to ask with,
  // and never able to fail the refresh: a `pr/mine` that could not reach
  // GitHub leaves the rows it last returned alone, exactly as the linked half
  // does.
  useEffect(() => {
    if (!client || !signedIn) {
      // Signing out takes the rows with it. Leaving them would show a
      // stranger — or a stale board nothing can refresh — under someone
      // else's account.
      setMine(null);
      setAccount(null);
      return;
    }
    let cancelled = false;
    (async () => client.myPullRequests())()
      .then((answer) => {
        if (cancelled) return;
        setAccount(answer.account ?? null);
        if (answer.unavailable) {
          setUnavailable(answer.unavailable);
          return;
        }
        setMine(answer.pullRequests.map(mineRow));
        setUnavailable(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, signedIn, generation]);

  /** The board as the host holds it. No `gh`, no network — see the poll. */
  const reread = useCallback(async () => {
    if (!client) return;
    try {
      const listed = await client.listPullRequests();
      setLinked(listed.pullRequests.map(prRow));
      setError(null);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [client]);

  const refresh = useCallback(async () => {
    if (!client) return;
    try {
      const refreshed = await client.refreshPullRequests();
      setLinked(refreshed.pullRequests.map(prRow));
      // One host in MVP1, so the first entry is the answer. It is `null` on a
      // successful refresh, which is what clears a stale "gh is not installed".
      setUnavailable(refreshed.unavailable[0] ?? null);
      setError(null);
      if (!signedIn) return;
      // Same tick, one call later: the two halves of the board must not drift
      // a poll apart, or a PR opened by a session here would show GitHub's
      // answer from a minute ago beside its own from now.
      const own = await client.myPullRequests();
      setAccount(own.account ?? null);
      if (own.unavailable) {
        setUnavailable(own.unavailable);
        return;
      }
      setMine(own.pullRequests.map(mineRow));
    } catch (err: unknown) {
      // A refresh that could not reach GitHub is not an error frame — this is
      // a genuine RPC failure, and the board stays as it was.
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [client, signedIn]);

  const pullRequests = useMemo(
    () => (linked === null && mine === null ? null : mergeBoards(linked, mine)),
    [linked, mine],
  );

  // Reading what the host has already fetched, not fetching.
  //
  // This used to be `refresh()` — `pr/refresh`, a subprocess and a network
  // round trip — armed only while a webview was alive and running. Since
  // `card::transition` writes an Inbox `pr` card only when a *refresh*
  // observes a change, that meant no "checks failed" card could be written
  // while the app sat in the Dock with its timers throttled, and none at all
  // under `jabot-hostd`, which has no renderer to arm anything. The poll is
  // the host's now (#28), on the same two cadences and the same code path.
  //
  // What is left here is `pr/list`: a pure store read that cannot fail without
  // a GitHub login, so a board on a machine that has never signed in costs a
  // SQLite query a minute and nothing else.
  const cadence = pullRequests?.some((pr) => pr.checkState === "running")
    ? POLL_WHILE_PENDING_MS
    : POLL_IDLE_MS;
  useEffect(() => {
    if (!client) return;
    const timer = setInterval(() => void reread(), cadence);
    return () => clearInterval(timer);
  }, [client, reread, cadence]);

  return { pullRequests, account, unavailable, error, reload, refresh };
}

/** `(provider, repo, number)`, which is what the host dedupes linkage on. */
function prKey(pr: PullRequest): string {
  return `${pr.provider}:${pr.repo}#${pr.number}`;
}

/**
 * One board out of two.
 *
 * GitHub wins where they overlap, because `pr/mine` is always a fresh answer
 * and a linked row may never have been polled at all — but it inherits the
 * linked row's `id`, so signing in re-labels the rows React is already drawing
 * rather than replacing them. A linked PR that GitHub did not mention is kept:
 * `pr/mine` asks only for what is *open*, and dropping the merged ones would
 * empty the Merged tab the moment somebody signed in.
 */
export function mergeBoards(
  linked: readonly PullRequest[] | null,
  mine: readonly PullRequest[] | null,
): PullRequest[] {
  const rows = new Map<string, PullRequest>();
  for (const pr of linked ?? []) rows.set(prKey(pr), pr);
  for (const pr of mine ?? []) {
    const already = rows.get(prKey(pr));
    rows.set(prKey(pr), already ? { ...pr, id: already.id } : pr);
  }
  return [...rows.values()].sort((a, b) =>
    b.updatedAt.localeCompare(a.updatedAt),
  );
}

/** One wire row as the props the prototype's card takes. */
export function prRow(pr: PullRequestView): PullRequest {
  const facts = factsOf(pr, pr.polledAt !== undefined);
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
    summary: summarize(facts),
    detail: detail(facts),
  };
}

/**
 * One of the user's own pull requests as the same card.
 *
 * Every row here came back from GitHub this second, so unlike the linked
 * board there is no "never polled" case to draw around — `polled` is always
 * true. What it does have that the linked board cannot is a *missing* thread:
 * most of these were written somewhere else, so `threadId` is absent and the
 * card offers no "Reopen thread" rather than a button that goes nowhere.
 */
export function mineRow(pr: GithubPullRequestView): PullRequest {
  const facts = factsOf(pr, true);
  return {
    id: pr.id,
    threadId: pr.threadId,
    provider: pr.provider,
    repo: pr.repo,
    number: pr.number,
    url: pr.url,
    title: pr.title || `${pr.repo} #${pr.number}`,
    status: pr.status,
    checkState: pr.checkState ?? null,
    updatedAt: pr.updatedAt,
    additions: pr.additions,
    deletions: pr.deletions,
    headRef: pr.headRef,
    baseRef: pr.baseRef,
    filesChanged: pr.changedFiles || undefined,
    summary: summarize(facts),
    detail: detail(facts),
  };
}

/**
 * What the copy is composed from.
 *
 * The two wire shapes differ in exactly the ways the *card* does not care
 * about — one carries a thread it is certain of, the other a thread it may not
 * have — so the sentences are written against this and not against either. The
 * one fact neither type spells the same way is whether GitHub has been asked
 * at all, which is why it is a parameter.
 */
interface PrFacts {
  status: PullRequestView["status"];
  reviewState: PullRequestView["reviewState"];
  checkState: PullRequestView["checkState"];
  checks: readonly PrCheckView[];
  threadTitle: string | undefined;
  threadState: PullRequestView["threadState"] | undefined;
  polled: boolean;
}

function factsOf(
  pr: PullRequestView | GithubPullRequestView,
  polled: boolean,
): PrFacts {
  return {
    status: pr.status,
    reviewState: pr.reviewState,
    checkState: pr.checkState,
    checks: pr.checks,
    // An empty title is the linked board's "no session named this yet"; both
    // shapes mean the same thing by absence.
    threadTitle: pr.threadTitle || undefined,
    threadState: pr.threadState,
    polled,
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
function summarize(pr: PrFacts): string | undefined {
  if (!pr.polled) {
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
function progress(checks: readonly PrCheckView[]): string {
  const done = checks.filter((check) => check.state !== "running").length;
  if (checks.length === 0) return "checks running";
  return `${done} of ${checks.length} checks done`;
}

function failing(checks: readonly PrCheckView[]): string {
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
function detail(pr: PrFacts): PullRequest["detail"] {
  if (!pr.polled) return undefined;
  return {
    checks: pr.checks.map(
      (check): PrCheck => ({ label: check.label, state: check.state }),
    ),
    bullets: bullets(pr),
    actions: actions(pr),
  };
}

function bullets(pr: PrFacts): string[] {
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
 * These secondary row actions retain the GitHub and coding-session links.
 * PullRequestsView supplies the primary in-app workspace entry point, where
 * reviews, comments and merges are handled by the authenticated host.
 */
function actions(pr: PrFacts): NoticeAction[] {
  const out: NoticeAction[] = [
    { id: "diff", label: "View on GitHub", primary: pr.status !== "merged" },
  ];
  if (pr.threadState !== undefined && pr.threadState !== "deleted") {
    out.push({ id: "reopen", label: "Reopen thread" });
  }
  return out;
}
