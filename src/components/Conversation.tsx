//! Scrolling transcript + composer, under whatever header the view supplies.
//!
//! A bot chat and a code thread are the same conversation with a different
//! nameplate — one is a standing thread with a persona, the other a job in a
//! repo — so the body is one component and only the header differs.

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
}: {
  header: ReactNode;
  items: readonly TranscriptItem[];
  composerPlaceholder: string;
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // Follow the tail. #14 replaces this with end-anchored virtualization once a
  // transcript is long enough to jank; the behaviour it has to preserve is
  // exactly this one.
  useEffect(() => {
    const scroll = scrollRef.current;
    if (scroll) scroll.scrollTop = scroll.scrollHeight;
  }, [items]);

  return (
    <div className="view">
      {header}
      <div className="chat-scroll" ref={scrollRef}>
        <Transcript items={items} onAction={onAction} />
      </div>
      <Composer placeholder={composerPlaceholder} onSend={onSend} />
    </div>
  );
}
