//! The pill tab row on Inbox and Pull Requests. The prototype only toggled a
//! class; here the selected tab is state the view filters by, and the tabs are
//! a real tablist so arrow keys and screen readers behave.

export interface TabSpec<T extends string> {
  id: T;
  label: string;
  /** Shown after the label, e.g. "Open · 4". Omitted when zero is not news. */
  count?: number;
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
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          className="tab"
          role="tab"
          aria-selected={value === tab.id}
          aria-controls={panelId}
          tabIndex={value === tab.id ? 0 : -1}
          onClick={() => onChange(tab.id)}
        >
          {tab.count === undefined ? tab.label : `${tab.label} · ${tab.count}`}
        </button>
      ))}
    </div>
  );
}
