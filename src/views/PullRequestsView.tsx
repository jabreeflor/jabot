//! Pull Requests — what the coding sessions produced.
//!
//! Sections are ordered by what the human can act on: PRs waiting for review,
//! then PRs whose checks are still running (nothing to do yet), then what has
//! already landed. A draft is listed under Open but never claims review.
//!
//! Rows are `thread_prs` joined with GitHub state (#28). Every PR here was
//! opened by a session — that is what the table is — so "Reopen thread"
//! always has somewhere to go.

import { useState } from "react";

import { PullRequestIcon, PullRequestMergedIcon } from "../components/Icon";
import { formatWhen } from "../components/format";
import { prTag } from "../components/status";
import { Tabs, tabButtonId, type TabSpec } from "../components/Tabs";
import type { PrUnavailable } from "../host";
import type { PullRequest } from "../components/types";

type PrTab = "open" | "merged" | "drafts";

export function PullRequestsView({
  pullRequests,
  now,
  unavailable,
  error,
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
        ]
      : tab === "merged"
        ? [{ title: "MERGED", rows: merged }]
        : [{ title: "DRAFTS", rows: drafts }];

  const visible = sections.filter((section) => section.rows.length > 0);

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Pull Requests</h1>
            <p>Opened by your coding sessions — review, merge, or send back</p>
          </div>

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
                    {check.state === "passing"
                      ? "✓"
                      : check.state === "running"
                        ? "●"
                        : "✗"}
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
                    action.id === "reopen"
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
