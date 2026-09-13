//! Compact model picker — the New Chat / composer chip (#296).
//!
//! Options are whatever the harness advertised (Doctor listing, cached ACP
//! `availableModels`, last-used that was previously advertised). The empty
//! value is the harness default. We never invent a vendor menu.

import { Select, type SelectOption } from "./Select";
import type { HarnessCard } from "./types";

export function defaultModelLabel(harnessId?: string): string {
  return harnessId === "opencode" ? "Project default" : "Harness default";
}

/** Last-used wins when it is still advertised. A stale id falls back. */
export function initialModel(harness: HarnessCard): string {
  const last = harness.lastModel ?? "";
  const advertised = harness.models ?? [];
  if (!last) return "";
  if (advertised.length === 0) return last;
  if (advertised.includes(last)) return last;
  return "";
}

export function modelOptions(harness: HarnessCard): SelectOption[] {
  const advertised = harness.models ?? [];
  const last = harness.lastModel ?? "";
  const seen = new Set<string>();
  const options: SelectOption[] = [
    { value: "", label: defaultModelLabel(harness.id) },
  ];
  for (const id of advertised) {
    if (!id || seen.has(id)) continue;
    seen.add(id);
    options.push({ value: id, label: id });
  }
  // A previously used id with no current listing is still a real advertised
  // id from an earlier session — not a fake. Drop it once a listing exists
  // that does not include it.
  if (last && !seen.has(last) && advertised.length === 0) {
    options.push({ value: last, label: last });
  }
  return options;
}

export function modelDisabledReason(harness: HarnessCard): string | null {
  if (harness.available === false) {
    return harness.installHint ?? "Not installed";
  }
  return null;
}

export function ModelChip({
  harness,
  value,
  onChange,
  disabled = false,
}: {
  harness: HarnessCard;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const options = modelOptions(harness);
  const selected = options.find((option) => option.value === value);
  const label = selected?.label ?? defaultModelLabel(harness.id);
  const reason = modelDisabledReason(harness);
  return (
    <span title={reason ?? undefined}>
      <Select
        variant="chip"
        aria-label={`Model: ${label}`}
        value={value}
        options={options}
        onChange={onChange}
        disabled={disabled || reason != null}
      />
    </span>
  );
}
