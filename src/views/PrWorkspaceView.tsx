import { renderMarkdown } from "../components/markdown";
import { useEffect, useRef, useState } from "react";
import type { HostClient } from "../host";
import type { PullRequest } from "../components/types";
import type { PrAction, PrWorkspace, PrFile } from "../host/prWorkspace";
import { Tabs, tabButtonId } from "../components/Tabs";

type Section = "conversation" | "files" | "commits" | "checks";
type LineTarget = { path: string; line: number; side: "LEFT" | "RIGHT" };
const message = (e: unknown) => {
  if (e && typeof e === "object" && "data" in e) {
    const data = e.data as { detail?: string } | undefined;
    if (data?.detail) return data.detail;
  }
  return e instanceof Error ? e.message : String(e);
};

export function PrWorkspaceView({
  pr,
  client,
  onBack,
  onOpenThread,
}: {
  pr: PullRequest;
  client: HostClient | null;
  onBack: () => void;
  onOpenThread: (id: string) => void;
}) {
  const [data, setData] = useState<PrWorkspace | null>(null);
  const [section, setSection] = useState<Section>("conversation");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editBody, setEditBody] = useState("");
  const [reviewers, setReviewers] = useState("");
  const [body, setBody] = useState("");
  const [review, setReview] = useState<
    "COMMENT" | "APPROVE" | "REQUEST_CHANGES"
  >("COMMENT");
  const [strategy, setStrategy] = useState<"squash" | "merge" | "rebase">(
    "squash",
  );
  const [confirm, setConfirm] = useState<"merge" | "close" | null>(null);
  const [line, setLine] = useState<LineTarget | null>(null);
  const [inlineBody, setInlineBody] = useState("");
  const [fileSearch, setFileSearch] = useState("");
  const [viewed, setViewed] = useState<Set<string>>(new Set());
  const inFlight = useRef(false);
  const generation = useRef(0);
  const target = {
    repo: pr.repo,
    number: pr.number,
    host: new URL(pr.url).hostname,
  };
  async function load() {
    if (!client) return;
    const request = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const next = await client.pullRequestDetail(target);
      if (request === generation.current) {
        setData(next);
      }
    } catch (e) {
      if (request === generation.current) setError(message(e));
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }
  useEffect(() => {
    setViewed(new Set());
  }, [data?.pr.head.sha]);
  useEffect(() => {
    void load();
    return () => {
      generation.current++;
    };
  }, [client, pr.id]); // keyed by PR in the parent
  async function act(action: PrAction["action"]) {
    if (!client || !data || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setError(null);
    setNotice("");
    try {
      await client.pullRequestAction({
        ...target,
        action,
        body:
          action === "inline"
            ? inlineBody
            : action === "edit"
              ? editBody
              : body,
        title: editTitle,
        reviewers: reviewers
          .split(",")
          .map((name) => name.trim().replace(/^@/, ""))
          .filter(Boolean),
        sha: data.pr.head.sha,
        method: strategy,
        ...(action === "inline" ? line : {}),
      });
      setNotice(
        action === "merge"
          ? "Pull request merged."
          : action === "close"
            ? "Pull request closed."
            : action === "reopen"
              ? "Pull request reopened."
              : "Posted to GitHub.",
      );
      if (action === "inline") {
        setInlineBody("");
        setLine(null);
      } else if (
        ["comment", "COMMENT", "APPROVE", "REQUEST_CHANGES"].includes(action)
      )
        setBody("");
      if (action === "edit") setEditing(false);
      if (action === "reviewers") setReviewers("");
      setConfirm(null);
      await load();
    } catch (e) {
      setError(
        `${message(e)} If the request timed out, refresh before retrying to check whether GitHub received it.`,
      );
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  }
  const detail = data?.pr;
  const open = detail?.state === "open" && !detail.merged;
  const checkRuns = data?.checks?.check_runs ?? [];
  const statuses = data?.statuses?.statuses ?? [];
  const blocked =
    !open ||
    detail?.draft ||
    detail?.mergeable !== true ||
    ["blocked", "dirty", "behind", "unknown"].includes(
      detail?.mergeable_state ?? "unknown",
    );
  return (
    <div className="view">
      <div className="page-scroll">
        <div className="pr-workspace">
          <div className="pr-toolbar">
            <button className="btn" onClick={onBack} disabled={busy}>
              ← Pull requests
            </button>
            <span className="pr-muted">
              {pr.repo} / #{pr.number}
            </span>
            <a className="btn" href={pr.url} target="_blank" rel="noreferrer">
              View on GitHub ↗
            </a>
            <button
              className="btn"
              onClick={() => void load()}
              disabled={loading || busy || !client}
            >
              {loading ? "Refreshing…" : "Refresh"}
            </button>
          </div>
          <header className="pr-header">
            <h1>
              {detail?.title ?? pr.title} <span>#{pr.number}</span>
            </h1>
            <div className="pr-meta">
              <span className={`tagpill ${detail?.merged ? "violet" : "ok"}`}>
                {detail
                  ? detail.merged
                    ? "Merged"
                    : detail.draft
                      ? "Draft"
                      : detail.state
                  : pr.status}
              </span>
              <span>
                {detail
                  ? `@${detail.user.login} wants to merge`
                  : "Pull request"}
              </span>
              <code>{detail?.head.ref ?? pr.headRef}</code>
              <span>→</span>
              <code>{detail?.base.ref ?? pr.baseRef}</code>
              <span className="pr-added">
                +{detail?.additions ?? pr.additions}
              </span>
              <span className="pr-removed">
                −{detail?.deletions ?? pr.deletions}
              </span>
            </div>
          </header>
          {!client && (
            <div className="page-notice">
              Connect to the desktop host to load this PR and post to GitHub.
            </div>
          )}
          {error && (
            <div className="page-notice" role="alert">
              {error}
            </div>
          )}
          {notice && (
            <div className="page-notice" role="status">
              {notice}
            </div>
          )}
          {loading && !data && (
            <p role="status">Loading conversation, files, and checks…</p>
          )}
          <Tabs
            label="Pull request sections"
            panelId="pr-content"
            value={section}
            onChange={setSection}
            tabs={[
              {
                id: "conversation",
                label: "Conversation",
                count: data
                  ? data.comments.length +
                    data.reviews.length +
                    data.inline.length
                  : undefined,
              },
              {
                id: "files",
                label: "Files changed",
                count: data?.files.length,
              },
              { id: "commits", label: "Commits", count: data?.commits.length },
              {
                id: "checks",
                label: "Checks",
                count: data ? checkRuns.length + statuses.length : undefined,
              },
            ]}
          />
          {data && (
            <div className="pr-layout">
              <main
                id="pr-content"
                role="tabpanel"
                aria-labelledby={tabButtonId("pr-content", section)}
              >
                {section === "conversation" && (
                  <>
                    <article className="pr-box">
                      <div className="pr-box-heading">
                        @{data.pr.user.login}
                        <span>Description</span>
                        <button
                          className="btn"
                          disabled={busy || loading}
                          onClick={() => {
                            setEditing(true);
                            setEditTitle(data.pr.title);
                            setEditBody(data.pr.body ?? "");
                          }}
                        >
                          Edit
                        </button>
                      </div>
                      {editing ? (
                        <div className="pr-compose">
                          <label htmlFor="pr-title">Title</label>
                          <input
                            id="pr-title"
                            value={editTitle}
                            onChange={(e) => setEditTitle(e.target.value)}
                            disabled={busy}
                          />
                          <label htmlFor="pr-description">Description</label>
                          <textarea
                            id="pr-description"
                            value={editBody}
                            onChange={(e) => setEditBody(e.target.value)}
                            disabled={busy}
                          />
                          <div className="pr-actions">
                            <button
                              className="btn"
                              disabled={busy}
                              onClick={() => setEditing(false)}
                            >
                              Cancel edit
                            </button>
                            <button
                              className="btn primary"
                              disabled={busy || loading || !editTitle.trim()}
                              onClick={() => void act("edit")}
                            >
                              Save changes
                            </button>
                          </div>
                        </div>
                      ) : (
                        <div className="pr-prose">
                          {renderMarkdown(
                            data.pr.body || "No description provided.",
                          )}
                        </div>
                      )}
                    </article>
                    {[...data.comments, ...data.reviews, ...data.inline]
                      .sort((a, b) =>
                        (a.created_at ?? a.submitted_at ?? "").localeCompare(
                          b.created_at ?? b.submitted_at ?? "",
                        ),
                      )
                      .map((item) => (
                        <article
                          className="pr-box"
                          key={`${item.html_url}-${item.id}`}
                        >
                          <div className="pr-box-heading">
                            <strong>@{item.user.login}</strong>
                            <span>
                              {item.state?.replace(/_/g, " ") ??
                                (item.path ? "Code comment" : "Comment")}
                            </span>
                            <a
                              href={item.html_url}
                              target="_blank"
                              rel="noreferrer"
                            >
                              {new Date(
                                item.created_at ?? item.submitted_at ?? "",
                              ).toLocaleDateString()}
                            </a>
                          </div>
                          {item.path && (
                            <code className="pr-file-label">
                              {item.path}
                              {item.line ? `:${item.line}` : " · outdated"}
                            </code>
                          )}
                          <div className="pr-prose">
                            {renderMarkdown(
                              item.body ||
                                "Review submitted without a comment.",
                            )}
                          </div>
                        </article>
                      ))}
                    <section className="pr-box pr-compose">
                      <h3>Join the conversation</h3>
                      <label htmlFor="pr-body">
                        Comment or review · Markdown supported by GitHub
                      </label>
                      <textarea
                        id="pr-body"
                        placeholder="Leave feedback, ask a question, or summarize your review…"
                        value={body}
                        onChange={(e) => setBody(e.target.value)}
                        disabled={busy}
                      />
                      <div className="pr-actions">
                        <button
                          className="btn"
                          disabled={busy || loading || !body.trim()}
                          onClick={() => void act("comment")}
                        >
                          Comment
                        </button>
                        {open && (
                          <>
                            <select
                              aria-label="Review decision"
                              value={review}
                              onChange={(e) =>
                                setReview(e.target.value as typeof review)
                              }
                              disabled={busy}
                            >
                              <option value="COMMENT">Comment on review</option>
                              <option value="APPROVE">Approve</option>
                              <option value="REQUEST_CHANGES">
                                Request changes
                              </option>
                            </select>
                            <button
                              className="btn primary"
                              disabled={
                                busy ||
                                loading ||
                                (review !== "APPROVE" && !body.trim())
                              }
                              onClick={() => void act(review)}
                            >
                              {busy ? "Submitting…" : "Submit review"}
                            </button>
                          </>
                        )}
                      </div>
                    </section>
                  </>
                )}
                {section === "files" && (
                  <>
                    <div className="pr-toolbar">
                      <input
                        aria-label="Filter files"
                        placeholder="Filter files…"
                        value={fileSearch}
                        onChange={(e) => setFileSearch(e.target.value)}
                      />
                      <span className="pr-muted">
                        {viewed.size} / {data.files.length} viewed
                      </span>
                    </div>
                    {data.files
                      .filter((f) =>
                        f.filename
                          .toLowerCase()
                          .includes(fileSearch.toLowerCase()),
                      )
                      .map((file) => (
                        <DiffFile
                          key={file.filename}
                          file={file}
                          viewed={viewed.has(file.filename)}
                          onViewed={() =>
                            setViewed((old) => {
                              const next = new Set(old);
                              if (next.has(file.filename))
                                next.delete(file.filename);
                              else next.add(file.filename);
                              return next;
                            })
                          }
                          onLine={
                            open && !busy && !loading ? setLine : undefined
                          }
                        />
                      ))}
                    {data.files.length < data.pr.changed_files && (
                      <p>
                        GitHub returned {data.files.length} of{" "}
                        {data.pr.changed_files} files. View the full change on
                        GitHub.
                      </p>
                    )}
                    {line && (
                      <section className="pr-box pr-compose">
                        <h3>
                          Comment on {line.path}:{line.line} (
                          {line.side.toLowerCase()})
                        </h3>
                        <textarea
                          aria-label="Inline comment"
                          value={inlineBody}
                          onChange={(e) => setInlineBody(e.target.value)}
                          autoFocus
                          disabled={busy}
                        />
                        <div className="pr-actions">
                          <button
                            className="btn"
                            disabled={busy}
                            onClick={() => setLine(null)}
                          >
                            Cancel
                          </button>
                          <button
                            className="btn primary"
                            disabled={busy || loading || !inlineBody.trim()}
                            onClick={() => void act("inline")}
                          >
                            Post line comment
                          </button>
                        </div>
                      </section>
                    )}
                  </>
                )}
                {section === "commits" &&
                  data.commits.map((commit) => (
                    <article className="pr-box pr-commit" key={commit.sha}>
                      <div>
                        <strong>{commit.commit.message.split("\n")[0]}</strong>
                        <p className="pr-muted">{commit.commit.author.name}</p>
                      </div>
                      <a
                        href={commit.html_url}
                        target="_blank"
                        rel="noreferrer"
                      >
                        <code>{commit.sha.slice(0, 7)}</code>
                      </a>
                    </article>
                  ))}
                {section === "checks" && (
                  <section className="pr-box">
                    <div className="pr-box-heading">
                      Checks and commit statuses
                    </div>
                    {data.checksError && (
                      <p className="pr-prose">
                        Some checks could not be loaded: {data.checksError}
                      </p>
                    )}
                    {checkRuns.length + statuses.length === 0 && (
                      <p className="pr-prose">
                        No checks reported for this commit.
                      </p>
                    )}
                    {checkRuns.map((check) => (
                      <div className="pr-check" key={check.id}>
                        <span>{check.name}</span>
                        <span>{check.conclusion ?? check.status}</span>
                        <a
                          href={check.html_url}
                          target="_blank"
                          rel="noreferrer"
                        >
                          Details ↗
                        </a>
                      </div>
                    ))}
                    {statuses.map((check) => (
                      <div className="pr-check" key={check.id}>
                        <span>{check.context}</span>
                        <span>{check.state}</span>
                        {check.target_url && (
                          <a
                            href={check.target_url}
                            target="_blank"
                            rel="noreferrer"
                          >
                            Details ↗
                          </a>
                        )}
                      </div>
                    ))}
                    {((data.checks?.total_count ?? 0) > checkRuns.length ||
                      (data.statuses?.total_count ?? 0) > statuses.length) && (
                      <p className="pr-prose">
                        Showing the first 100 checks/statuses. View additional
                        results on GitHub.
                      </p>
                    )}
                  </section>
                )}
              </main>
              <aside className="pr-aside">
                <section className="pr-box pr-compose">
                  <h3>
                    {data.pr.merged
                      ? "Successfully merged"
                      : open
                        ? "Merge pull request"
                        : "Pull request closed"}
                  </h3>
                  <p className="pr-muted">
                    {data.pr.merged
                      ? "These changes have landed."
                      : data.pr.draft
                        ? "This is a draft. Mark it ready for review before merging."
                        : `Merge status: ${data.pr.mergeable_state}. GitHub enforces branch protections and repository permissions.`}
                  </p>
                  {open && (
                    <>
                      <button
                        className="btn"
                        disabled={busy || loading}
                        onClick={() =>
                          void act(data.pr.draft ? "ready" : "draft")
                        }
                      >
                        {data.pr.draft
                          ? "Ready for review"
                          : "Convert to draft"}
                      </button>
                      <label htmlFor="merge-method">Merge method</label>
                      <select
                        id="merge-method"
                        value={strategy}
                        onChange={(e) =>
                          setStrategy(e.target.value as typeof strategy)
                        }
                        disabled={busy}
                      >
                        <option value="squash">Squash and merge</option>
                        <option value="merge">Create a merge commit</option>
                        <option value="rebase">Rebase and merge</option>
                      </select>
                      <button
                        className="btn primary"
                        disabled={blocked || busy || loading}
                        onClick={() => setConfirm("merge")}
                      >
                        Merge pull request…
                      </button>
                      <button
                        className="btn"
                        disabled={busy || loading}
                        onClick={() => setConfirm("close")}
                      >
                        Close pull request…
                      </button>
                    </>
                  )}
                  {!open && !data.pr.merged && (
                    <button
                      className="btn"
                      disabled={busy || loading}
                      onClick={() => void act("reopen")}
                    >
                      Reopen pull request
                    </button>
                  )}
                  {confirm && (
                    <div
                      className="pr-confirm"
                      role="group"
                      aria-label="Confirm PR action"
                    >
                      <p>
                        {confirm === "merge"
                          ? `Merge ${data.pr.head.ref} into ${data.pr.base.ref} using ${strategy}?`
                          : "Close this pull request without merging?"}
                      </p>
                      <button
                        className="btn primary"
                        disabled={
                          busy || loading || (confirm === "merge" && blocked)
                        }
                        onClick={() => void act(confirm)}
                      >
                        {busy ? "Working…" : `Confirm ${confirm}`}
                      </button>
                      <button
                        className="btn"
                        disabled={busy}
                        onClick={() => setConfirm(null)}
                      >
                        Cancel
                      </button>
                    </div>
                  )}
                </section>
                <section className="pr-box pr-compose">
                  <h3>Reviewers</h3>
                  {data.pr.requested_reviewers.length ? (
                    data.pr.requested_reviewers.map((user) => (
                      <span key={user.login}>@{user.login}</span>
                    ))
                  ) : (
                    <p className="pr-muted">No reviewers requested</p>
                  )}
                  {open && (
                    <>
                      <input
                        aria-label="Request reviewers"
                        placeholder="GitHub usernames, comma separated"
                        value={reviewers}
                        onChange={(e) => setReviewers(e.target.value)}
                        disabled={busy}
                      />
                      <button
                        className="btn"
                        disabled={busy || loading || !reviewers.trim()}
                        onClick={() => void act("reviewers")}
                      >
                        Request review
                      </button>
                    </>
                  )}
                  <h3>Labels</h3>
                  <div className="pr-meta">
                    {data.pr.labels.length ? (
                      data.pr.labels.map((label) => (
                        <span className="tagpill" key={label.name}>
                          {label.name}
                        </span>
                      ))
                    ) : (
                      <span className="pr-muted">None</span>
                    )}
                  </div>
                  {pr.threadId && (
                    <button
                      className="btn"
                      onClick={() => onOpenThread(pr.threadId!)}
                    >
                      Open coding session →
                    </button>
                  )}
                </section>
              </aside>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function diffLines(patch: string) {
  let left = 0,
    right = 0;
  return patch.replace(/\n$/, "").split("\n").map((text) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) {
      left = Number(hunk[1]);
      right = Number(hunk[2]);
      return { text, kind: "hunk", left: null, right: null };
    }
    if (text.startsWith("\\"))
      return { text, kind: "hunk", left: null, right: null };
    if (text.startsWith("+"))
      return { text, kind: "add", left: null, right: right++ };
    if (text.startsWith("-"))
      return { text, kind: "remove", left: left++, right: null };
    return { text, kind: "context", left: left++, right: right++ };
  });
}
function DiffFile({
  file,
  viewed,
  onViewed,
  onLine,
}: {
  file: PrFile;
  viewed: boolean;
  onViewed: () => void;
  onLine?: (line: LineTarget) => void;
}) {
  return (
    <details className="pr-box pr-diff" open={!viewed}>
      <summary>
        <code>{file.filename}</code>
        <span className="pr-added">+{file.additions}</span>
        <span className="pr-removed">−{file.deletions}</span>
        <label onClick={(e) => e.stopPropagation()}>
          <input type="checkbox" checked={viewed} onChange={onViewed} /> Viewed
        </label>
      </summary>
      {file.previous_filename && (
        <p className="pr-file-label">Renamed from {file.previous_filename}</p>
      )}
      {file.patch ? (
        <div className="pr-diff-scroll">
          {diffLines(file.patch).map((line, i) => (
            <div className={`pr-diff-line ${line.kind}`} key={i}>
              <span>{line.left}</span>
              <span>{line.right}</span>
              <button
                disabled={!onLine || line.kind === "hunk"}
                aria-label={`Comment on ${file.filename} line ${line.right ?? line.left}`}
                onClick={() =>
                  onLine?.({
                    path: file.filename,
                    line: (line.right ?? line.left)!,
                    side: line.right === null ? "LEFT" : "RIGHT",
                  })
                }
              >
                +
              </button>
              <code>{line.text}</code>
            </div>
          ))}
        </div>
      ) : (
        <p className="pr-prose">
          Binary file or diff unavailable.{" "}
          <a href={file.blob_url} target="_blank" rel="noreferrer">
            View file on GitHub ↗
          </a>
        </p>
      )}
    </details>
  );
}
