/**
 * The message box. A real form, so Return submits and the field clears — the
 * prototype's input did nothing at all.
 *
 * Sending is a prop: this component never calls the host. #14 hands it a
 * `session/prompt`.
 */

import { useState, type FormEvent } from "react";

export function Composer({
  placeholder,
  onSend,
  disabled = false,
}: {
  placeholder: string;
  onSend: (text: string) => void;
  disabled?: boolean;
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
        <button
          type="button"
          className="round-btn"
          aria-label="Voice"
          disabled={disabled}
        >
          🎙
        </button>
      </form>
    </div>
  );
}
