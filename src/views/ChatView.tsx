//! A bot's standing chat. Chief and every worker has exactly one (#6) — extra
//! tasks append to it or fold away to the Inbox, so there is no thread list here
//! and no way to accumulate twelve half-finished conversations with the Writer.
//!
//! The conversation controls #14 added to the code thread are optional props
//! here rather than a second implementation: nothing opens a bot's standing
//! thread yet (#24 does), and when something does, this view already carries
//! the queue strip, the Stop button and the error line that a live turn needs.

import { Avatar, avatarStateFor } from "../components/avatar";
import { Conversation } from "../components/Conversation";
import { HostPicker } from "../components/HostPicker";
import type { Bot, HostTarget, TranscriptItem } from "../components/types";

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
