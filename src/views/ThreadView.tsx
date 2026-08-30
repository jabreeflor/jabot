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

import { useEffect, useState } from "react";

import { Conversation } from "../components/Conversation";
import { canFold, FoldButton } from "../components/FoldButton";
import { HarnessChip } from "../components/HarnessChip";
import { HostPicker } from "../components/HostPicker";
import { CodeSessionIcon } from "../components/Icon";
import { threadStatus, type ThreadStatus } from "../components/status";
import type {
  FoldPolicy,
  HarnessCard,
  HostTarget,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";
import type { HandoffView, HostClient } from "../host";
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
          <span className="status" data-tone={line.tone}>
            {line.label}
          </span>
          <HostPicker host={host} onPick={onPickHost} />
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
    />
  );
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
  const handoff = useHandoff(client, thread.id);

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
      handoff={handoff}
    />
  );
}

/**
 * `thread/state`, for the one field the transcript does not carry.
 *
 * Swallows its failure on purpose. Provenance is a caption on a conversation
 * that is otherwise entirely readable, so a host that cannot answer should
 * cost the caption and nothing else — the same call that would blank the
 * chat over it is the wrong trade.
 */
function useHandoff(client: HostClient, threadId: string): HandoffView | undefined {
  const [handoff, setHandoff] = useState<HandoffView | undefined>(undefined);
  useEffect(() => {
    let cancelled = false;
    // Method lookup guarded too, the way `useCrew` guards the Doctor: a
    // transport that predates `thread/state` should cost the caption, not
    // throw synchronously out of an effect and take the chat down with it.
    if (typeof client.threadState !== "function") return;
    client
      .threadState({ threadId })
      .then((state) => {
        if (!cancelled) setHandoff(state.handoff);
      })
      .catch(() => {
        // Deliberately silent: see above.
      });
    return () => {
      cancelled = true;
    };
  }, [client, threadId]);
  return handoff;
}
