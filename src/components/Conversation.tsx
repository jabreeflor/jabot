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

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

import { ArrowUpIcon } from "./Icon";
import { Composer } from "./Composer";
import type { OnInteract } from "./InteractionCards";
import { Transcript } from "./Transcript";
import type { Bot, TranscriptItem } from "./types";

/** How close to the bottom still counts as being at it. Sub-pixel rounding and
    a streaming bubble growing between frames both put the exact bottom a few
    pixels out of reach, and a reader who never left should not be treated as
    having done so. */
const STICK_THRESHOLD = 32;

export function Conversation({
  header,
  items,
  bots,
  onSelectBot,
  composerPlaceholder,
  onSend,
  onAction,
  onInteract,
  onReact,
  onBranch,
  branchingSeq,
  busy = false,
  queued,
  onCancel,
  error,
  notice,
  disabled = false,
  modelChip,
  modelStatus,
}: {
  header: ReactNode;
  items: readonly TranscriptItem[];
  /** Crew for in-thread mention pills and the composer @ picker. */
  bots?: readonly Bot[];
  onSelectBot?: (botId: string) => void;
  composerPlaceholder: string;
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  /** A question or plan card's answer (#298). */
  onInteract?: OnInteract;
  /** Toggle an emoji on an agent bubble (#265). */
  onReact?: (itemId: string, emoji: string) => void;
  /** Code chats only (#266): fork the conversation through this message. */
  onBranch?: (itemId: string, seq: number) => void;
  branchingSeq?: number | null;
  /** A turn is in flight. */
  busy?: boolean;
  /** Prompts the host is holding until it ends, oldest first (#14). */
  queued?: readonly string[];
  onCancel?: () => void;
  /** The standing thread (or equivalent) is not ready — a send would be dropped. */
  disabled?: boolean;
  /** The last host error on this thread, shown rather than swallowed. */
  error?: string | null;
  /** A standing caution about this thread, distinct from `error`: an error is
      something that just failed, a notice is something true about the next
      thing you do. Drawn above the composer because that is where the action
      it is about is taken. */
  notice?: ReactNode;
  modelChip?: ReactNode;
  modelStatus?: ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const spacerRef = useRef<HTMLDivElement>(null);
  const stuckRef = useRef(true);
  const promptRef = useRef<string | null>(null);
  const [stuck, setStuck] = useState(true);
  // WebKit delivers `scroll` after a programmatic pin with the *old* offset.
  // Remember that offset so we re-apply the pin instead of treating the echo
  // as the reader leaving.
  const pinEchoTopRef = useRef<number | null>(null);
  const pinTargetRef = useRef<number | null>(null);
  const lastClientHeightRef = useRef(0);
  const latestUser = [...items].reverse().find((item) => item.kind === "user");

  function pinTo(top: number) {
    const scroll = scrollRef.current;
    if (!scroll) return;
    pinEchoTopRef.current = scroll.scrollTop;
    pinTargetRef.current = top;
    lastClientHeightRef.current = scroll.clientHeight;
    scroll.scrollTop = top;
  }

  function pinToEnd() {
    const scroll = scrollRef.current;
    if (!scroll || !stuckRef.current) return;
    pinTo(scroll.scrollHeight);
  }

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    const spacer = spacerRef.current;
    const transcript = scroll?.querySelector<HTMLElement>(".transcript");
    if (!scroll || !spacer || !transcript) return;

    // Reserve the unused part of a turn so even a one-line prompt can sit at
    // the top. As the reply grows, it consumes this space instead of pushing
    // the prompt upward. Measure content, not scrollHeight (which includes it).
    const measure = () => {
      const users = transcript.querySelectorAll<HTMLElement>(".msg.me");
      const prompt = users[users.length - 1];
      const styles = getComputedStyle(scroll);
      const top = parseFloat(styles.paddingTop) || 0;
      const bottom = parseFloat(styles.paddingBottom) || 0;
      const turnHeight = prompt
        ? transcript.getBoundingClientRect().bottom -
          prompt.getBoundingClientRect().top
        : scroll.clientHeight;
      spacer.style.height = `${Math.max(0, scroll.clientHeight - top - bottom - turnHeight)}px`;
      return prompt
        ? prompt.getBoundingClientRect().top -
            scroll.getBoundingClientRect().top +
            scroll.scrollTop -
            top
        : null;
    };

    const promptTop = measure();
    if (
      latestUser &&
      latestUser.id !== promptRef.current &&
      promptTop !== null
    ) {
      promptRef.current = latestUser.id;
      stuckRef.current = false;
      pinTo(promptTop);
    } else if (stuckRef.current) {
      pinToEnd();
    }
    const updateIndicator = () =>
      setStuck(
        scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight <=
          STICK_THRESHOLD,
      );
    updateIndicator();

    // Fonts, window resizing, and expanded transcript rows can change geometry
    // without a new stream item. They must also release/reserve the blank space.
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      measure();
      updateIndicator();
    });
    observer.observe(scroll);
    observer.observe(transcript);
    return () => observer.disconnect();
  }, [items, latestUser?.id]);

  // Composer chrome (the model chip, a status line) lives *outside*
  // `.chat-scroll`. When it mounts, flex shrinks the scroller past
  // STICK_THRESHOLD — the reader did not move. Re-pin only while stuck.
  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll || typeof ResizeObserver === "undefined") return;
    lastClientHeightRef.current = scroll.clientHeight;
    const ro = new ResizeObserver(() => pinToEnd());
    ro.observe(scroll);
    return () => ro.disconnect();
  }, []);

  function onScroll() {
    const scroll = scrollRef.current;
    if (!scroll) return;
    const height = scroll.clientHeight;
    const previousHeight = lastClientHeightRef.current;
    lastClientHeightRef.current = height;

    // Echo of our own pin: WebKit reports the pre-assignment scrollTop.
    if (
      pinEchoTopRef.current !== null &&
      Math.abs(scroll.scrollTop - pinEchoTopRef.current) <= 1
    ) {
      pinEchoTopRef.current = null;
      if (pinTargetRef.current !== null) scroll.scrollTop = pinTargetRef.current;
      return;
    }
    pinEchoTopRef.current = null;

    // Viewport shrank (composer chrome). Same rule as the observer: stay put
    // only if we were already following.
    if (previousHeight > 0 && height < previousHeight && stuckRef.current) {
      scroll.scrollTop = scroll.scrollHeight;
      return;
    }

    // A threshold rather than an equality: sub-pixel rounding, and a streaming
    // bubble that grows between the scroll event and this read, both put the
    // exact bottom a few pixels out of reach.
    const atEnd =
      scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight <=
      STICK_THRESHOLD;
    // A prompt remains anchored even when its short reply fits on screen.
    // Explicitly jumping to latest opts back into following the reply.
    if (stuckRef.current || !atEnd) stuckRef.current = atEnd;
    setStuck(atEnd);
  }

  function jumpToLatest() {
    stuckRef.current = true;
    setStuck(true);
    pinToEnd();
  }

  const waiting = queued ?? [];

  return (
    <div className="view">
      {header}
      <div className="chat-scroll" ref={scrollRef} onScroll={onScroll}>
        <Transcript
          items={items}
          bots={bots}
          onSelectBot={onSelectBot}
          onAction={onAction}
          onInteract={onInteract}
          onReact={onReact}
          onBranch={onBranch}
          branchingSeq={branchingSeq}
        />
        <div ref={spacerRef} aria-hidden="true" className="turn-space" />
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
        disabled={disabled}
        mentionBots={bots}
        modelChip={modelChip}
        modelStatus={modelStatus}
      />
    </div>
  );
}
