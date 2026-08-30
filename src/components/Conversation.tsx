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

import { useEffect, useRef, useState, type ReactNode } from "react";

import { ArrowUpIcon } from "./Icon";
import { Composer } from "./Composer";
import { Transcript } from "./Transcript";
import type { TranscriptItem } from "./types";

/** How close to the bottom still counts as being at it. Sub-pixel rounding and
    a streaming bubble growing between frames both put the exact bottom a few
    pixels out of reach, and a reader who never left should not be treated as
    having done so. */
const STICK_THRESHOLD = 32;

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
  notice,
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
  /** A standing caution about this thread, distinct from `error`: an error is
      something that just failed, a notice is something true about the next
      thing you do. Drawn above the composer because that is where the action
      it is about is taken. */
  notice?: ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  // True while the reader is parked at the end. Default true, because a
  // conversation opens at its tail.
  const stuckRef = useRef(true);
  const [stuck, setStuck] = useState(true);

  // End-anchored, not tail-following.
  //
  // The effect this replaces set `scrollTop = scrollHeight` on every change to
  // `items`, and #14's reducer rebuilds `items` on *every streamed chunk* — so
  // scrolling back through history while an agent was talking was impossible:
  // the view snapped to the bottom a few times a second. That is the live
  // defect here; windowing below is the half the record deferred for
  // performance.
  //
  // Stuck is measured rather than remembered, in one place, so the answer
  // cannot drift from what the element is actually doing.
  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) return;
    if (!stuckRef.current) return;
    scroll.scrollTop = scroll.scrollHeight;
  }, [items]);

  // Sending re-sticks. Somebody who scrolled up to check something and then
  // typed is done reading back — and a reply that arrived off-screen because
  // the view was still held at the old position would be the worse surprise.
  const lastId = items[items.length - 1]?.id;
  useEffect(() => {
    const last = items[items.length - 1];
    if (last?.kind !== "user") return;
    stuckRef.current = true;
    setStuck(true);
    const scroll = scrollRef.current;
    if (scroll) scroll.scrollTop = scroll.scrollHeight;
    // Keyed on the last item's id rather than the array: this must fire when
    // a *new* user item lands, not on every chunk of the reply to it.
  }, [lastId, items]);

  function onScroll() {
    const scroll = scrollRef.current;
    if (!scroll) return;
    // A threshold rather than an equality: sub-pixel rounding, and a streaming
    // bubble that grows between the scroll event and this read, both put the
    // exact bottom a few pixels out of reach.
    const atEnd =
      scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight <=
      STICK_THRESHOLD;
    stuckRef.current = atEnd;
    setStuck(atEnd);
  }

  function jumpToLatest() {
    const scroll = scrollRef.current;
    if (!scroll) return;
    scroll.scrollTop = scroll.scrollHeight;
    stuckRef.current = true;
    setStuck(true);
  }

  const waiting = queued ?? [];

  return (
    <div className="view">
      {header}
      <div className="chat-scroll" ref={scrollRef} onScroll={onScroll}>
        <Transcript items={items} onAction={onAction} />
        {/* The way back, and the only sign that the view is deliberately not
            following. Without it a reader who scrolled up during a long turn
            has no idea whether the agent is still talking. */}
        {!stuck && items.length > 0 && (
          <button
            type="button"
            className="jump-latest"
            onClick={jumpToLatest}
            title="Jump to the end of the conversation"
          >
            <ArrowUpIcon />
            Jump to latest
          </button>
        )}
      </div>
      {error && (
        <div className="chat-error" role="alert">
          {error}
        </div>
      )}
      {notice}
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
