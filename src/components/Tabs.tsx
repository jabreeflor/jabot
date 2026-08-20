//! The pill tab row on Inbox and Pull Requests. The prototype only toggled a
//! class; here the selected tab is state the view filters by, and the tabs are
//! a real tablist so arrow keys and screen readers behave.
//!
//! Roving tabindex means Tab reaches the row once and lands on the selected
//! tab; the arrows are then the only way to the others, so they are not
//! decoration — without them the filters would be mouse-only.

import { useRef, type KeyboardEvent } from "react";

export interface TabSpec<T extends string> {
  id: T;
  label: string;
  /** Shown after the label, e.g. "Open · 4". Omitted when zero is not news. */
  count?: number;
}

/** The id of a tab's button, so its panel can name it with `aria-labelledby`. */
export function tabButtonId(panelId: string, tabId: string): string {
  return `${panelId}-tab-${tabId}`;
}

export function Tabs<T extends string>({
  label,
  panelId,
  tabs,
  value,
  onChange,
}: {
  label: string;
  panelId: string;
  tabs: readonly TabSpec<T>[];
  value: T;
  onChange: (id: T) => void;
}) {
  const buttons = useRef(new Map<string, HTMLButtonElement>());

  // Selection follows focus, which is what a tablist of cheap filters should
  // do: arrowing to "Merged" shows merged PRs, no second keystroke.
  function move(event: KeyboardEvent<HTMLDivElement>) {
    const from = tabs.findIndex((tab) => tab.id === value);
    if (from < 0) return;
    const to = targetIndex(event.key, from, tabs.length);
    if (to === null) return;

    event.preventDefault();
    const next = tabs[to];
    onChange(next.id);
    buttons.current.get(next.id)?.focus();
  }

  return (
    <div className="tabs" role="tablist" aria-label={label} onKeyDown={move}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          id={tabButtonId(panelId, tab.id)}
          type="button"
          className="tab"
          role="tab"
          aria-selected={value === tab.id}
          aria-controls={panelId}
          tabIndex={value === tab.id ? 0 : -1}
          ref={(node) => {
            if (node) buttons.current.set(tab.id, node);
            else buttons.current.delete(tab.id);
          }}
          onClick={() => onChange(tab.id)}
        >
          {tab.count === undefined ? tab.label : `${tab.label} · ${tab.count}`}
        </button>
      ))}
    </div>
  );
}

/** Horizontal tablist keys, wrapping at both ends. Null means "not ours". */
function targetIndex(key: string, from: number, count: number): number | null {
  switch (key) {
    case "ArrowRight":
      return (from + 1) % count;
    case "ArrowLeft":
      return (from - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}
