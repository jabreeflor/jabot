/**
 * Pick the engine. Shared by New Chat (this thread) and the bot editor (this
 * bot's default), because #6 made them the same choice at two scopes: a thread
 * override else `bots.harness_id`.
 *
 * A harness the Doctor could not find is still shown — greying it out and
 * saying how to install it is more useful than pretending it does not exist.
 */

import type { CSSProperties } from "react";

import type { HarnessCard } from "./types";

export function HarnessPicker({
  harnesses,
  value,
  onChange,
  label,
}: {
  harnesses: readonly HarnessCard[];
  value: string;
  onChange: (harnessId: string) => void;
  label: string;
}) {
  return (
    <div className="harness-grid" role="group" aria-label={label}>
      {harnesses.map((harness) => (
        <button
          key={harness.id}
          type="button"
          className="harness-card"
          aria-pressed={value === harness.id}
          onClick={() => onChange(harness.id)}
          style={{ "--dot": harness.accent } as CSSProperties}
        >
          <b>
            <i />
            {harness.label}
          </b>
          <p>
            {harness.available === false ? (
              <span className="missing">
                {harness.installHint ?? "Not installed"}
              </span>
            ) : (
              harness.blurb
            )}
          </p>
        </button>
      ))}
    </div>
  );
}
