//! A bot's standing chat. Chief and every worker has exactly one (#6) — extra
//! tasks append to it or fold away to the Inbox, so there is no thread list here
//! and no way to accumulate twelve half-finished conversations with the Writer.
//!
//! The conversation controls #14 added to the code thread are optional props
//! here rather than a second implementation. `LiveChatView` below is what
//! fills them in: it resolves the bot's standing thread with `crew/thread`
//! (#24) and drives this view from the same transcript hook the code thread
//! uses, so the queue strip, the Stop button and the error line are one
//! implementation rather than two.

import { useEffect, useState } from "react";

import { Avatar, avatarStateFor } from "../components/avatar";
import { Conversation } from "../components/Conversation";
import { HostPicker } from "../components/HostPicker";
import type { Bot, HostTarget, TranscriptItem } from "../components/types";
import type { HostClient } from "../host";
import { useThreadTranscript } from "./transcript";

export function ChatView({
  bot,
  host,
  items,
  onSend,
  onAction,
  onPickHost,
  busy,
  queued,
  onCancel,
  error,
}: {
  bot: Bot;
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onPickHost?: (hostId: string) => void;
  /** A turn is in flight on this bot's standing thread (#24). */
  busy?: boolean;
  queued?: readonly string[];
  onCancel?: () => void;
  error?: string | null;
}) {
  return (
    <Conversation
      header={
        <div className="chat-head">
          {/* The only run state this view is ever handed: #24's `busy` is a
              turn in flight on this bot's standing thread, and a queued
              message is one about to be. Everything else a bot can be doing
              happens in a thread this header knows nothing about, so the icon
              stays unringed rather than guessing. */}
          <Avatar
            name={bot.name}
            color={bot.color}
            image={bot.image}
            state={avatarStateFor(
              busy || (queued?.length ?? 0) > 0 ? "running" : null,
            )}
          />
          <h2>{bot.name}</h2>
          <HostPicker host={host} onPick={onPickHost} />
        </div>
      }
      items={items}
      composerPlaceholder={`Message ${bot.name}`}
      onSend={onSend}
      onAction={onAction}
      busy={busy}
      queued={queued}
      onCancel={onCancel}
      error={error}
    />
  );
}

/**
 * The same view, driven by the host.
 *
 * `crew/thread` has been served since #24 and `HostClient.botThread` typed
 * beside it, with no caller anywhere in `src/` — so a bot's chat drew the mock
 * reducer's fixtures keyed by bot id, and every message typed into it went to
 * the reducer too. The bot's real standing thread, its runs and its memory
 * directory were somewhere else entirely.
 *
 * Two steps rather than one because they are two facts: which thread this bot
 * has, and what is in it. `botThread` is idempotent host-side — the id is
 * derived from the bot — so a remount cannot fork the conversation, and
 * `useThreadTranscript` already tolerates a null id, which is what makes the
 * resolve an ordinary effect rather than a conditional hook.
 *
 * Keyed on the bot by its caller, so switching bots remounts and starts a
 * fresh hydrate rather than folding one bot's stream into another's.
 */
export function LiveChatView({
  client,
  bot,
  host,
  onPickHost,
}: {
  client: HostClient;
  bot: Bot;
  host: HostTarget;
  onPickHost?: (hostId: string) => void;
}) {
  const [threadId, setThreadId] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setThreadId(null);
    setOpenError(null);
    (async () => client.botThread({ botId: bot.id }))()
      .then((thread) => {
        if (!cancelled) setThreadId(thread.threadId);
      })
      .catch((err: unknown) => {
        // Said rather than swallowed: without a thread there is no
        // conversation to fall back to, and an empty chat that silently
        // discards what you type is the failure this view existed to fix.
        if (!cancelled) {
          setOpenError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [client, bot.id]);

  const { stream, error, send, cancel, answer } = useThreadTranscript(
    client,
    threadId,
  );

  return (
    <ChatView
      bot={bot}
      host={host}
      items={stream.items}
      onSend={send}
      // The buttons on a permission card are the agent's own ACP options, and
      // this is what carries the one the user pressed back to it (#20).
      onAction={answer}
      onPickHost={onPickHost}
      busy={stream.busy}
      queued={stream.queued}
      onCancel={cancel}
      error={openError ?? error}
    />
  );
}
