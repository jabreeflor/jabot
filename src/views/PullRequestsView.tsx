//! Pull Requests — what the coding sessions produced, and what you have open.
//!
//! Sections are ordered by what the human can act on: PRs waiting for review,
//! then PRs whose checks are still running (nothing to do yet), then what has
//! already landed. A draft is listed under Open but never claims review.
//!
//! Rows come from two places (#28). `thread_prs` joined with GitHub state is
//! the linkage half: what a session on this Mac opened, drawable with no
//! credential at all. `pr/mine` is the other half — every open pull request
//! the signed-in user wrote, wherever it lives — and it is why this view is
//! also where signing in happens. A row from the first always has a session to
//! reopen; a row from the second usually does not, and says so by simply not
//! offering the button.
//!
//! Signed out, the board is exactly what it was, and the strip above it says
//! what signing in would add rather than blocking the view behind a login.

import { useState } from "react";

import {
  CheckIcon,
  CrossIcon,
  DotIcon,
  PullRequestIcon,
  PullRequestMergedIcon,
} from "../components/Icon";
import { formatWhen } from "../components/format";
import { prTag } from "../components/status";
import { Tabs, tabButtonId, type TabSpec } from "../components/Tabs";
import type { GithubStatusResult, PrUnavailable } from "../host";
import type { PullRequest } from "../components/types";

type PrTab = "open" | "merged" | "drafts";

export function PullRequestsView({
  pullRequests,
  now,
  unavailable,
  error,
  githubStatus,
  account,
  onSignIn,
  onRefresh,
  onOpenThread,
  onAction,
}: {
  pullRequests: readonly PullRequest[];
  now?: Date;
  /** Why the last poll did not reach GitHub, if it did not (#28). Drawn as a
      strip rather than as an empty board: the rows are linkage, which needs no
      credential, and they are still true when GitHub is unreachable. */
  unavailable?: PrUnavailable | null;
  error?: string | null;
  /** Whether the host can ask GitHub as anybody, and as whom (#16). `null`
      until it has answered — a preview build or a unit test — which draws no
      strip at all rather than an offer to sign in that would go nowhere. */
  githubStatus?: GithubStatusResult | null;
  /** Who GitHub itself answered as on the last `pr/mine`. Preferred over the
      `gh` status's account when both are known: it is the login the rows on
      screen actually belong to. */
  account?: string | null;
  /** Open the sign-in dialog. Absent means this build cannot sign in. */
  onSignIn?: () => void;
  onRefresh?: () => void;
  onOpenThread: (threadId: string) => void;
  onAction?: (prId: string, actionId: string) => void;
}) {
  const [tab, setTab] = useState<PrTab>("open");
  const [openId, setOpenId] = useState<string | null>(
    pullRequests.find((pr) => pr.detail)?.id ?? null,
  );

  const open = pullRequests.filter((pr) => pr.status === "open");
  const drafts = pullRequests.filter((pr) => pr.status === "draft");
  const merged = pullRequests.filter((pr) => pr.status === "merged");
  // Closed-without-merging. The host has parsed, stored and served this status
  // all along and `prTag` has always had a pill for it, but no section ever
  // held one — so a PR someone closed simply vanished off the board, which
  // reads as "JaBot lost it" rather than "somebody closed it".
  const closed = pullRequests.filter((pr) => pr.status === "closed");

  const tabs: readonly TabSpec<PrTab>[] = [
    { id: "open", label: "Open", count: open.length },
    { id: "merged", label: "Merged" },
    { id: "drafts", label: "Drafts" },
  ];

  const sections =
    tab === "open"
      ? [
          {
            title: "NEEDS REVIEW",
            rows: open.filter((pr) => pr.checkState !== "running"),
          },
          {
            title: "CHECKS RUNNING",
            rows: open.filter((pr) => pr.checkState === "running"),
          },
          { title: "RECENTLY MERGED", rows: merged },
          // On the Open tab as well as the Merged one, and for the same
          // reason RECENTLY MERGED is here: the question a vanished row
          // raises is asked while looking at Open. Empty sections are
          // dropped below, so this costs nothing on a board with none.
          { title: "CLOSED WITHOUT MERGING", rows: closed },
        ]
      : tab === "merged"
        ? [
            { title: "MERGED", rows: merged },
            { title: "CLOSED WITHOUT MERGING", rows: closed },
          ]
        : [{ title: "DRAFTS", rows: drafts }];

  const visible = sections.filter((section) => section.rows.length > 0);

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Pull Requests</h1>
            <p>
              {githubStatus?.authenticated
                ? "Everything you have open, and what your sessions opened"
                : "Opened by your coding sessions — review, merge, or send back"}
            </p>
          </div>

          <GithubStrip
            status={githubStatus}
            account={account}
            onSignIn={onSignIn}
          />

          {(unavailable || error) && (
            <div className="page-notice" role="status">
              <span>
                {unavailable
                  ? `${unavailable.detail}${
                      unavailable.remedy ? ` Try: ${unavailable.remedy}` : ""
                    }`
                  : error}
              </span>
              {onRefresh && (
                <button type="button" className="btn" onClick={onRefresh}>
                  Retry
                </button>
              )}
            </div>
          )}

          <Tabs
            label="Pull request filter"
            panelId="prs-panel"
            tabs={tabs}
            value={tab}
            onChange={setTab}
          />

          <div
            id="prs-panel"
            role="tabpanel"
            aria-labelledby={tabButtonId("prs-panel", tab)}
          >
            {visible.length === 0 && (
              <div className="page-empty">No pull requests here.</div>
            )}
            {visible.map((section) => (
              <div key={section.title}>
                <div className="page-section">{section.title}</div>
                {section.rows.map((pr) => (
                  <PrRow
                    key={pr.id}
                    pr={pr}
                    now={now}
                    open={openId === pr.id}
                    onToggle={() => setOpenId(openId === pr.id ? null : pr.id)}
                    onOpenThread={onOpenThread}
                    onAction={onAction}
                  />
                ))}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Who the board is showing, or an offer to make it show more.
 *
 * Never a gate. The rows underneath are linkage, they needed no credential to
 * collect, and they are still true for a user who will never sign in — so this
 * is one line above them, not a wall in front of them.
 *
 * The three states are the three the host reports, because they have three
 * different ways forward: signed in (say as whom), signed out (offer the
 * dialog), and no `gh` at all (still offer it — the dialog is where the
 * install line is written, and it is a better place for it than a strip that
 * everybody who *is* signed in would also have to read past).
 */
function GithubStrip({
  status,
  account,
  onSignIn,
}: {
  status?: GithubStatusResult | null;
  account?: string | null;
  onSignIn?: () => void;
}) {
  if (!status) return null;

  if (status.authenticated) {
    const who = account ?? status.account;
    return (
      <div className="page-account">
        Showing every pull request you have open
        {who ? ` as @${who}` : ""} on {status.host}.
      </div>
    );
  }

  return (
    <div className="page-notice offer" role="status">
      <span>
        Sign in to GitHub to see every pull request you have open — not just the
        ones opened here.
      </span>
      {onSignIn && (
        <button type="button" className="btn primary" onClick={onSignIn}>
          Sign in with GitHub
        </button>
      )}
    </div>
  );
}

function PrRow({
  pr,
  now,
  open,
  onToggle,
  onOpenThread,
  onAction,
}: {
  pr: PullRequest;
  now?: Date;
  open: boolean;
  onToggle: () => void;
  onOpenThread: (threadId: string) => void;
  onAction?: (prId: string, actionId: string) => void;
}) {
  const tag = prTag(pr);
  const quiet = pr.status === "merged" || pr.status === "draft";

  return (
    <div
      className={["card-row", open ? "open" : "", quiet ? "dim" : ""]
        .filter(Boolean)
        .join(" ")}
    >
      <div className={`prav ${pr.status}`}>
        {pr.status === "merged" ? (
          <PullRequestMergedIcon />
        ) : (
          <PullRequestIcon />
        )}
      </div>
      <div className="bd">
        <button
          type="button"
          className="card-summary"
          aria-expanded={pr.detail ? open : undefined}
          onClick={onToggle}
        >
          <span className="r1">
            <span className="ti">{pr.title}</span>
            <span className="when">{formatWhen(pr.updatedAt, now)}</span>
          </span>
          <span className="de">
            {pr.repo} #{pr.number}
            {pr.summary ? ` · ${pr.summary}` : ""} ·{" "}
            <span className="diffstat">
              <span className="a">+{pr.additions}</span>{" "}
              <span className="d">−{pr.deletions}</span>
            </span>
          </span>
          <span className={`tagpill ${tag.tone}`}>{tag.label}</span>
        </button>

        {open && pr.detail && (
          <div className="card-detail">
            <div className="path">
              {pr.headRef} → <b>{pr.baseRef}</b>
              {pr.filesChanged !== undefined &&
                ` · ${pr.filesChanged} files changed`}
            </div>
            <div className="checkline">
              {pr.detail.checks.map((check) => (
                <span key={check.label}>
                  <span
                    className={
                      check.state === "passing"
                        ? "tick"
                        : check.state === "running"
                          ? "spin"
                          : "fail"
                    }
                  >
                    {check.state === "passing" ? (
                      <CheckIcon />
                    ) : check.state === "running" ? (
                      <DotIcon />
                    ) : (
                      <CrossIcon />
                    )}
                  </span>{" "}
                  {check.label}
                </span>
              ))}
            </div>
            <ul>
              {pr.detail.bullets.map((bullet) => (
                <li key={bullet}>{bullet}</li>
              ))}
            </ul>
            <div className="acts">
              {pr.detail.actions.map((action) => (
                <button
                  key={action.id}
                  type="button"
                  className={action.primary ? "btn primary" : "btn"}
                  onClick={() =>
                    action.id === "reopen" && pr.threadId
                      ? onOpenThread(pr.threadId)
                      : onAction?.(pr.id, action.id)
                  }
                >
                  {action.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
