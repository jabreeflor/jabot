//! What you are actually answering (#29).
//!
//! `InboxScreen` has had an `onOpen(threadId)` since #29 and nothing ever
//! called it: tapping a card title was a dead callback, and
//! `MobileSession.transcript()` was exercised only by a scope test asserting
//! it throws. So the phone could tell you an agent was blocked on a question
//! and give you no way to see what it had been doing.
//!
//! Presentational, in the same spirit as `InboxScreen`: props in, callbacks
//! out, no protocol knowledge. The fetching lives in [`useThreadTranscript`]
//! below, which is what lets the screen be tested without a transport.
//!
//! **One reducer, not two.** The items come from `views/transcript.ts`'s
//! `hydrate` — the same pure function the desktop replays through, and the one
//! whose own docs say a second copy is exactly the drift to avoid. What is
//! mobile here is the *rendering*: bubbles, one line per tool call, and the
//! truncation said out loud.
//!
//! **The window is stated, not implied.** `transcript()`'s doc says "enough of
//! the thread to know what you are answering — never more", and it takes the
//! last 40 events. A screen that silently began mid-sentence would let
//! somebody answer a permission on a conversation whose start they think they
//! have read.

import { useEffect, useState } from "react";

import type { MobileSession } from "./session";
import type { TranscriptItem } from "../components/types";
import { applyAcpEvent, EMPTY_STREAM, hydrate } from "../views/transcript";
import "./mobile.css";

export interface TranscriptScreenProps {
  /** What the Inbox card said, so the header is not a bare id. */
  title?: string;
  items: readonly TranscriptItem[];
  /** The host had more than the window held. */
  truncated: boolean;
  /** Prompts the host is holding behind the turn in flight. */
  queued?: readonly string[];
  loading?: boolean;
  error?: string | null;
  onBack(): void;
}

export function TranscriptScreen({
  title,
  items,
  truncated,
  queued = [],
  loading,
  error,
  onBack,
}: TranscriptScreenProps) {
  return (
    <div className="jm" aria-label="Transcript" role="region">
      <div className="jm-top">
        <button type="button" className="jm-back" onClick={onBack}>
          ‹ Inbox
        </button>
        <h1 className="jm-thread-title">{title ?? "Thread"}</h1>
      </div>

      {error && (
        <p className="jm-error" role="alert">
          {error}
        </p>
      )}

      {loading && !error && items.length === 0 && (
        <div className="jm-empty">Reading the thread…</div>
      )}

      {!loading && !error && items.length === 0 && (
        <div className="jm-empty">Nothing has been said in this thread yet.</div>
      )}

      {/* Said out loud rather than implied: this is a window onto the end of
          the conversation, and somebody about to answer a permission should
          know they have not read the start. */}
      {truncated && (
        <p className="jm-truncated">
          Showing the end of this thread — the rest is on your Mac.
        </p>
      )}

      <ol className="jm-lines">
        {items.map((item) => (
          <Line key={item.id} item={item} />
        ))}
      </ol>

      {queued.length > 0 && (
        <p className="jm-queued">
          {queued.length} prompt{queued.length === 1 ? "" : "s"} waiting behind
          this turn.
        </p>
      )}
    </div>
  );
}

function Line({ item }: { item: TranscriptItem }) {
  switch (item.kind) {
    case "user":
      return <li className="jm-line user">{item.text}</li>;
    case "agent":
      return <li className="jm-line agent">{item.text}</li>;
    case "tool":
      // One line each, and the target rather than the output: a phone is not
      // where somebody reads a diff, and a screen that tried would bury the
      // sentence the question is actually about.
      return (
        <li className={`jm-line tool ${item.call.status}`}>
          <span className="jm-tool-kind">{item.call.kind}</span>
          <span className="jm-tool-target">{item.call.target}</span>
          {item.call.note && <span className="jm-tool-note">{item.call.note}</span>}
        </li>
      );
    case "notice":
      return (
        <li className="jm-line notice">
          <strong>{item.title}</strong>
          {item.body && <span>{item.body}</span>}
        </li>
      );
    // `stamp` and `sys` are chrome the desktop draws between messages. On a
    // phone they are noise between the two things worth reading.
    default:
      return null;
  }
}

export interface ThreadTranscript {
  items: readonly TranscriptItem[];
  truncated: boolean;
  queued: readonly string[];
  loading: boolean;
  error: string | null;
}

/**
 * Read one thread, once, for the screen above.
 *
 * `null` for `threadId` means no thread is open, and nothing is asked — which
 * is what lets the shell mount this hook unconditionally.
 *
 * No live stream. `session/update` for an open thread would need the replay
 * and the notifications de-duplicated against each other, and #101 is where
 * that belongs; a screen you opened to read what you are answering is
 * correct as a snapshot, and a wrong one would be worse than a still one.
 */
/**
 * Read one thread, and then follow it.
 *
 * `null` for `threadId` means no thread is open, and nothing is asked — which
 * is what lets the shell mount this hook unconditionally.
 *
 * The replay and the live stream go through the *same* reducer, which is what
 * makes following safe: `applyAcpEvent` drops anything at or below the seq the
 * hydrate reached, so a frame that arrives while the read is in flight cannot
 * be drawn twice. That de-duplication is the reducer's, deliberately, because
 * it is the only thing that knows how far the replay got.
 */
export function useThreadTranscript(
  session:
    | (Pick<MobileSession, "transcript"> & Partial<Pick<MobileSession, "watchThread">>)
    | null,
  threadId: string | null,
): ThreadTranscript {
  const [state, setState] = useState<ThreadTranscript>({
    items: [],
    truncated: false,
    queued: [],
    loading: false,
    error: null,
  });

  useEffect(() => {
    if (!session || !threadId) {
      setState({ items: [], truncated: false, queued: [], loading: false, error: null });
      return;
    }
    let cancelled = false;
    let stream = EMPTY_STREAM;
    // Buffered until the replay lands. A chunk that arrived first is not
    // dropped — it is folded in afterwards, where the reducer can compare it
    // against the head the replay established.
    let pending: Array<{ payload: unknown; seq: number }> = [];
    let hydrated = false;

    // A property of the *replay*, not of the stream: a live chunk arriving
    // afterwards does not make the window any less of a window.
    let truncated = false;

    const draw = () =>
      setState({
        items: stream.items,
        truncated,
        queued: stream.queued,
        loading: false,
        error: null,
      });

    const fold = (payload: unknown, seq: number) => {
      stream = applyAcpEvent(stream, payload, seq);
      draw();
    };

    const stop = session.watchThread?.(threadId, (update) => {
      if (cancelled) return;
      if (!hydrated) {
        pending.push({ payload: update.acp, seq: update.transcriptSeq ?? 0 });
        return;
      }
      fold(update.acp, update.transcriptSeq ?? 0);
    });

    setState((prev) => ({ ...prev, loading: true, error: null }));
    session
      .transcript(threadId)
      .then((result) => {
        if (cancelled) return;
        stream = hydrate(result, EMPTY_STREAM);
        truncated = Boolean(result.truncated);
        hydrated = true;
        for (const held of pending) fold(held.payload, held.seq);
        pending = [];
        draw();
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setState({
          items: [],
          truncated: false,
          queued: [],
          loading: false,
          error: err instanceof Error ? err.message : String(err),
        });
      });

    return () => {
      cancelled = true;
      stop?.();
    };
  }, [session, threadId]);

  return state;
}
