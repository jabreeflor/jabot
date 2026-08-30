//! A code thread — one job in one repo, wrapped in the same chat surface as a
//! bot conversation.
//!
//! The header answers the three questions a running session raises: what am I
//! doing, what engine is doing it, and where has it got to. The harness comes
//! from the *thread*, not from the Code bot, because New Chat can override it
//! per thread (#6).
//!
//! [`ThreadView`] stays presentational — items in, callbacks out — and
//! [`LiveThreadView`] is the one that talks to the host: it hydrates the
//! transcript from SQLite, folds `session/update` into it as the turn runs,
//! wires the send box to `session/prompt` and Stop to `session/cancel` (#14),
//! and answers the permission cards the broker raises on it (#20). The split
//! is what lets the shell keep rendering fixtures before a host has answered,
//! and what keeps the chat testable without one.

import { useCallback, useEffect, useState } from "react";

import { Conversation } from "../components/Conversation";
import { canFold, FoldButton } from "../components/FoldButton";
import { HarnessChip } from "../components/HarnessChip";
import { HostPicker } from "../components/HostPicker";
import { BranchIcon, CodeSessionIcon } from "../components/Icon";
import { threadStatus, type ThreadStatus } from "../components/status";
import type {
  FoldPolicy,
  HarnessCard,
  HostTarget,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";
import type {
  HandoffView,
  HostClient,
  ProcessView,
  ThreadResumeResult,
  ThreadStateResult,
} from "../host";
import { streamStatus, useThreadTranscript } from "./transcript";

export function ThreadView({
  thread,
  harnesses,
  host,
  items,
  onSend,
  onAction,
  onPickHost,
  onFold,
  status,
  busy,
  queued,
  onCancel,
  error,
  handoff,
  drift,
  worktreePath,
  branch,
  detached,
  onResume,
  resuming,
  resumeNotice,
}: {
  thread: ThreadSummary;
  harnesses: readonly HarnessCard[];
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onPickHost?: (hostId: string) => void;
  /** Fold this thread from the chat itself — "Disappear until done" without
      going back to the sidebar to right-click the row you are looking at. */
  onFold?: (policy?: FoldPolicy) => void;
  /** Live status from the stream, when there is one. Falls back to the row's
      own run state — which is all a fixture-backed thread has. */
  status?: ThreadStatus;
  busy?: boolean;
  queued?: readonly string[];
  onCancel?: () => void;
  error?: string | null;
  /** Where this thread's work came from, when a bot sent it rather than the
      human (#24). Absent for the ordinary case: the person started it. */
  handoff?: HandoffView;
  /** Fields that have moved since this thread's ACP session was created, by
      wire name. Non-empty means the next prompt starts a fresh conversation
      rather than continuing this one (#21). */
  drift?: readonly string[];
  /** The host-owned worktree this thread edits in, and the branch it is on
      (#23). Both absent for every thread that works in place — a bot's
      standing thread, a folder that is not a checkout, the "use my own
      checkout" opt-out — and absent again once the tree has been collected. */
  worktreePath?: string;
  branch?: string;
  /** The adapter is gone and the host says the conversation can be put back
      (#21). Absent — or false — draws no button, which is every thread with a
      process still attached and every thread there is nothing to resume. */
  detached?: boolean;
  onResume?: () => void;
  resuming?: boolean;
  /** What the last resume actually did, in the host's own words. */
  resumeNotice?: { tone: "ok" | "warn" | "bad"; text: string } | null;
}) {
  const line = status ?? threadStatus(thread);

  return (
    <Conversation
      header={
        <div className="chat-head">
          <div className="codeav">
            <CodeSessionIcon />
          </div>
          <h2>{thread.title}</h2>
          <HarnessChip harnessId={thread.harnessId} harnesses={harnesses} />
          {worktreePath && (
            <WorktreeChip worktreePath={worktreePath} branch={branch} />
          )}
          <span className="status" data-tone={line.tone}>
            {line.label}
          </span>
          <HostPicker host={host} onPick={onPickHost} />
          {onResume && detached && (
            <button
              type="button"
              className="resume-btn"
              onClick={onResume}
              disabled={resuming}
              title="Put this conversation back where it left off"
            >
              {resuming ? "Resuming…" : "Resume"}
            </button>
          )}
          {onFold && canFold(thread.state) && <FoldButton onFold={onFold} />}
          {handoff && <HandoffLine handoff={handoff} />}
        </div>
      }
      items={items}
      composerPlaceholder={`Message ${thread.title}`}
      onSend={onSend}
      onAction={onAction}
      busy={busy}
      queued={queued}
      onCancel={onCancel}
      error={error}
      /* One slot, and the resume's own word wins it: a `drifted` resume and
         the standing drift notice are the same fact said twice, and the one
         the user just asked for is the one they are waiting to read. */
      notice={
        resumeNotice ? (
          <ResumeNotice notice={resumeNotice} />
        ) : drift && drift.length > 0 ? (
          <DriftNotice drift={drift} />
        ) : null
      }
    />
  );
}

/**
 * What `thread/resume` actually did.
 *
 * The outcome is six answers, not a success and a failure, and each one sends
 * the user somewhere different: `drifted` means the stored session is a
 * different job now, `cwd_missing` means the folder is gone, `unsupported`
 * means this adapter cannot be resumed at all. "Could not resume" with no
 * reason sends every one of them to the wrong fix, so the host's own sentence
 * is what is shown.
 */
function ResumeNotice({
  notice,
}: {
  notice: { tone: "ok" | "warn" | "bad"; text: string };
}) {
  return (
    <div className="chat-resume" role="status" data-tone={notice.tone}>
      {notice.text}
    </div>
  );
}

/**
 * Where this thread is actually editing.
 *
 * A code thread opened in a git folder does not run in the user's checkout: it
 * runs in a host-owned worktree under the app data directory, on a `jabot/<id>`
 * branch (#23). That is the right design — two threads in one repo cannot
 * stand on each other's uncommitted work — and until now nothing on screen
 * said it. Someone looking at a running thread could not tell which directory
 * or which branch the agent was changing, and would go looking for the edits
 * in the wrong tree.
 *
 * The branch is the visible half because it is the one that identifies the
 * work; the path is long, machine-generated and the same prefix for every
 * thread, so it lives in the tooltip where it can be read when it is wanted.
 * A thread whose branch the host did not record still gets the chip, naming
 * the tree's own directory — the point is to say *somewhere else*, and the
 * path alone says that.
 *
 * Nothing at all for a thread that works in place: a bot's standing thread, a
 * folder that is not a checkout, the "use my own checkout" opt-out. A chip on
 * every thread would say nothing about any of them.
 */
function WorktreeChip({
  worktreePath,
  branch,
}: {
  worktreePath: string;
  branch?: string;
}) {
  return (
    <span className="worktree-chip" title={worktreePath}>
      <BranchIcon />
      <span className="ref">{branch ?? basename(worktreePath)}</span>
    </span>
  );
}

/** The last segment of a path, for the fallback label. Trailing separators are
    dropped first so a path that ends in one does not resolve to "". */
function basename(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut === -1 ? trimmed : trimmed.slice(cut + 1);
}

/**
 * The stored session no longer matches the job that would be spawned.
 *
 * The host works this out on every `thread/state` — `resume_readiness` diffs
 * the receipt against what the thread would start as now — and it is the one
 * thing on this screen the user cannot possibly infer. Everything looks
 * normal: the transcript is there, the composer works, and the next message
 * silently opens a *new* conversation the agent has no memory of, because
 * resuming a session whose harness or cwd has moved would be continuing
 * someone else's job.
 *
 * Above the composer rather than in the header, because it is about what
 * happens when you press Enter, not about what this thread is.
 */
function DriftNotice({ drift }: { drift: readonly string[] }) {
  return (
    <div className="chat-drift" role="status">
      <b>This thread's setup has changed</b> — {andList(drift.map(driftLabel))}{" "}
      {drift.length === 1 ? "is" : "are"} not what this conversation was started
      with. Your next message begins a new one.
    </div>
  );
}

/** "a", "a and b", "a, b and c". A comma-joined list reads as a fragment —
    "the engine, the folder are not what..." — and this sentence is the whole
    warning, so it has to be a sentence. */
function andList(parts: readonly string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/** The wire names `receipt::drift` uses, in the words the rest of the UI uses.
    An unknown name is printed as it came rather than dropped: a field the host
    learns to report before this list does is still worth naming. */
function driftLabel(field: string): string {
  switch (field) {
    case "harnessId":
      return "the engine";
    case "model":
      return "the model";
    case "cwd":
      return "the folder";
    case "tools":
      return "its tools";
    case "permissionMode":
      return "the permission mode";
    default:
      return field;
  }
}

/**
 * Who asked for this, when it was not the person reading it.
 *
 * A thread Chief spawned looks exactly like one the human started — same
 * header, same transcript, same everything — and the human coming back to it
 * tomorrow has no way to tell which. The host has always resolved this
 * (`ThreadStateResult.handoff`); nothing ever drew it.
 *
 * `dispatched: false` is the case worth the different tone. A handoff to a bot
 * whose harness is not installed is still a real handoff — the task was sent,
 * the row exists, the thread is here — but nobody heard it, and a line that
 * said only "Handed off by Chief" would be describing work that is not
 * happening. `detail` is the host's own sentence about why.
 */
function HandoffLine({ handoff }: { handoff: HandoffView }) {
  const who = handoff.fromBotName ?? "a bot";
  const verb = handoff.kind === "code_session" ? "Coding job from" : "Handed off by";
  return (
    <p
      className="chat-handoff"
      data-tone={handoff.dispatched ? "note" : "warn"}
      title={handoff.context ?? undefined}
    >
      <span className="from">
        {verb} {who}
      </span>
      {" — "}
      {handoff.task}
      {!handoff.dispatched && (
        <span className="undelivered">
          {handoff.detail ? ` · ${handoff.detail}` : " · nobody picked this up"}
        </span>
      )}
    </p>
  );
}

/**
 * The same view, driven by the host.
 *
 * Keyed on the thread by its caller, so switching threads remounts and the
 * hook starts a fresh hydrate rather than folding one conversation's events
 * into another's.
 */
export function LiveThreadView({
  client,
  thread,
  harnesses,
  host,
  onPickHost,
  onFold,
}: {
  client: HostClient;
  thread: ThreadSummary;
  harnesses: readonly HarnessCard[];
  host: HostTarget;
  onPickHost?: (hostId: string) => void;
  onFold?: (policy?: FoldPolicy) => void;
}) {
  const { stream, error, send, cancel, answer } = useThreadTranscript(
    client,
    thread.id,
  );
  // Fetched once per thread rather than folded into the transcript stream:
  // provenance is a fact about how the thread began, so it cannot change while
  // you are reading it, and re-asking on every `session/update` would be a
  // round trip per streamed chunk. The component is keyed on the thread by its
  // caller, so switching threads remounts and asks again.
  const { facts, applyState } = useThreadFacts(client, thread.id);
  const [resumeNotice, setResumeNotice] = useState<ResumeNoticeLine | null>(
    null,
  );
  const [resuming, setResuming] = useState(false);

  // `thread/reopen` — what the Inbox's Open thread runs — is a store write. It
  // puts the row back and spawns nothing, so after a quit or an idle evict the
  // conversation only came back when the user happened to send another prompt,
  // and until then the pane looked live and was not. `thread/resume` is the
  // explicit verb for reattaching without prompting, and it has been served
  // and typed since #21 with no caller.
  const onResume = useCallback(() => {
    if (typeof client.resumeThread !== "function") return;
    setResuming(true);
    client
      .resumeThread({ threadId: thread.id })
      .then((result) => {
        setResumeNotice(resumeLine(result));
        // The answer carries the thread as it stands afterwards, so the button
        // goes away — or does not — without a second round trip.
        applyState(result.state);
      })
      .catch((err: unknown) => {
        setResumeNotice({
          tone: "bad",
          text: err instanceof Error ? err.message : String(err),
        });
      })
      .finally(() => setResuming(false));
  }, [applyState, client, thread.id]);

  return (
    <ThreadView
      thread={thread}
      harnesses={harnesses}
      host={host}
      items={stream.items}
      onSend={send}
      // The buttons on a permission card are the agent's own ACP options, and
      // this is what carries the one the user pressed back to it (#20).
      onAction={answer}
      onPickHost={onPickHost}
      onFold={onFold}
      status={streamStatus(stream, threadStatus(thread))}
      busy={stream.busy}
      queued={stream.queued}
      onCancel={cancel}
      error={error}
      handoff={facts?.handoff}
      drift={facts?.process?.drift}
      worktreePath={facts?.worktreePath}
      branch={facts?.branch}
      // Both halves, and both from the host: a thread with no adapter that
      // cannot be resumed gets no button, because offering one that can only
      // fail is worse than offering none.
      detached={
        facts?.process
          ? !facts.process.connected && facts.process.resumable
          : false
      }
      onResume={onResume}
      resuming={resuming}
      resumeNotice={resumeNotice}
    />
  );
}

/**
 * `thread/state`, for the facts the transcript does not carry.
 *
 * Swallows its failure on purpose. Provenance is a caption on a conversation
 * that is otherwise entirely readable, so a host that cannot answer should
 * cost the caption and nothing else — the same call that would blank the
 * chat over it is the wrong trade.
 */
interface ThreadFacts {
  handoff?: HandoffView;
  process?: ProcessView;
  worktreePath?: string;
  branch?: string;
}

function factsOf(state: ThreadStateResult): ThreadFacts {
  return {
    handoff: state.handoff,
    process: state.process,
    worktreePath: state.worktreePath,
    branch: state.branch,
  };
}

function useThreadFacts(
  client: HostClient,
  threadId: string,
): {
  facts: ThreadFacts | undefined;
  /** Fold in a `thread/state` somebody else already has — a resume answers
      with one, and asking again for what is in hand is a wasted round trip. */
  applyState: (state: ThreadStateResult) => void;
} {
  const [facts, setFacts] = useState<ThreadFacts | undefined>(undefined);
  const applyState = useCallback(
    (state: ThreadStateResult) => setFacts(factsOf(state)),
    [],
  );
  useEffect(() => {
    let cancelled = false;
    // Method lookup guarded too, the way `useCrew` guards the Doctor: a
    // transport that predates `thread/state` should cost the caption, not
    // throw synchronously out of an effect and take the chat down with it.
    if (typeof client.threadState !== "function") return;
    client
      .threadState({ threadId })
      .then((state) => {
        if (!cancelled) setFacts(factsOf(state));
      })
      .catch(() => {
        // Deliberately silent: see above.
      });
    return () => {
      cancelled = true;
    };
  }, [client, threadId]);
  return { facts, applyState };
}

type ResumeNoticeLine = { tone: "ok" | "warn" | "bad"; text: string };

/**
 * One `ResumeOutcome`, in a sentence.
 *
 * Six answers rather than a success and a failure, and each one sends the user
 * somewhere different — `drifted` to starting a fresh conversation,
 * `cwd_missing` to the folder, `unsupported` to a different engine. A bare
 * "could not resume" sends all three to the wrong fix, so the host's own
 * `detail` is preferred wherever it sent one.
 */
function resumeLine(result: ThreadResumeResult): ResumeNoticeLine {
  switch (result.outcome) {
    case "live":
      return { tone: "ok", text: "Still connected — nothing needed restoring." };
    case "resumed":
      return { tone: "ok", text: "Conversation restored where it left off." };
    // `session/load` means the agent replayed its history to get here, which
    // is a slower and more visible thing than `session/resume` and worth
    // naming: the transcript may have moved under the reader.
    case "loaded":
      return {
        tone: "ok",
        text: "Conversation restored — the agent replayed its history.",
      };
    case "drifted":
      return {
        tone: "warn",
        text: `${sentence(
          result.detail ?? "This thread's setup has changed.",
        )}${driftTail(result.drift)}`,
      };
    default:
      return {
        tone: "bad",
        text: result.detail ?? `Could not resume: ${result.outcome}.`,
      };
  }
}

/** The moved fields appended to a `drifted` detail, in the words the drift
    notice uses. Nothing at all when the host named none.

    Its own sentence, capitalised: the host's `detail` is a sentence and this
    follows it, so "…matches this thread. the engine and the folder have
    moved." reads as a typo rather than as two facts. */
function driftTail(drift?: readonly string[]): string {
  if (!drift || drift.length === 0) return "";
  const names = andList(drift.map(driftLabel));
  return ` ${names.charAt(0).toUpperCase()}${names.slice(1)} ${
    drift.length === 1 ? "has" : "have"
  } moved.`;
}

/** The host's own sentence, ended if it was not. Anything appended after an
    unpunctuated one would run the two together. */
function sentence(text: string): string {
  const trimmed = text.trim();
  return /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
}
