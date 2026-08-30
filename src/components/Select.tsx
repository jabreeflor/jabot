//! A dropdown that is actually ours to shape.
//!
//! The native `<select>` popup is the browser's own chrome — on the desktop
//! shell this renders in, that means a flat rectangle with system padding no
//! stylesheet reaches: `border-radius` and `padding` on `<option>` are simply
//! not honoured. Everything else in a modal is a rounded card (`.fold-menu`,
//! `.ctx-menu`), so a field that opens into a hard-cornered strip reads as a
//! different app bolted on. This redraws the popup ourselves — same listbox
//! semantics, but a shape and spacing this stylesheet controls.

import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

import { ChevronDownIcon } from "./Icon";

export interface SelectOption {
  value: string;
  label: string;
}

export function Select({
  id,
  value,
  options,
  onChange,
}: {
  id?: string;
  value: string;
  options: readonly SelectOption[];
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const listId = useId();
  const selected = options.find((option) => option.value === value);

  // Opening always lands on the current value, not wherever the last close
  // left it — the highlight is "what is picked", not "what you last hovered".
  useEffect(() => {
    if (!open) return;
    const index = options.findIndex((option) => option.value === value);
    setActive(index < 0 ? 0 : index);
    listRef.current?.focus();
  }, [open, value, options]);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  // Escape closes the popup, not the modal it lives in. Modal.tsx listens for
  // Escape on `document` itself, capturing — the popup's own listener has to
  // beat it, and a listener on `window` always sees a captured event before
  // one on `document` does, whichever was registered first. Without this, the
  // first Escape while the list is open closes the whole New Chat card.
  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open]);

  function choose(chosen: string) {
    onChange(chosen);
    setOpen(false);
  }

  function onListKeyDown(event: KeyboardEvent<HTMLUListElement>) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setActive((index) => Math.min(index + 1, options.length - 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((index) => Math.max(index - 1, 0));
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        if (options[active]) choose(options[active].value);
        break;
      case "Tab":
        setOpen(false);
        break;
    }
  }

  return (
    <div className="mselect" ref={rootRef}>
      <button
        type="button"
        id={id}
        className="mselect-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((was) => !was)}
      >
        <span>{selected?.label ?? ""}</span>
        <ChevronDownIcon className="mselect-chev" />
      </button>
      {open && (
        <ul
          className="mselect-list"
          role="listbox"
          id={listId}
          tabIndex={-1}
          aria-activedescendant={
            options[active] ? `${listId}-${active}` : undefined
          }
          onKeyDown={onListKeyDown}
          ref={listRef}
        >
          {options.map((option, index) => (
            <li
              key={option.value}
              id={`${listId}-${index}`}
              role="option"
              aria-selected={option.value === value}
              className={index === active ? "active" : undefined}
              onMouseEnter={() => setActive(index)}
              onClick={() => choose(option.value)}
            >
              {option.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
