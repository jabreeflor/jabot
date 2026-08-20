//! Scrolling transcript + composer, under whatever header the view supplies.
//!
//! A bot chat and a code thread are the same conversation with a different
//! nameplate — one is a standing thread with a persona, the other a job in a
//! repo — so the body is one component and only the header differs.
//!
//! Between the two sits the one piece of chrome #14 added: the strip that says
//! what you typed while the agent was busy is *waiting*, not lost and not
//! delivered. ACP cannot inject a message mid-turn, so a follow-up is held
//! until the turn ends — and a UI that took the text and then said nothing
//! would be indistinguishable from one that dropped it.

import { useEffect, useRef, type ReactNode } from "react";

import { Composer } from "./Composer";
import { Transcript } from "./Transcript";
import type { TranscriptItem } from "./types";

export function Conversation({
  header,
  items,
  composerPlaceholder,
  onSend,
  onAction,
  busy = false,
  queued,
  onCancel,
  error,
}: {
  header: ReactNode;
  items: readonly TranscriptItem[];
  composerPlaceholder: string;
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  /** A turn is in flight. */
  busy?: boolean;
  /** Prompts the host is holding until it ends, oldest first (#14). */
  queued?: readonly string[];
  onCancel?: () => void;
  /** The last host error on this thread, shown rather than swallowed. */
  error?: string | null;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // Follow the tail. #14 replaces this with end-anchored virtualization once a
  // transcript is long enough to jank; the behaviour it has to preserve is
  // exactly this one.
  useEffect(() => {
    const scroll = scrollRef.current;
    if (scroll) scroll.scrollTop = scroll.scrollHeight;
  }, [items]);

  const waiting = queued ?? [];

  return (
    <div className="view">
      {header}
      <div className="chat-scroll" ref={scrollRef}>
        <Transcript items={items} onAction={onAction} />
      </div>
      {error && (
        <div className="chat-error" role="alert">
          {error}
        </div>
      )}
      {waiting.length > 0 && (
        <div className="queued" role="status">
          <span className="queued-count">
            {waiting.length === 1
              ? "1 message waiting"
              : `${waiting.length} messages waiting`}
          </span>
          <span className="queued-text">{waiting[0]}</span>
          {onCancel && (
            <button
              type="button"
              className="btn"
              onClick={onCancel}
              title="Stop the turn in flight so this goes now"
            >
              Send now
            </button>
          )}
        </div>
      )}
      <Composer
        placeholder={composerPlaceholder}
        onSend={onSend}
        busy={busy}
        onCancel={onCancel}
      />
    </div>
  );
}
