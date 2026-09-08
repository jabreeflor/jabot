//! Compact Code-conversation popover: repos, Git state, and sources (#269).
//!
//! The header already names the job and the engine. This is the rest of the
//! location — which checkout the agent is editing, whether it is dirty, and
//! which files the person attached — so Git actions have a place to live
//! that is obviously about *this* repository, not a sibling.

import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useId, useRef, useState } from "react";

import type { HostClient } from "../host";
import type {
  ThreadGitDiffResult,
  ThreadRepoChoice,
  ThreadRepoSummary,
  ThreadSourceView,
  ThreadSummaryResult,
} from "../host/protocol";
import { FieldLabel, Modal } from "./Modal";
import {
  BranchIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CompareIcon,
  DiffIcon,
  FileIcon,
  MonitorIcon,
  PlusIcon,
  UploadIcon,
} from "./Icon";

export function ConversationSummary({
  client,
  threadId,
  onOpenPullRequest,
}: {
  client?: HostClient;
  threadId: string;
  onOpenPullRequest?: (url?: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [summary, setSummary] = useState<ThreadSummaryResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [review, setReview] = useState<ThreadGitDiffResult | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [commitOpen, setCommitOpen] = useState(false);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const popoverId = useId();

  const load = useCallback(async () => {
    if (!client || typeof client.threadSummary !== "function") {
      setError("Summary is unavailable on this host.");
      setSummary(null);
      return;
    }
    setLoading(true);
    try {
      const next = await client.threadSummary({ threadId });
      setSummary(next);
      setError(null);
      setSelectedId((current) => current ?? next.selectedRepoId);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [client, threadId]);

  useEffect(() => {
    if (!open) return;
    void load();
  }, [load, open]);

  useEffect(() => {
    if (!open || !client?.onNotification) return;
    return client.onNotification((note) => {
      if (note.method === "session/update") void load();
    });
  }, [client, load, open]);

  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => {
      void load();
    }, 4000);
    return () => window.clearInterval(timer);
  }, [load, open]);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setPickerOpen(false);
      }
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.stopPropagation();
        if (pickerOpen) setPickerOpen(false);
        else setOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, pickerOpen]);

  const selected =
    summary?.repositories.find((repo) => repo.id === selectedId) ??
    summary?.repositories.find((repo) => repo.primary) ??
    summary?.repositories[0];
  const extras = (summary?.repositories ?? []).filter(
    (repo) => repo.id !== selected?.id,
  );
  const triggerLabel = selected?.name ?? "Repositories";

  return (
    <div className="summary-control" ref={rootRef}>
      <button
        type="button"
        className="summary-trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? popoverId : undefined}
        aria-label={`Conversation summary for ${triggerLabel}`}
        onClick={() => setOpen((was) => !was)}
      >
        <span className="name">{triggerLabel}</span>
        <ChevronDownIcon />
      </button>
      {open && (
        <div
          className="summary-popover"
          id={popoverId}
          role="dialog"
          aria-label="Conversation summary"
        >
          {loading && !summary ? (
            <p className="summary-status" role="status">
              Loading repositories…
            </p>
          ) : error && !summary ? (
            <p className="summary-status" role="status">
              {error}
            </p>
          ) : !selected ? (
            <p className="summary-status" role="status">
              No repository is attached to this conversation.
            </p>
          ) : (
            <RepoPanel
              selected={selected}
              extras={extras}
              available={summary?.availableFolders ?? []}
              sources={summary?.sources ?? []}
              pickerOpen={pickerOpen}
              onTogglePicker={() => setPickerOpen((was) => !was)}
              onSelectRepo={(id) => {
                setSelectedId(id);
                setPickerOpen(false);
              }}
              onAttach={(folderId) => {
                void (async () => {
                  if (!client) return;
                  const next = await client.attachThreadRepo({
                    threadId,
                    folderId,
                  });
                  setSummary(next);
                  setSelectedId(folderId);
                  setPickerOpen(false);
                })();
              }}
              onInspect={() => {
                void (async () => {
                  if (!client) return;
                  setReviewing(true);
                  try {
                    setReview(
                      await client.threadGitDiff({
                        threadId,
                        repoId: selected.id,
                      }),
                    );
                    setOpen(false);
                  } finally {
                    setReviewing(false);
                  }
                })();
              }}
              onCommit={() => {
                setCommitOpen(true);
                setOpen(false);
              }}
              onCompare={() => {
                if (selected.compareUrl) {
                  window.open(selected.compareUrl, "_blank", "noopener");
                }
                if (selected.pullRequestUrl && onOpenPullRequest) {
                  onOpenPullRequest(selected.pullRequestUrl);
                }
                void (async () => {
                  if (!client || !selected.isGit) return;
                  setReview(
                    await client.threadGitDiff({
                      threadId,
                      repoId: selected.id,
                    }),
                  );
                  setOpen(false);
                })();
              }}
              onAddSource={() => {
                void (async () => {
                  if (!client) return;
                  const paths = await pickSourcePaths();
                  let next = summary;
                  for (const path of paths) {
                    next = await client.addThreadSource({ threadId, path });
                  }
                  if (next) setSummary(next);
                })();
              }}
              onOpenSource={(source) => {
                void client?.openThreadSource({
                  threadId,
                  sourceId: source.id,
                });
              }}
              onViewAll={() => {
                setSourcesOpen(true);
                setOpen(false);
              }}
              busy={reviewing}
            />
          )}
        </div>
      )}
      {review && (
        <GitReviewModal
          repoName={selected?.name ?? "repository"}
          diff={review}
          onClose={() => setReview(null)}
          onOpenPr={
            selected?.pullRequestUrl && onOpenPullRequest
              ? () => onOpenPullRequest(selected.pullRequestUrl)
              : undefined
          }
        />
      )}
      {commitOpen && selected && client && (
        <GitCommitModal
          client={client}
          threadId={threadId}
          repo={selected}
          onClose={() => setCommitOpen(false)}
          onDone={(next) => {
            setSummary(next);
            setCommitOpen(false);
          }}
        />
      )}
      {sourcesOpen && (
        <SourcesModal
          sources={summary?.sources ?? []}
          onClose={() => setSourcesOpen(false)}
          onOpen={(source) => {
            void client?.openThreadSource({ threadId, sourceId: source.id });
          }}
        />
      )}
    </div>
  );
}

function RepoPanel({
  selected,
  extras,
  available,
  sources,
  pickerOpen,
  onTogglePicker,
  onSelectRepo,
  onAttach,
  onInspect,
  onCommit,
  onCompare,
  onAddSource,
  onOpenSource,
  onViewAll,
  busy,
}: {
  selected: ThreadRepoSummary;
  extras: readonly ThreadRepoSummary[];
  available: readonly ThreadRepoChoice[];
  sources: readonly ThreadSourceView[];
  pickerOpen: boolean;
  onTogglePicker: () => void;
  onSelectRepo: (id: string) => void;
  onAttach: (folderId: string) => void;
  onInspect: () => void;
  onCommit: () => void;
  onCompare: () => void;
  onAddSource: () => void;
  onOpenSource: (source: ThreadSourceView) => void;
  onViewAll: () => void;
  busy: boolean;
}) {
  const gitReady = selected.isGit && selected.available && selected.status === "ok";
  return (
    <>
      <div className="summary-repo-head">
        <button
          type="button"
          className="summary-repo-select"
          aria-haspopup="listbox"
          aria-expanded={pickerOpen}
          aria-label={`Selected repository ${selected.name}`}
          onClick={onTogglePicker}
        >
          {selected.name}
          <ChevronDownIcon />
        </button>
      </div>
      {pickerOpen && (
        <ul className="summary-repo-menu" role="listbox" aria-label="Repositories">
          <li>
            <button
              type="button"
              role="option"
              aria-current="true"
              onClick={() => onSelectRepo(selected.id)}
            >
              {selected.name}
              {selected.primary ? " · this conversation" : ""}
            </button>
          </li>
          {extras.map((repo) => (
            <li key={repo.id}>
              <button
                type="button"
                role="option"
                onClick={() => onSelectRepo(repo.id)}
              >
                {repo.name}
              </button>
            </li>
          ))}
          {available.map((folder) => (
            <li key={folder.folderId}>
              <button
                type="button"
                role="option"
                onClick={() => onAttach(folder.folderId)}
              >
                Add {folder.name}
              </button>
            </li>
          ))}
        </ul>
      )}
      <RepoBody
        repo={selected}
        gitReady={gitReady}
        busy={busy}
        onInspect={onInspect}
        onCommit={onCommit}
        onCompare={onCompare}
      />
      {extras.map((repo) => (
        <button
          key={repo.id}
          type="button"
          className="summary-extra"
          aria-label={`${repo.name}, ${countLabel(repo)}`}
          onClick={() => onSelectRepo(repo.id)}
        >
          <ChevronRightIcon />
          <span className="label">{repo.name}</span>
          <Counts repo={repo} />
        </button>
      ))}
      <SourcesStrip
        sources={sources}
        onAdd={onAddSource}
        onOpen={onOpenSource}
        onViewAll={onViewAll}
      />
    </>
  );
}

function RepoBody({
  repo,
  gitReady,
  busy,
  onInspect,
  onCommit,
  onCompare,
}: {
  repo: ThreadRepoSummary;
  gitReady: boolean;
  busy: boolean;
  onInspect: () => void;
  onCommit: () => void;
  onCompare: () => void;
}) {
  return (
    <>
      <button
        type="button"
        className="summary-changes"
        aria-label={`Changes in ${repo.name}: ${countLabel(repo)}`}
        disabled={!gitReady}
        onClick={onInspect}
      >
        Changes
        <Counts repo={repo} />
      </button>
      <div className="summary-meta">
        <span className="summary-chip">
          <MonitorIcon />
          {repo.environment}
        </span>
        {repo.branch && (
          <span className="summary-chip" title={repo.path}>
            <BranchIcon />
            <span className="ref">{repo.branch}</span>
          </span>
        )}
      </div>
      {repo.status === "not_git" && (
        <p className="summary-status" role="status">
          {repo.name} is not a Git repository.
        </p>
      )}
      {repo.status === "unavailable" && (
        <p className="summary-status" role="status">
          {repo.name} is not available on disk.
        </p>
      )}
      {repo.status === "empty" && (
        <p className="summary-status" role="status">
          Git state is not available yet.
        </p>
      )}
      <div className="summary-actions">
        <button
          type="button"
          className="summary-action"
          disabled={!gitReady || busy}
          onClick={onInspect}
        >
          <DiffIcon />
          Inspect changes
        </button>
        <button
          type="button"
          className="summary-action"
          disabled={!gitReady}
          onClick={onCommit}
        >
          <UploadIcon />
          Commit or push
        </button>
        <button
          type="button"
          className="summary-action"
          disabled={!gitReady && !repo.compareUrl}
          onClick={onCompare}
        >
          <CompareIcon />
          Compare branch
        </button>
      </div>
    </>
  );
}

function Counts({ repo }: { repo: ThreadRepoSummary }) {
  if (!repo.isGit || repo.additions == null || repo.deletions == null) {
    return <span className="summary-counts">—</span>;
  }
  return (
    <span className="summary-counts" aria-hidden="true">
      <span className="add">+{repo.additions}</span>
      <span className="del">−{repo.deletions}</span>
    </span>
  );
}

function countLabel(repo: ThreadRepoSummary): string {
  if (!repo.isGit || repo.additions == null || repo.deletions == null) {
    return "no change counts";
  }
  return `+${repo.additions} −${repo.deletions}`;
}

function SourcesStrip({
  sources,
  onAdd,
  onOpen,
  onViewAll,
}: {
  sources: readonly ThreadSourceView[];
  onAdd: () => void;
  onOpen: (source: ThreadSourceView) => void;
  onViewAll: () => void;
}) {
  const visible = sources.slice(0, 4);
  return (
    <section className="summary-sources" aria-label="Sources">
      <h3>Sources</h3>
      <div className="summary-source-row">
        {visible.map((source) => (
          <button
            key={source.id}
            type="button"
            className="summary-source"
            aria-label={`Open ${source.name}`}
            title={source.path}
            onClick={() => onOpen(source)}
          >
            {source.kind === "image" ? (
              <img alt="" src={filePreview(source.path)} />
            ) : (
              <FileIcon />
            )}
          </button>
        ))}
        <button
          type="button"
          className="summary-add-source"
          aria-label="Add source"
          onClick={onAdd}
        >
          <PlusIcon />
        </button>
        <button
          type="button"
          className="summary-view-all"
          onClick={onViewAll}
        >
          View all
        </button>
      </div>
    </section>
  );
}

function GitReviewModal({
  repoName,
  diff,
  onClose,
  onOpenPr,
}: {
  repoName: string;
  diff: ThreadGitDiffResult;
  onClose: () => void;
  onOpenPr?: () => void;
}) {
  return (
    <Modal title={`Changes in ${repoName}`} onClose={onClose}>
      <p className="summary-status">
        <span className="summary-counts">
          <span className="add">+{diff.additions}</span>{" "}
          <span className="del">−{diff.deletions}</span>
        </span>
      </p>
      {diff.files.length === 0 ? (
        <p className="summary-status">No changes in this repository.</p>
      ) : (
        <div className="summary-review-files">
          {diff.files.map((file) => (
            <div key={file.path} className="summary-review-file">
              <code>{file.path}</code>
              <span className="summary-counts">
                <span className="add">+{file.additions}</span>
                <span className="del">−{file.deletions}</span>
              </span>
            </div>
          ))}
        </div>
      )}
      {diff.patch && <pre className="summary-review-patch">{diff.patch}</pre>}
      {onOpenPr && (
        <div className="summary-commit-actions">
          <button type="button" className="btn" onClick={onOpenPr}>
            Open pull request
          </button>
        </div>
      )}
    </Modal>
  );
}

function GitCommitModal({
  client,
  threadId,
  repo,
  onClose,
  onDone,
}: {
  client: HostClient;
  threadId: string;
  repo: ThreadRepoSummary;
  onClose: () => void;
  onDone: (summary: ThreadSummaryResult) => void;
}) {
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function commit() {
    const trimmed = message.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      const next = await client.threadGitCommit({
        threadId,
        repoId: repo.id,
        message: trimmed,
      });
      onDone(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function push() {
    setBusy(true);
    setError(null);
    try {
      const result = await client.threadGitPush({
        threadId,
        repoId: repo.id,
      });
      if (!result.ok) {
        setError(result.detail ?? "Push failed.");
        return;
      }
      onDone(await client.threadSummary({ threadId }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={`Commit or push ${repo.name}`} onClose={onClose}>
      <FieldLabel htmlFor="summary-commit-message">Commit message</FieldLabel>
      <form
        className="summary-commit-form"
        onSubmit={(event) => {
          event.preventDefault();
          void commit();
        }}
      >
        <textarea
          id="summary-commit-message"
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          placeholder="Describe the changes"
        />
        {error && (
          <p className="summary-status" role="alert">
            {error}
          </p>
        )}
        <div className="summary-commit-actions">
          <button type="submit" className="btn" disabled={busy || !message.trim()}>
            Commit
          </button>
          <button type="button" className="btn" disabled={busy} onClick={() => void push()}>
            Push
          </button>
        </div>
      </form>
    </Modal>
  );
}

function SourcesModal({
  sources,
  onClose,
  onOpen,
}: {
  sources: readonly ThreadSourceView[];
  onClose: () => void;
  onOpen: (source: ThreadSourceView) => void;
}) {
  return (
    <Modal title="Sources" onClose={onClose}>
      {sources.length === 0 ? (
        <p className="summary-status">No sources attached yet.</p>
      ) : (
        <div className="summary-source-list">
          {sources.map((source) => (
            <button
              key={source.id}
              type="button"
              onClick={() => onOpen(source)}
            >
              <FileIcon />
              <span>
                {source.name}
                {!source.available ? " · missing" : ""}
              </span>
            </button>
          ))}
        </div>
      )}
    </Modal>
  );
}

async function pickSourcePaths(): Promise<string[]> {
  try {
    return await invoke<string[]>("pick_sources");
  } catch {
    return [];
  }
}

function filePreview(path: string): string {
  if (path.startsWith("data:") || path.startsWith("http")) return path;
  return `file://${path}`;
}
