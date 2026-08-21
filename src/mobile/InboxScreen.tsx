//! The phone screen: three sections, and buttons that answer.
//!
//! Presentational on purpose — it takes a [`MobileInbox`] and calls back. The
//! session owns the protocol, so this file can be replaced by a React Native
//! screen without any of the client moving.
//!
//! Two rules it must not break.
//!
//! **The buttons are the agent's options, verbatim.** #20 promises the host
//! never invents an option the agent did not offer, and a phone inventing one
//! would break that promise on the device where it is hardest to notice. If
//! the agent offered nothing answerable, the card says so and offers Dismiss,
//! which is `cancelled` and is a different answer.
//!
//! **Sleeping is only ever shown as sleeping.** Decision #5: a folded thread
//! is not a notification. It appears at the bottom, dimmed, with nothing to
//! press — showing it any louder would undo the fold on the device most likely
//! to be buzzing in somebody's pocket.

import type { ReactNode } from "react";

import type { MobileCard, MobileInbox } from "./inbox";
import "./mobile.css";

export interface InboxScreenProps {
  inbox: MobileInbox;
  /** What the host bound this connection to; drawn so the human can see it. */
  deviceName?: string;
  /** The card whose answer is in flight — its buttons go inert, not away. */
  busyId?: string | null;
  onAnswer(requestId: string, optionId: string): void;
  onDecline(requestId: string): void;
  onOpen?(threadId: string): void;
}

export function InboxScreen({
  inbox,
  deviceName,
  busyId,
  onAnswer,
  onDecline,
  onOpen,
}: InboxScreenProps) {
  const empty =
    inbox.needs.length === 0 &&
    inbox.done.length === 0 &&
    inbox.sleeping.length === 0;

  return (
    <div className="jm">
      <div className="jm-top">
        <h1>Inbox</h1>
        {deviceName && <span className="jm-device">{deviceName}</span>}
      </div>

      {empty && <div className="jm-empty">Nothing waiting. Enjoy it.</div>}

      <Section title="NEEDS YOU" cards={inbox.needs}>
        {(card) => (
          <Card
            key={card.id}
            card={card}
            busy={busyId === card.id}
            onAnswer={onAnswer}
            onDecline={onDecline}
            onOpen={onOpen}
          />
        )}
      </Section>

      <Section title="DONE" cards={inbox.done}>
        {(card) => <Card key={card.id} card={card} onOpen={onOpen} />}
      </Section>

      <Section title="STILL SLEEPING" cards={inbox.sleeping}>
        {(card) => <Card key={card.id} card={card} dim />}
      </Section>
    </div>
  );
}

function Section({
  title,
  cards,
  children,
}: {
  title: string;
  cards: readonly MobileCard[];
  children: (card: MobileCard) => ReactNode;
}) {
  if (cards.length === 0) return null;
  return (
    <section aria-label={title}>
      <div className="jm-section">{title}</div>
      {cards.map((card) => children(card))}
    </section>
  );
}

function Card({
  card,
  busy,
  dim,
  onAnswer,
  onDecline,
  onOpen,
}: {
  card: MobileCard;
  busy?: boolean;
  dim?: boolean;
  onAnswer?: (requestId: string, optionId: string) => void;
  onDecline?: (requestId: string) => void;
  onOpen?: (threadId: string) => void;
}) {
  const ask = card.ask;
  return (
    <article className={dim ? "jm-card dim" : "jm-card"}>
      <span className={`jm-pill ${card.tag.tone}`}>{card.tag.label}</span>
      {onOpen ? (
        <button
          type="button"
          className="jm-card-title"
          onClick={() => onOpen(card.threadId)}
        >
          {card.title}
        </button>
      ) : (
        <span className="jm-card-title">{card.title}</span>
      )}
      <span className="jm-card-summary">{card.summary}</span>

      {ask?.stale && (
        // #20: the record is still answerable, the agent is not still asking.
        // Saying so is the difference between "allowed" and "allowed, but
        // nothing is going to happen".
        <span className="jm-stale">
          The agent that asked this is gone — your answer is recorded, not acted
          on.
        </span>
      )}

      {ask && onAnswer && onDecline && (
        <div className="jm-acts">
          {ask.options.map((option) => (
            <button
              key={option.optionId}
              type="button"
              className={option.kind === "allow_once" ? "primary" : undefined}
              disabled={busy}
              onClick={() => onAnswer(ask.requestId, option.optionId)}
            >
              {option.name}
            </button>
          ))}
          <button
            type="button"
            disabled={busy}
            onClick={() => onDecline(ask.requestId)}
          >
            {ask.options.length === 0 ? "Dismiss" : "Not now"}
          </button>
        </div>
      )}
    </article>
  );
}
