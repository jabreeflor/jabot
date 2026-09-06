//! New Chat: the empty window that spawns a code thread.
//!
//! This is a main-pane chat surface, not a card. The shape is the one a new
//! conversation actually has — a context row, a large composer, and a couple
//! of starter pills — because picking an engine and a folder used to be a
//! modal that sat *on top of* the chat you were about to have, and sending
//! the first message was a second step on a different screen.
//!
//! Harness is still picked *per thread*, not per bot — Code owns many folder
//! threads and any one of them may run a different engine than Code's default
//! (#6). The folder is the repo the thread will work in; "No folder" is a
//! scratch session with no worktree.
//!
//! A folder thread always gets its own worktree. The Advanced disclosure that
//! offered the checkout opt-out and a base branch (#23, #92) is gone: a fresh
//! worktree per thread is what stops two threads in one repo standing on each
//! other's uncommitted work. `thread/open` still accepts `useCheckout` and
//! `baseRef` — the Rust host honours both and `tests/e2e/worktree.test.ts`
//! drives them — but nothing this window sends sets either, so the base ref
//! is the host's default and the tree is always fresh.

import { useState, type FormEvent, type KeyboardEvent } from "react";

import { HarnessMark } from "./HarnessIcon";
import {
  ArrowUpIcon,
  ChevronDownIcon,
  MicIcon,
  MonitorIcon,
  PlusIcon,
} from "./Icon";
import { Select, type SelectOption } from "./Select";
import { WorkspacePicker, type WorkspaceActions } from "./WorkspacePicker";
import type {
  Folder,
  HarnessCard,
  HostTarget,
  NewChatDraft,
} from "./types";

const UNTITLED = "Untitled session";

const FALLBACK_HOST: HostTarget = {
  hostId: "local",
  name: "This Mac",
  reachable: true,
};

export function NewChatView({
  harnesses,
  folders,
  defaultFolderId = null,
  defaultHarnessId,
  error = null,
  workspaceActions,
  host = FALLBACK_HOST,
  onStart,
}: {
  harnesses: readonly HarnessCard[];
  folders: readonly Folder[];
  defaultFolderId?: string | null;
  defaultHarnessId?: string;
  /** Why the last attempt did not start a session. The window stays holding
      the draft, because a refused spawn is something to fix and retry. */
  error?: string | null;
  workspaceActions?: WorkspaceActions;
  host?: HostTarget;
  onStart: (draft: NewChatDraft) => void | Promise<void>;
}) {
  const [harnessId, setHarnessId] = useState(
    defaultHarnessId ?? harnesses[0]?.id ?? "",
  );
  const [folder, setFolder] = useState(defaultFolderId ?? "");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);

  const selectedHarness = harnesses.find((harness) => harness.id === harnessId);
  const selectedFolder = folders.find((row) => row.id === folder);

  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setWorkspaceError(null);
    try {
      await action();
    } catch (err) {
      setWorkspaceError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  function start(task: string) {
    if (!harnessId) return;
    void run(async () => {
      await onStart({
        harnessId,
        folderId: folder || null,
        task,
      });
    });
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    start(text.trim() || UNTITLED);
  }

  function onComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    start(text.trim() || UNTITLED);
  }

  const workspaceOptions: SelectOption[] = [
    { value: "", label: "No folder" },
    ...folders.map((row) => ({ value: row.id, label: row.name })),
  ];
  const harnessOptions: SelectOption[] = harnesses.map((harness) => ({
    value: harness.id,
    label: harness.label,
    icon: (
      <span className="mselect-mark" style={{ color: harness.accent }}>
        <HarnessMark harnessId={harness.id} />
      </span>
    ),
    detail:
      harness.available === false
        ? (harness.installHint ?? "Not installed")
        : harness.blurb,
  }));

  return (
    <div className="view newchat" role="region" aria-label="New Chat">
      <div className="newchat-stage">
        <div className="newchat-meta">
          <Select
            variant="chip"
            aria-label={`Workspace: ${selectedFolder?.name ?? "No folder"}`}
            value={folder}
            options={workspaceOptions}
            onChange={setFolder}
            disabled={busy}
          />
          <Select
            variant="chip"
            aria-label={`Harness: ${selectedHarness?.label ?? "Harness"}`}
            value={harnessId}
            options={harnessOptions}
            onChange={setHarnessId}
            disabled={busy}
          />
          <button
            type="button"
            className="ctx-chip"
            aria-label={`Host: ${host.name}`}
            title={host.reachable ? host.name : `${host.name} — unreachable`}
          >
            <MonitorIcon />
            <span>{host.name}</span>
            {!host.reachable && <span className="host-error">offline</span>}
            <ChevronDownIcon className="mselect-chev" />
          </button>
        </div>

        <form className="newchat-box" onSubmit={submit}>
          <textarea
            value={text}
            placeholder="Plan, build, or describe a change"
            aria-label="Plan, build, or describe a change"
            disabled={busy}
            autoFocus
            rows={3}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onComposerKeyDown}
          />
          <div className="newchat-toolbar">
            <button
              type="button"
              className="round-btn"
              aria-label="Attach"
              disabled={busy}
            >
              <PlusIcon />
            </button>
            <span className="newchat-toolbar-grow" />
            <button
              type="submit"
              className="round-btn send-hero"
              aria-label={text.trim() ? "Send" : "Start session"}
              disabled={busy || !harnessId}
            >
              {text.trim() ? <ArrowUpIcon /> : <MicIcon />}
            </button>
          </div>
        </form>

        <WorkspacePicker
          onChange={setFolder}
          actions={workspaceActions}
          busy={busy}
          run={run}
        />

        {busy && (
          <p className="workspace-hint" role="status">
            Preparing your workspace…
          </p>
        )}
        {selectedHarness?.available === false && (
          <p className="workspace-hint" role="status">
            {selectedHarness.installHint ?? "Not installed"}
          </p>
        )}

        {(workspaceError || error) && (
          <p className="modal-error" role="alert">
            {workspaceError || error}
          </p>
        )}
      </div>
    </div>
  );
}
