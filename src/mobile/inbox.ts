//! The Inbox as a phone sees it: needs you, done, still sleeping.
//!
//! Same three states of mind as the desktop view (`src/views/InboxView.tsx`),
//! same source — `inbox/list`, the projection of `inbox_events` plus
//! `threads.state = folded` that #15 owns and #22 renders. The phone does not
//! get a second Inbox with different rules; decision #5 says the Inbox *is*
//! the projection, and two devices disagreeing about what needs you would be
//! two products.
//!
//! One thing is genuinely different here, and it is the reason the file
//! exists: **an outstanding permission is a card even when no inbox event has
//! been written for it.** An ask on an *active* thread never resurfaces
//! anything — the desktop is showing it inline in the transcript. For the
//! phone that ask is the entire point of the device, so `permission/pending`
//! is folded into the same list. Where a thread has both an ask and a
//! `needs_you` card, they are one card: the answerable one wins, because two
//! rows for one question is how a human answers twice.

import { NEEDS_YOU_KINDS, inboxTag, type Tag } from "../components/status";
import type { InboxKind } from "../components/types";
import type {
  InboxEventView,
  InboxListResult,
  PendingPermissionView,
  PermissionPendingResult,
  SleepingThreadView,
} from "../host/protocol";
import { askDetail, parseAskOptions, type AskOption } from "./ask";

export type MobileSection = "needs" | "done" | "sleeping";

/** Every card kind the tag helper knows, so an unknown one degrades instead of throwing. */
const KNOWN_KINDS: readonly InboxKind[] = [
  "done",
  "failed",
  "stuck",
  "needs_you",
  "judgment_call",
  "permission",
  "lost",
  "folded",
  // #28. Without it a PR card degrades to `needs_you` and the phone draws
  // "checks failed" under a NEEDS YOU pill, which claims an agent is blocked.
  "pr",
];

export interface MobileCard {
  /** Stable across refreshes: the ask id when there is one, else the event id. */
  id: string;
  threadId: string;
  title: string;
  summary: string;
  kind: InboxKind;
  tag: Tag;
  section: MobileSection;
  /** ISO-8601, newest first within a section. */
  at: string;
  /** Whose thread this is, when it is a bot's. The desktop Inbox resolves it
      against the crew roster to draw that bot's face; absent means a code
      thread, which wears the code mark. */
  botId?: string;
  /** Present exactly when this card can be answered from here. */
  ask?: MobileAsk;
}

export interface MobileAsk {
  requestId: string;
  /** The agent's options, verbatim and in its order. */
  options: AskOption[];
  detail?: string;
  /** No adapter is waiting any more: answering records the decision only. */
  stale: boolean;
}

export interface MobileInbox {
  needs: MobileCard[];
  done: MobileCard[];
  sleeping: MobileCard[];
  /** What the host counts as unread, not what this list happens to hold. */
  unread: number;
}

export const EMPTY_INBOX: MobileInbox = {
  needs: [],
  done: [],
  sleeping: [],
  unread: 0,
};

function kindOf(raw: string): InboxKind {
  return KNOWN_KINDS.includes(raw as InboxKind) ? (raw as InboxKind) : "needs_you";
}

function newestFirst(a: MobileCard, b: MobileCard): number {
  return b.at.localeCompare(a.at);
}

/** A live ask, as the card a phone can act on. */
export function askCard(
  ask: PendingPermissionView,
  threadTitle?: string,
): MobileCard {
  const detail = askDetail(ask);
  return {
    id: ask.requestId,
    threadId: ask.threadId,
    title: ask.title || threadTitle || ask.threadId,
    summary: detail ?? threadTitle ?? "Waiting on you",
    kind: "permission",
    tag: inboxTag("permission"),
    section: "needs",
    at: ask.createdAt,
    ask: {
      requestId: ask.requestId,
      options: parseAskOptions(ask.options),
      detail,
      stale: ask.stale,
    },
  };
}

function eventCard(event: InboxEventView): MobileCard {
  const kind = kindOf(event.kind);
  return {
    id: event.id,
    threadId: event.threadId,
    title: event.title || event.threadTitle,
    summary: event.summary,
    kind,
    tag: inboxTag(kind),
    section: NEEDS_YOU_KINDS.includes(kind) ? "needs" : "done",
    at: event.createdAt,
    botId: event.botId,
  };
}

function sleepingCard(thread: SleepingThreadView): MobileCard {
  return {
    id: `sleeping:${thread.threadId}`,
    threadId: thread.threadId,
    title: thread.title,
    summary:
      thread.acpState === "running"
        ? "Still working"
        : thread.foldPolicy === "wait_for_inbox"
          ? "Waiting for Inbox"
          : "Folded away",
    kind: "folded",
    tag: inboxTag("folded"),
    section: "sleeping",
    at: thread.foldedAt ?? "",
    botId: thread.botId,
  };
}

/**
 * Fold `inbox/list` and `permission/pending` into the three sections.
 *
 * Both calls are in the approver allowlist, and both are reads, so a phone can
 * build this whole screen without being able to change anything.
 */
export function projectInbox(
  inbox: InboxListResult,
  pending: PermissionPendingResult = { requests: [] },
): MobileInbox {
  const titles = new Map<string, string>();
  for (const event of inbox.events) titles.set(event.threadId, event.threadTitle);
  for (const thread of inbox.sleeping) titles.set(thread.threadId, thread.title);

  const asks = pending.requests.map((ask) => askCard(ask, titles.get(ask.threadId)));
  const askedThreads = new Set(asks.map((card) => card.threadId));

  const needs = [...asks];
  const done: MobileCard[] = [];
  for (const event of inbox.events) {
    if (event.dismissedAt) continue;
    const card = eventCard(event);
    if (card.section === "needs") {
      // The thread's question is already on screen as something answerable.
      if (askedThreads.has(card.threadId)) continue;
      needs.push(card);
    } else {
      done.push(card);
    }
  }

  return {
    needs: needs.sort(newestFirst),
    done: done.sort(newestFirst),
    sleeping: inbox.sleeping.map(sleepingCard).sort(newestFirst),
    unread: inbox.unread,
  };
}

/** Drop the card for an ask somebody answered — here, or on another device. */
export function withoutAsk(inbox: MobileInbox, requestId: string): MobileInbox {
  const needs = inbox.needs.filter((card) => card.ask?.requestId !== requestId);
  return needs.length === inbox.needs.length ? inbox : { ...inbox, needs };
}

/** Add a live ask that arrived as a notification, without a round trip. */
export function withAsk(inbox: MobileInbox, card: MobileCard): MobileInbox {
  if (inbox.needs.some((existing) => existing.id === card.id)) return inbox;
  return { ...inbox, needs: [card, ...inbox.needs].sort(newestFirst) };
}
