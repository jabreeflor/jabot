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
//!
//! Create is chat-first: an unformed bot (empty instructions) opens on a
//! welcome and starter pills. The first send shapes the persona. The editor
//! stays behind the header gear as advanced settings.

import { useEffect, useMemo, useState } from "react";

import { AgentPill } from "../components/AgentPill";
import { avatarStateFor } from "../components/avatar";
import { Conversation } from "../components/Conversation";
import { GearIcon } from "../components/Icon";
import { HostPicker } from "../components/HostPicker";
import type {
  Bot,
  BotDraft,
  BotTemplate,
  HostTarget,
  TranscriptItem,
} from "../components/types";
import type { BotDraftView, HostClient } from "../host";
import { hostErrorText } from "./errors";
import {
  SHAPING_WELCOME,
  asBotColor,
  draftFromTemplate,
  isUnformedBot,
  shapeFromMessage,
} from "./shape-bot";
import { useThreadTranscript } from "./transcript";

export function ChatView({
  bot,
  bots = [],
  templates = [],
  pendingDraft = null,
  host,
  items,
  onSend,
  onShape,
  onSelectBot,
  onOpenSettings,
  onAcceptDraft,
  onReviewDraft,
  onAction,
  onReact,
  onPickHost,
  busy,
  queued,
  onCancel,
  error,
  disabled,
}: {
  bot: Bot;
  /** The crew, for presence pills and @mentions. */
  bots?: readonly Bot[];
  templates?: readonly BotTemplate[];
  pendingDraft?: BotDraftView | null;
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onShape?: (draft: BotDraft) => void | Promise<void>;
  onSelectBot?: (botId: string) => void;
  onOpenSettings?: () => void;
  onAcceptDraft?: (draftId: string) => void;
  onReviewDraft?: (draftId: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onReact?: (itemId: string, emoji: string) => void;
  onPickHost?: (hostId: string) => void;
  /** A turn is in flight on this bot's standing thread (#24). */
  busy?: boolean;
  queued?: readonly string[];
  onCancel?: () => void;
  error?: string | null;
  /** The standing thread is not open yet — a send would be dropped. */
  disabled?: boolean;
}) {
  const unformed = isUnformedBot(bot);
  const others = bots.filter((row) => row.id !== bot.id);
  const runState = avatarStateFor(
    busy || (queued?.length ?? 0) > 0 ? "running" : null,
  );

  const displayItems = useMemo(() => {
    if (!unformed) return items;
    if (items.some((item) => item.kind === "user" || item.kind === "agent")) {
      return items;
    }
    return [
      {
        kind: "agent" as const,
        id: `${bot.id}-shape-welcome`,
        text: SHAPING_WELCOME,
      },
      ...items,
    ];
  }, [bot.id, items, unformed]);

  async function handleSend(text: string) {
    if (unformed && onShape) {
      const shaped = shapeFromMessage(text, bot);
      await onShape({
        name: shaped.name,
        color: bot.color,
        instructions: shaped.instructions,
        tools: bot.tools,
        harnessId: bot.harnessId,
        templateId: bot.templateId,
        image: bot.image,
      });
    }
    onSend(text);
  }

  const notice =
    unformed || pendingDraft ? (
      <div className="chat-shape">
        {unformed && templates.length > 0 && onShape && (
          <div
            className="shape-starters"
            role="group"
            aria-label="Start from a role"
          >
            <p>Or start from a role — talking is enough.</p>
            <div className="shape-starter-row">
              {templates.map((template) => (
                <AgentPill
                  key={template.templateId}
                  bot={{
                    name: template.name,
                    color: template.color,
                    image: null,
                  }}
                  variant="starter"
                  onClick={() => {
                    void onShape(draftFromTemplate(template));
                  }}
                />
              ))}
            </div>
          </div>
        )}
        {pendingDraft && onAcceptDraft && (
          <div className="bot-proposal" role="status">
            <AgentPill
              bot={{
                name: pendingDraft.name,
                color: asBotColor(pendingDraft.color),
                image: null,
              }}
              variant="mention"
            />
            <p>
              {pendingDraft.sourceBotName ?? "A crew member"} proposed{" "}
              {pendingDraft.name} from this chat.
              {pendingDraft.status === "stale"
                ? " This proposal is stale — review it before starting."
                : " Start chatting to add them to the crew."}
            </p>
            <div className="bot-proposal-acts">
              <button
                type="button"
                className="btn primary"
                onClick={() => onAcceptDraft(pendingDraft.draftId)}
              >
                Start chatting
              </button>
              {onReviewDraft && (
                <button
                  type="button"
                  className="btn"
                  onClick={() => onReviewDraft(pendingDraft.draftId)}
                >
                  Settings
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    ) : null;

  return (
    <Conversation
      header={
        <div className="chat-head">
          <AgentPill bot={bot} variant="title" state={runState} />
          {others.length > 0 && onSelectBot && (
            <div
              className="agent-presence"
              role="group"
              aria-label="Agents in chat"
            >
              {others.map((other) => (
                <AgentPill
                  key={other.id}
                  bot={other}
                  variant="presence"
                  onClick={() => onSelectBot(other.id)}
                />
              ))}
            </div>
          )}
          <div className="chat-head-end">
            {onOpenSettings && (
              <button
                type="button"
                className="chat-settings"
                aria-label={`Customize ${bot.name}`}
                title="Advanced settings"
                onClick={onOpenSettings}
              >
                <GearIcon />
              </button>
            )}
            <HostPicker host={host} onPick={onPickHost} />
          </div>
        </div>
      }
      items={displayItems}
      bots={bots}
      onSelectBot={onSelectBot}
      composerPlaceholder={
        unformed ? "Name them, and say what they do" : `Message ${bot.name}`
      }
      onSend={(text) => {
        void handleSend(text);
      }}
      onAction={onAction}
      onReact={onReact}
      busy={busy}
      queued={queued}
      onCancel={onCancel}
      error={error}
      disabled={disabled}
      notice={notice}
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
  bots,
  templates,
  pendingDraft,
  host,
  onPickHost,
  onShape,
  onSelectBot,
  onOpenSettings,
  onAcceptDraft,
  onReviewDraft,
}: {
  client: HostClient;
  bot: Bot;
  bots?: readonly Bot[];
  templates?: readonly BotTemplate[];
  pendingDraft?: BotDraftView | null;
  host: HostTarget;
  onPickHost?: (hostId: string) => void;
  onShape?: (draft: BotDraft) => void | Promise<void>;
  onSelectBot?: (botId: string) => void;
  onOpenSettings?: () => void;
  onAcceptDraft?: (draftId: string) => void;
  onReviewDraft?: (draftId: string) => void;
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
          setOpenError(hostErrorText(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [client, bot.id]);

  const { stream, error, send, cancel, answer, react } = useThreadTranscript(
    client,
    threadId,
  );

  return (
    <ChatView
      bot={bot}
      bots={bots}
      templates={templates}
      pendingDraft={pendingDraft}
      host={host}
      items={stream.items}
      onSend={send}
      onShape={onShape}
      onSelectBot={onSelectBot}
      onOpenSettings={onOpenSettings}
      onAcceptDraft={onAcceptDraft}
      onReviewDraft={onReviewDraft}
      // The buttons on a permission card are the agent's own ACP options, and
      // this is what carries the one the user pressed back to it (#20).
      onAction={answer}
      onReact={react}
      onPickHost={onPickHost}
      busy={stream.busy}
      queued={stream.queued}
      onCancel={cancel}
      error={openError ?? error}
      disabled={threadId === null}
    />
  );
}
