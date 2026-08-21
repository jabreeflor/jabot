//! Overlay shell for New Chat and the bot editor.
//!
//! The prototype toggled a class and left the keyboard behind. A modal here
//! actually behaves: Escape closes it, a click on the backdrop closes it, focus
//! moves inside on open and Tab is trapped so it cannot wander back into the
//! sidebar underneath.

import { useEffect, useId, useRef, type ReactNode } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const headingId = useId();
  const modalRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const modal = modalRef.current;
    if (!modal) return;
    modal.querySelector<HTMLElement>(FOCUSABLE)?.focus();
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;

      const modal = modalRef.current;
      if (!modal) return;
      const focusable = [...modal.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;

      if (event.shiftKey && (active === first || !modal.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [onClose]);

  return (
    // mousedown, not click: releasing a text selection over the backdrop is not
    // a request to throw the form away.
    <div
      className="overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={headingId}
        ref={modalRef}
      >
        <h2 id={headingId}>{title}</h2>
        {children}
      </div>
    </div>
  );
}

/** The small uppercase label above each field. */
export function FieldLabel({
  htmlFor,
  children,
}: {
  htmlFor?: string;
  children: ReactNode;
}) {
  return htmlFor ? (
    <label className="mlab" htmlFor={htmlFor}>
      {children}
    </label>
  ) : (
    <div className="mlab">{children}</div>
  );
}
