//! A 3×3 field of dots, the way Cursor marks that something is thinking.
//!
//! Nine cells in a square. Circles light up and go dark on their own clocks
//! so a running thread twinkles; a finished one sits still on a constellation.
//! Colour is no longer how a row says what it is doing — motion is. The word
//! ("running", "done") lives on the control's accessible name, not as a label
//! beside the title.

import type { CSSProperties } from "react";

import type { StatusTone } from "./status";

const CELLS = [0, 1, 2, 3, 4, 5, 6, 7, 8] as const;

export function Sparkle({
  tone,
  title,
  seed = "",
}: {
  tone: StatusTone;
  /** Native tooltip: the word the row no longer prints. */
  title?: string;
  /** Offsets the loop so two live rows do not twinkle in lockstep. */
  seed?: string;
}) {
  const live = tone === "running";
  return (
    <span
      className={live ? "sparkle live" : "sparkle"}
      data-tone={tone}
      data-testid="sparkle"
      title={title}
      aria-hidden="true"
      style={{ "--spark-base": `${offsetMs(seed)}ms` } as CSSProperties}
    >
      {CELLS.map((i) => (
        <span key={i} />
      ))}
    </span>
  );
}

function offsetMs(seed: string): number {
  let hash = 2166136261;
  for (let i = 0; i < seed.length; i++) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % 1800;
}
