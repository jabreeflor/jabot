//! A bot's standing chat. Chief and every worker has exactly one (#6) — extra
//! tasks append to it or fold away to the Inbox, so there is no thread list here
//! and no way to accumulate twelve half-finished conversations with the Writer.

import { Blob } from "../components/Blob";
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
}: {
  bot: Bot;
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onPickHost?: (hostId: string) => void;
}) {
  return (
    <Conversation
      header={
        <div className="chat-head">
          <Blob color={bot.color} />
          <h2>{bot.name}</h2>
          <HostPicker host={host} onPick={onPickHost} />
        </div>
      }
      items={items}
      composerPlaceholder={`Message ${bot.name}`}
      onSend={onSend}
      onAction={onAction}
    />
  );
}
