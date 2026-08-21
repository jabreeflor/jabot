//! The message box. A real form, so Return submits and the field clears — the
//! prototype's input did nothing at all.
//!
//! Sending is a prop: this component never calls the host. #14 hands it a
//! `session/prompt`, and — while a turn is in flight — a Stop button in place
//! of the decorative mic. The field itself is never disabled mid-turn: talking
//! to a busy thread is the ordinary case, and what happens to what you type is
//! the queue's decision, not the input's.

import { useState, type FormEvent } from "react";

export function Composer({
  placeholder,
  onSend,
  disabled = false,
  busy = false,
  onCancel,
}: {
  placeholder: string;
  onSend: (text: string) => void;
  disabled?: boolean;
  /** A turn is running: offer to stop it. */
  busy?: boolean;
  onCancel?: () => void;
}) {
  const [text, setText] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    setText("");
    onSend(trimmed);
  }

  return (
    <div className="composer">
      <form onSubmit={submit}>
        <button
          type="button"
          className="round-btn"
          aria-label="Attach"
          disabled={disabled}
        >
          ＋
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
            ■
          </button>
        ) : (
          <button
            type="button"
            className="round-btn"
            aria-label="Voice"
            disabled={disabled}
          >
            🎙
          </button>
        )}
      </form>
    </div>
  );
}
