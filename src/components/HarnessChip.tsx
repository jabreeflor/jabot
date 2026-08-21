//! Which engine is behind this thread. Every bot is an ACP harness session (#6),
//! and a thread can override its bot's default at spawn, so the chip reads the
//! thread's own `harnessId` rather than the bot's.

import type { CSSProperties } from "react";

import type { HarnessCard } from "./types";

export function HarnessChip({
  harnessId,
  harnesses,
}: {
  harnessId: string;
  harnesses: readonly HarnessCard[];
}) {
  const harness = harnesses.find((h) => h.id === harnessId);
  // An unknown id is still worth showing: a custom harness the catalog has not
  // loaded is more useful on screen than a blank chip.
  const style = { "--dot": harness?.accent } as CSSProperties;

  return (
    <span className="harness-chip" style={style}>
      <i />
      {harness?.label ?? harnessId}
    </span>
  );
}
