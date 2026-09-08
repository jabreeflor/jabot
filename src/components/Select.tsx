//! A dropdown that is actually ours to shape.
//!
//! The native `<select>` popup is the browser's own chrome — on the desktop
//! shell this renders in, that means a flat rectangle with system padding no
//! stylesheet reaches: `border-radius` and `padding` on `<option>` are simply
//! not honoured. Everything else in a modal is a rounded card (`.fold-menu`,
//! `.ctx-menu`), so a field that opens into a hard-cornered strip reads as a
//! different app bolted on. This redraws the popup ourselves — same listbox
//! semantics, but a shape and spacing this stylesheet controls.
//!
//! `variant="chip"` is the ghost control New Chat's context row uses: the
//! same listbox, but a trigger that sits in a metadata line rather than
//! filling a labelled field.

import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import { ChevronDownIcon } from "./Icon";

export interface SelectOption {
  value: string;
  label: string;
  icon?: ReactNode;
  detail?: string;
}

export function Select({
  id,
  value,
  options,
  onChange,
  variant = "field",
  disabled = false,
  "aria-label": ariaLabel,
}: {
  id?: string;
  value: string;
  options: readonly SelectOption[];
  onChange: (value: string) => void;
  variant?: "field" | "chip";
  disabled?: boolean;
  "aria-label"?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const listId = useId();
  const generatedId = useId();
  const triggerId = id ?? generatedId;
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

  // Escape closes the popup, not the surface it lives in. Modal.tsx listens
  // for Escape on `document` itself, capturing — the popup's own listener has
  // to beat it, and a listener on `window` always sees a captured event
  // before one on `document` does, whichever was registered first. Without
  // this, the first Escape while the list is open would leave New Chat.
  useEffect(() => {
    if (!open) return;
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
      triggerRef.current?.focus();
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open]);

  function choose(chosen: string) {
    onChange(chosen);
    setOpen(false);
    triggerRef.current?.focus();
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
    <div
      className={variant === "chip" ? "mselect chip" : "mselect"}
      ref={rootRef}
    >
      <button
        type="button"
        id={triggerId}
        className={
          variant === "chip" ? "mselect-trigger ctx-chip" : "mselect-trigger"
        }
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        disabled={disabled}
        onClick={() => setOpen((was) => !was)}
        ref={triggerRef}
      >
        {selected?.icon}
        <span>{selected?.label ?? ""}</span>
        <ChevronDownIcon className="mselect-chev" />
      </button>
      {open && (
        <ul
          className="mselect-list"
          role="listbox"
          id={listId}
          tabIndex={0}
          // Named from the trigger so an open listbox is not an anonymous
          // ARIA input — axe treats that as serious, and so does a reader.
          aria-labelledby={triggerId}
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
              {option.icon}
              <span>
                {option.label}
                {option.detail && (
                  <span className="mselect-detail">{option.detail}</span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
