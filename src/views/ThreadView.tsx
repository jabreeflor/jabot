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
//! and wires the send box to `session/prompt` and Stop to `session/cancel`
//! (#14). The split is what lets the shell keep rendering fixtures before a
//! host has answered, and what keeps the chat testable without one.

import { Conversation } from "../components/Conversation";
import { HarnessChip } from "../components/HarnessChip";
import { HostPicker } from "../components/HostPicker";
import { CodeSessionIcon } from "../components/Icon";
import { threadStatus, type ThreadStatus } from "../components/status";
import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";
import type { HostClient } from "../host";
import { streamStatus, useThreadTranscript } from "./transcript";

export function ThreadView({
  thread,
  harnesses,
  host,
  items,
  onSend,
  onAction,
  onPickHost,
  status,
  busy,
  queued,
  onCancel,
  error,
}: {
  thread: ThreadSummary;
  harnesses: readonly HarnessCard[];
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onPickHost?: (hostId: string) => void;
  /** Live status from the stream, when there is one. Falls back to the row's
      own run state — which is all a fixture-backed thread has. */
  status?: ThreadStatus;
  busy?: boolean;
  queued?: readonly string[];
  onCancel?: () => void;
  error?: string | null;
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
}: {
  client: HostClient;
  thread: ThreadSummary;
  harnesses: readonly HarnessCard[];
  host: HostTarget;
  onPickHost?: (hostId: string) => void;
}) {
  const { stream, error, send, cancel } = useThreadTranscript(
    client,
    thread.id,
  );

  return (
    <ThreadView
      thread={thread}
      harnesses={harnesses}
      host={host}
      items={stream.items}
      onSend={send}
      onPickHost={onPickHost}
      status={streamStatus(stream, threadStatus(thread))}
      busy={stream.busy}
      queued={stream.queued}
      onCancel={cancel}
      error={error}
    />
  );
}
