//! The phone, as a screen stack (#29).
//!
//! Two screens and one piece of state: which thread is open. `InboxScreen` has
//! carried an `onOpen` since #29 with nothing to hand it to, so tapping a card
//! title did nothing at all; this is the thing that was missing.
//!
//! Still presentational about the protocol — everything it knows about the
//! host it gets from the [`MobileSession`] handed in. Which is what lets the
//! whole stack be rendered in a test, and in the dev harness, without a
//! transport.

import { useState } from "react";

import { InboxScreen } from "./InboxScreen";
import { TranscriptScreen, useThreadTranscript } from "./TranscriptScreen";
import type { MobileInbox } from "./inbox";
import type { MobileSession } from "./session";
import "./mobile.css";

export interface MobileAppProps {
  inbox: MobileInbox;
  /** Only `transcript` is used, so a fake in a test is one method wide. */
  session: Pick<MobileSession, "transcript"> | null;
  deviceName?: string;
  busyId?: string | null;
  onAnswer(requestId: string, optionId: string): void;
  onDecline(requestId: string): void;
}

export function MobileApp({
  inbox,
  session,
  deviceName,
  busyId,
  onAnswer,
  onDecline,
}: MobileAppProps) {
  const [openThreadId, setOpenThreadId] = useState<string | null>(null);
  // Mounted unconditionally, and inert while nothing is open: a hook behind a
  // branch is a hook that changes the render's shape.
  const transcript = useThreadTranscript(session, openThreadId);

  if (openThreadId) {
    return (
      <TranscriptScreen
        title={titleFor(inbox, openThreadId)}
        items={transcript.items}
        truncated={transcript.truncated}
        queued={transcript.queued}
        loading={transcript.loading}
        error={transcript.error}
        onBack={() => setOpenThreadId(null)}
      />
    );
  }

  return (
    <InboxScreen
      inbox={inbox}
      deviceName={deviceName}
      busyId={busyId}
      onAnswer={onAnswer}
      onDecline={onDecline}
      onOpen={setOpenThreadId}
    />
  );
}

/** The card's own words for the thread. A bare id in the header would make
    the screen look like a debugging tool. */
function titleFor(inbox: MobileInbox, threadId: string): string | undefined {
  for (const section of [inbox.needs, inbox.done, inbox.sleeping]) {
    const card = section.find((row) => row.threadId === threadId);
    if (card) return card.title;
  }
  return undefined;
}
