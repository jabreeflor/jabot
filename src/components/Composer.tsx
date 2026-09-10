//! The message box. A real form, so Return submits and the field clears — the
//! prototype's input did nothing at all.
//!
//! Sending is a prop: this component never calls the host. #14 hands it a
//! `session/prompt`, and — while a turn is in flight — a Stop button in place
//! of the decorative mic. The field itself is never disabled mid-turn: talking
//! to a busy thread is the ordinary case, and what happens to what you type is
//! the queue's decision, not the input's.

import { useState, type FormEvent } from "react";

import { AgentPill } from "./AgentPill";
import { MicIcon, PlusIcon, StopIcon } from "./Icon";
import type { Bot } from "./types";
import {
  filterMentionBots,
  insertMention,
  mentionQuery,
} from "../views/shape-bot";

export function Composer({
  placeholder,
  onSend,
  disabled = false,
  busy = false,
  onCancel,
  mentionBots,
}: {
  placeholder: string;
  onSend: (text: string) => void;
  disabled?: boolean;
  /** A turn is running: offer to stop it. */
  busy?: boolean;
  onCancel?: () => void;
  /** Crew to offer as @mention pills while the query is open. */
  mentionBots?: readonly Bot[];
}) {
  const [text, setText] = useState("");
  const query = mentionBots ? mentionQuery(text) : null;
  const suggestions =
    query !== null && mentionBots ? filterMentionBots(mentionBots, query) : [];

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    setText("");
    onSend(trimmed);
  }

  function pickMention(bot: Bot) {
    setText(insertMention(text, bot.name));
  }

  return (
    <div className="composer">
      {suggestions.length > 0 && (
        <div
          className="composer-mentions"
          role="group"
          aria-label="Mention an agent"
        >
          {suggestions.map((bot) => (
            <AgentPill
              key={bot.id}
              bot={bot}
              variant="mention"
              actionLabel={`Mention ${bot.name}`}
              onClick={() => pickMention(bot)}
            />
          ))}
        </div>
      )}
      <form onSubmit={submit}>
        <button
          type="button"
          className="round-btn"
          aria-label="Attach"
          disabled={disabled}
        >
          <PlusIcon />
        </button>
        <input
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          disabled={disabled}
          onChange={(event) => setText(event.target.value)}
        />
        {busy && onCancel ? (
          <button
            type="button"
            className="round-btn stop"
            aria-label="Stop"
            title="Stop this turn"
            onClick={onCancel}
          >
            <StopIcon />
          </button>
        ) : (
          <button
            type="button"
            className="round-btn"
            aria-label="Voice"
            disabled={disabled}
          >
            <MicIcon />
          </button>
        )}
      </form>
    </div>
  );
}
