//! The Inbox on real data (#22).
//!
//! Decision #5 makes the Inbox a *projection*: `inbox_events` for what came
//! back, `threads.state = folded` for what is still asleep, and the host owns
//! both. So this hook is the same shape as `folders.ts`, `crew.ts` and
//! `pulls.ts` — ask the host, rename the wire rows into the props the view
//! already takes, and let the fixtures stand only until the first answer.
//!
//! Two things here are not a rename.
//!
//! **An outstanding permission is a card.** The desktop draws an ask inline in
//! the transcript (#20), which is the right place when you are reading the
//! thread — and no place at all when you are not. `permission/pending` is
//! folded into the list by the same [`projectInbox`] the phone uses, so the
//! two devices cannot end up with different ideas of what needs you. Where a
//! thread has both an ask and a `needs_you` card they collapse to one row: the
//! answerable one.
//!
//! **Opening a card is a state change.** `thread/reopen` is what clears a
//! thread's badge (`resurface.md`), puts an archived thread's worktree back
//! (#23), and moves the row out of Still Sleeping and into the sidebar. So the
//! Inbox opens threads through the host rather than by pointing the main pane
//! at an id — and only when the thread is somewhere `reopen` is legal from:
//! the transition table refuses it on a thread that is already active, and a
//! `pr` card's thread very often is.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  INBOX_EVENT,
  INBOX_RESURFACE,
  PERMISSION_ASK,
  PERMISSION_RESOLVED,
  type HostClient,
  type InboxEventView,
  type NotifyStatusResult,
  type PermissionPendingResult,
  type ThreadOverlayState,
} from "../host";
import type {
  Bot,
  CardSource,
  InboxCard,
  InboxDetail,
  NoticeAction,
} from "../components/types";
// The projection lives under `src/mobile/` because #29 needed it first, and it
// is deliberately device-neutral: its own module docs say two devices
// disagreeing about what needs you would be two products. Importing it is what
// keeps that true — a second copy here is exactly the drift it warns about.
import { projectInbox, type MobileCard as ProjectedCard } from "../mobile/inbox";

/** Open the card's thread. The view raises it through `onOpenThread`. */
export const CARD_REOPEN = "reopen";
/** Close the card's thread out for good. */
export const CARD_ARCHIVE = "archive";
/** `ask:<optionId>` — answer the permission this card *is*, with the agent's
    own option id. Prefixed because a card's other buttons are ours. */
export const ASK_PREFIX = "ask:";

/** The states `thread/reopen` is legal from. Anything else is already open. */
const REOPENABLE: readonly ThreadOverlayState[] = [
  "folded",
  "resurfaced",
  "archived",
];

export interface HostInbox {
  /** `null` until the host answers; `[]` is a real answer and a good day. */
  cards: InboxCard[] | null;
  /** The host's own badge count, not a second classification of these rows. */
  unread: number | null;
  /** Why the last load failed, for the pane that would otherwise look empty. */
  error: string | null;
  loading: boolean;
  reload: () => void;
  /** Open a card's thread, reopening it on the host when it is asleep. */
  open: (threadId: string) => Promise<void>;
  /** Whatever the card's own buttons do. Unknown ids are ignored. */
  act: (cardId: string, actionId: string) => Promise<void>;
  /** What the OS says about banners (#27). `null` until asked, and on any host
      that will not answer — the Inbox is complete without it. */
  notify: NotifyStatusResult | null;
}

/**
 * @param onThreadChanged Re-read the lists this moved a thread between — the
 * sidebar gains a row on reopen and loses one on archive.
 */
export function useInbox(
  client: HostClient | null,
  onThreadChanged?: () => void,
  /** The crew, so a card belonging to a bot can wear that bot's face. `null`
      while it loads, which draws the code mark — see `cardSource`. */
  bots?: readonly Bot[] | null,
): HostInbox {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [notify, setNotify] = useState<NotifyStatusResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [generation, setGeneration] = useState(0);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    setLoading(true);
    // Guarded as a whole, method lookup included: a transport that predates
    // `inbox/list` — an older host, a unit test's stub — should leave the
    // shell on its fixtures rather than take the render down.
    load(client)
      .then((next) => {
        if (cancelled) return;
        setSnapshot(next);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(message(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  // Asked once, not on every reload and not inside `load`. An OS notification
  // permission is changed in System Settings, not by anything this app does,
  // so re-asking on every poll would be a round trip per refresh for an answer
  // that does not move. Its failure is swallowed for the same reason
  // `pendingOrNone` swallows its own: the Inbox is complete without it, and
  // every card is written whether or not a banner was ever allowed.
  useEffect(() => {
    if (!client) return;
    if (typeof client.notifyStatus !== "function") return;
    let cancelled = false;
    client
      .notifyStatus()
      .then((status) => {
        if (!cancelled) setNotify(status);
      })
      .catch(() => {
        // Deliberately silent; see above.
      });
    return () => {
      cancelled = true;
    };
  }, [client]);

  // The Inbox is the one view that has to be right while nobody is looking at
  // it: the badge is drawn from it, and the card the user came back for was
  // written by a thread that resurfaced on its own.
  useEffect(() => {
    if (!client) return;
    try {
      return client.onNotification((notification) => {
        if (LIVE.includes(notification.method)) reload();
      });
    } catch {
      // A transport with no notification channel — an older host, a test's
      // stub — still has an Inbox; it just has to be asked for it again.
      return;
    }
  }, [client, reload]);

  const open = useCallback(
    async (threadId: string) => {
      const state = snapshot?.states.get(threadId);
      // Nothing to reopen: the thread is already active, or it is a fixture
      // row this hook has never heard of.
      if (!client || !state || !REOPENABLE.includes(state)) return;
      try {
        await client.reopenThread({ threadId });
        setError(null);
      } catch (err: unknown) {
        setError(message(err));
      } finally {
        reload();
        onThreadChanged?.();
      }
    },
    [client, snapshot, reload, onThreadChanged],
  );

  const act = useCallback(
    async (cardId: string, actionId: string) => {
      const card = snapshot?.cards.find((row) => row.id === cardId);
      const answering = actionId.startsWith(ASK_PREFIX);
      // Nothing to do, and nothing to re-read for: an id this hook does not
      // know is a fixture card's, or a button a later issue added.
      if (!client || !card || (!answering && actionId !== CARD_ARCHIVE)) return;
      try {
        if (answering) {
          const deviceId = client.deviceId;
          if (!deviceId) throw new Error("Not connected to the host yet.");
          // The card *is* the ask, so its id is the request id — the same
          // identity the phone answers with.
          await client.replyPermission({
            requestId: card.id,
            deviceId,
            optionId: actionId.slice(ASK_PREFIX.length),
          });
        } else {
          await client.archiveThread({ threadId: card.threadId });
        }
        setError(null);
      } catch (err: unknown) {
        setError(message(err));
      } finally {
        reload();
        onThreadChanged?.();
      }
    },
    [client, snapshot, reload, onThreadChanged],
  );

  const cards = useMemo(
    () =>
      snapshot ? snapshot.cards.map((card) => cardRow(card, snapshot, bots ?? null)) : null,
    [snapshot, bots],
  );

  return {
    cards,
    unread: snapshot?.unread ?? null,
    error,
    loading,
    reload,
    open,
    act,
    notify,
  };
}

/** The notifications that change what the Inbox says. */
const LIVE: readonly string[] = [
  INBOX_RESURFACE,
  INBOX_EVENT,
  PERMISSION_ASK,
  PERMISSION_RESOLVED,
];

interface Snapshot {
  cards: ProjectedCard[];
  unread: number;
  /** Per thread, so a card knows whether opening it is a reopen. */
  states: Map<string, ThreadOverlayState>;
  /** Per event id, for the journey line a card expands onto. */
  events: Map<string, InboxEventView>;
}

async function load(client: HostClient): Promise<Snapshot> {
  const listed = await client.inbox();
  const pending = await pendingOrNone(client);
  const projection = projectInbox(listed, pending);
  const states = new Map<string, ThreadOverlayState>();
  const events = new Map<string, InboxEventView>();
  for (const event of listed.events) {
    events.set(event.id, event);
    states.set(event.threadId, event.threadState);
  }
  for (const thread of listed.sleeping) states.set(thread.threadId, "folded");
  return {
    // One flat list in the order the view draws them; `InboxView` splits
    // resurfaced from sleeping itself, and filters the tabs off the kind.
    cards: [...projection.needs, ...projection.done, ...projection.sleeping],
    unread: projection.unread,
    states,
    events,
  };
}

/**
 * Asks, or none.
 *
 * A host that will not answer `permission/pending` still has an Inbox, and the
 * events half is the half that resurfaced the thread. Losing the whole list
 * over the answerable extra would be the wrong way round.
 */
async function pendingOrNone(
  client: HostClient,
): Promise<PermissionPendingResult> {
  try {
    return await client.pendingPermissions();
  } catch {
    return { requests: [] };
  }
}

function cardRow(
  card: ProjectedCard,
  snapshot: Snapshot,
  bots: readonly Bot[] | null,
): InboxCard {
  return {
    id: card.id,
    threadId: card.threadId,
    kind: card.kind,
    title: card.title,
    summary: card.summary,
    createdAt: card.at,
    source: cardSource(card.botId, bots),
    detail: detail(card, snapshot),
  };
}

/**
 * The face on an Inbox row.
 *
 * The avatar is the only thing on the row that says *who* this is, and until
 * `inbox/list` carried `botId` every host card wore the generic code mark —
 * including the ones belonging to a named crew member. #22 was right to refuse
 * to invent a bot rather than guess; the fix was to put the id on the wire.
 *
 * Still the code mark in two cases, and both are honest rather than lazy: a
 * thread with no bot really is a code session, and a bot the roster does not
 * (yet) contain cannot be drawn — the crew loads separately, and a card
 * holding only a reference has to draw *something* in between. A face with the
 * wrong name on it would be worse than no face.
 */
function cardSource(botId: string | undefined, bots: readonly Bot[] | null): CardSource {
  if (!botId) return { type: "code" };
  const bot = bots?.find((candidate) => candidate.id === botId);
  if (!bot) return { type: "code" };
  return { type: "bot", name: bot.name, color: bot.color, image: bot.image };
}

/**
 * What a card expands onto.
 *
 * Sleeping rows get nothing: `InboxView` gives them a click that opens the
 * thread instead of a disclosure, because there is nothing to say about work
 * that has not come back yet beyond the fact that it has not.
 */
function detail(
  card: ProjectedCard,
  snapshot: Snapshot,
): InboxDetail | undefined {
  if (card.section === "sleeping") return undefined;
  if (card.ask) {
    return {
      // The command, when the agent sent one: "Run ls" and "Run rm -rf /" are
      // the same title, and this is the line that tells them apart.
      path: card.ask.detail ?? card.title,
      bullets: card.ask.stale
        ? ["The session that asked this is gone — answering records your decision, but the agent will never hear it."]
        : [],
      actions: [
        // The agent's own options, in the agent's order. The host never
        // invents one and neither does this (#20).
        ...card.ask.options.map(
          (option, index): NoticeAction => ({
            id: `${ASK_PREFIX}${option.optionId}`,
            label: option.name,
            primary: index === 0,
          }),
        ),
        { id: CARD_REOPEN, label: "Open thread" },
      ],
    };
  }
  const event = snapshot.events.get(card.id);
  const state = event?.threadState;
  const actions: NoticeAction[] = [
    { id: CARD_REOPEN, label: "Open thread", primary: true },
  ];
  // Archiving an archived thread is an illegal transition, and offering a
  // button whose only outcome is an error message is worse than not offering it.
  if (state !== "archived") {
    actions.push({ id: CARD_ARCHIVE, label: "Archive" });
  }
  return {
    path: event?.threadTitle
      ? `${event.threadTitle} · ${state ?? "thread"}`
      : card.title,
    bullets: [],
    actions,
  };
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
