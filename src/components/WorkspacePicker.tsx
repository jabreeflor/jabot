import { useState } from "react";
import { FolderIcon } from "./Icon";
import { FieldLabel } from "./Modal";
import type { Folder } from "./types";

export interface Repository {
  full_name: string;
  description: string | null;
  private: boolean;
}
export interface WorkspaceActions {
  pickFolder: () => Promise<string | null>;
  listRepositories: (page: number) => Promise<Repository[]>;
  pickRepository: (repo: string) => Promise<string>;
  signedIn: boolean;
  signIn: () => void;
}

export function WorkspacePicker({
  folders,
  value,
  onChange,
  actions,
  busy,
  run,
}: {
  folders: readonly Folder[];
  value: string;
  onChange: (id: string) => void;
  actions?: WorkspaceActions;
  busy: boolean;
  run: (action: () => Promise<void>) => Promise<void>;
}) {
  const selected = folders.find((folder) => folder.id === value);
  const [showRepos, setShowRepos] = useState(false);
  const [repos, setRepos] = useState<Repository[]>([]);
  const [page, setPage] = useState(0);
  const [more, setMore] = useState(false);
  const [query, setQuery] = useState("");
  async function load(next: number) {
    if (!actions) return;
    const rows = await actions.listRepositories(next);
    setRepos((old) => (next === 1 ? rows : [...old, ...rows]));
    setPage(next);
    setMore(rows.length === 50);
  }
  return (
    <>
      <FieldLabel>WORKSPACE</FieldLabel>
      <div className="workspace-sources">
        <button
          type="button"
          className="workspace-source"
          disabled={busy || !actions}
          onClick={() =>
            void run(async () => {
              const picked = await actions!.pickFolder();
              if (picked) {
                onChange(picked);
                setShowRepos(false);
              }
            })
          }
        >
          <FolderIcon open />
          <strong>Open folder</strong>
          <span>Choose from your computer</span>
        </button>
        <button
          type="button"
          className="workspace-source"
          disabled={busy || !actions}
          aria-expanded={showRepos}
          onClick={() => {
            if (!actions?.signedIn) {
              actions?.signIn();
              return;
            }
            setShowRepos((old) => !old);
            if (!showRepos && page === 0) void run(() => load(1));
          }}
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
            aria-hidden="true"
          >
            <circle cx="6" cy="5" r="3" />
            <circle cx="6" cy="19" r="3" />
            <circle cx="18" cy="6" r="3" />
            <path d="M6 8v8m12-7c0 6-12 0-12 7" />
          </svg>
          <strong>GitHub repository</strong>
          <span>
            {actions?.signedIn
              ? "Choose a repository to clone"
              : "Connect your GitHub account"}
          </span>
        </button>
      </div>
      {!actions && (
        <p className="workspace-hint">
          Open the desktop app to browse folders or GitHub.
        </p>
      )}
      {showRepos && (
        <div className="workspace-repos">
          <input
            type="text"
            aria-label="Filter repositories"
            placeholder="Filter loaded repositories…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <div className="workspace-repo-list" aria-label="GitHub repositories">
            {repos
              .filter((repo) =>
                repo.full_name.toLowerCase().includes(query.toLowerCase()),
              )
              .map((repo) => (
                <button
                  key={repo.full_name}
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      onChange(await actions!.pickRepository(repo.full_name));
                      setShowRepos(false);
                    })
                  }
                >
                  <strong>{repo.full_name}</strong>
                  <span>
                    {repo.private ? "Private · " : ""}
                    {repo.description ?? "GitHub repository"}
                  </span>
                </button>
              ))}
            {!busy &&
              repos.filter((repo) =>
                repo.full_name.toLowerCase().includes(query.toLowerCase()),
              ).length === 0 && (
                <p className="workspace-hint">No matching repositories.</p>
              )}
          </div>
          {more && (
            <button
              className="btn"
              type="button"
              disabled={busy}
              onClick={() => void run(() => load(page + 1))}
            >
              Load more repositories
            </button>
          )}
          {page === 0 && !busy && (
            <button
              className="btn"
              type="button"
              onClick={() => void run(() => load(1))}
            >
              Retry
            </button>
          )}
        </div>
      )}
      {selected && (
        <div
          className="workspace-selection"
          role="group"
          aria-label="Selected workspace"
        >
          <FolderIcon open />
          <div>
            <strong>{selected.name}</strong>
            <span>{selected.path}</span>
          </div>
          <button
            type="button"
            className="btn"
            disabled={busy}
            aria-label="Remove selected workspace"
            onClick={() => onChange("")}
          >
            Remove
          </button>
        </div>
      )}
    </>
  );
}
