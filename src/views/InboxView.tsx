//! Inbox — where folded threads come back to you.
//!
//! Two sections, because they are two different states of mind: work that has
//! *returned* and wants something, and work that is still asleep and does not.
//! Sleeping cards only appear under "All"; filtering to "Needs you" and still
//! showing them would defeat the point of folding them away.
//!
//! Cards are `inbox_events` rows (#22). The projection is the host's job; this
//! view only decides what a card looks like and which tab it falls under.

import { useState } from "react";

import { Avatar } from "../components/avatar";
import { CodeSessionIcon } from "../components/Icon";
import { formatWhen } from "../components/format";
import { NEEDS_YOU_KINDS, inboxTag } from "../components/status";
import { Tabs, tabButtonId, type TabSpec } from "../components/Tabs";
import type { CardSource, InboxCard } from "../components/types";

type InboxTab = "all" | "needs" | "done";

const TABS: readonly TabSpec<InboxTab>[] = [
  { id: "all", label: "All" },
  { id: "needs", label: "Needs you" },
  { id: "done", label: "Done" },
];

export function InboxView({
  cards,
  now,
  error,
  loading,
  onOpenThread,
  onAction,
}: {
  cards: readonly InboxCard[];
  /** Injected so "10:12" is not a moving target in tests. */
  now?: Date;
  /** Why the host's answer did not arrive. Said here rather than in the app
      banner: an Inbox that cannot be read is a fact about this pane. */
  error?: string | null;
  /** The host has been asked and has not answered yet. */
  loading?: boolean;
  onOpenThread: (threadId: string) => void;
  onAction?: (cardId: string, actionId: string) => void;
}) {
  const [tab, setTab] = useState<InboxTab>("all");
  const [openId, setOpenId] = useState<string | null>(
    cards.find((card) => card.detail)?.id ?? null,
  );

  const matching = cards.filter((card) => {
    if (tab === "needs") return NEEDS_YOU_KINDS.includes(card.kind);
    if (tab === "done") return card.kind === "done";
    return true;
  });
  const resurfaced = matching.filter((card) => card.kind !== "folded");
  const sleeping =
    tab === "all" ? matching.filter((c) => c.kind === "folded") : [];

  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Inbox</h1>
            <p>Where folded threads come back to you</p>
          </div>

          <Tabs
            label="Inbox filter"
            panelId="inbox-panel"
            tabs={TABS}
            value={tab}
            onChange={setTab}
          />

          {error && (
            <div className="page-empty" role="alert">
              {error}
            </div>
          )}

          <div
            id="inbox-panel"
            role="tabpanel"
            aria-labelledby={tabButtonId("inbox-panel", tab)}
          >
            {loading && <div className="page-empty">Checking the Inbox…</div>}

            {!loading && resurfaced.length === 0 && sleeping.length === 0 && (
              <div className="page-empty">Nothing waiting. Enjoy it.</div>
            )}

            {resurfaced.length > 0 && (
              <>
                <div className="page-section">RESURFACED</div>
                {resurfaced.map((card) => (
                  <InboxRow
                    key={card.id}
                    card={card}
                    now={now}
                    open={openId === card.id}
                    onToggle={() =>
                      setOpenId(openId === card.id ? null : card.id)
                    }
                    onOpenThread={onOpenThread}
                    onAction={onAction}
                  />
                ))}
              </>
            )}

            {sleeping.length > 0 && (
              <>
                <div className="page-section">STILL SLEEPING</div>
                {sleeping.map((card) => (
                  <InboxRow
                    key={card.id}
                    card={card}
                    now={now}
                    open={false}
                    onToggle={() => onOpenThread(card.threadId)}
                    onOpenThread={onOpenThread}
                    onAction={onAction}
                  />
                ))}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function InboxRow({
  card,
  now,
  open,
  onToggle,
  onOpenThread,
  onAction,
}: {
  card: InboxCard;
  now?: Date;
  open: boolean;
  onToggle: () => void;
  onOpenThread: (threadId: string) => void;
  onAction?: (cardId: string, actionId: string) => void;
}) {
  const tag = inboxTag(card.kind);
  const sleeping = card.kind === "folded";

  return (
    <div
      className={["card-row", open ? "open" : "", sleeping ? "dim" : ""]
        .filter(Boolean)
        .join(" ")}
    >
      <CardAvatar source={card.source} />
      <div className="bd">
        <button
          type="button"
          className="card-summary"
          aria-expanded={card.detail ? open : undefined}
          onClick={onToggle}
        >
          <span className="r1">
            <span className="ti">{card.title}</span>
            <span className="when">{formatWhen(card.createdAt, now)}</span>
          </span>
          <span className="de">{card.summary}</span>
          <span className={`tagpill ${tag.tone}`}>{tag.label}</span>
        </button>

        {open && card.detail && (
          <div className="card-detail">
            <div className="path">{card.detail.path}</div>
            {card.detail.bullets.length > 0 && (
              <ul>
                {card.detail.bullets.map((bullet) => (
                  <li key={bullet}>{bullet}</li>
                ))}
              </ul>
            )}
            <div className="acts">
              {card.detail.actions.map((action) => (
                <button
                  key={action.id}
                  type="button"
                  className={action.primary ? "btn primary" : "btn"}
                  onClick={() =>
                    action.id === "reopen"
                      ? onOpenThread(card.threadId)
                      : onAction?.(card.id, action.id)
                  }
                >
                  {action.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function CardAvatar({ source }: { source: CardSource }) {
  if (source.type === "code") {
    return (
      <div className="codeav">
        <CodeSessionIcon />
      </div>
    );
  }
  // Named, unusually for an avatar: every other call site prints the bot's
  // name in text beside the drawing, and an Inbox row does not — it carries a
  // title, a summary, a time and a pill, none of which say who this is. The
  // avatar is the only thing on the row that does, which is the complaint #44
  // opens with, so here it is the name rather than a tooltip.
  return (
    <Avatar
      name={source.name}
      color={source.color}
      image={source.image}
      labelled
    />
  );
}
