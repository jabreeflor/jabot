//! Candidate 04, Pixel pets: eight by eight, hand-drawn, two paints and an
//! ink. Fill this in from `prototypes/jabot-avatars-characters.html`, the
//! `pixelpet` renderer and its SPRITES table — including the part that reads
//! the eye positions back out of the sprite so a state face lands on the
//! right pixels, and the `free`/`grow` rules that stop a lid welding an eye
//! to the nearest ink. This one draws on a 9-unit grid, not 24.
//!
//! Placeholder body until then, so the switch has six working entries.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { eyesFor, mouthFor } from "./face";

export function PixelPets(props: CrewRenderProps): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <g className="rig">
        <circle className="body" cx="12" cy="12.6" r="8.2" />
        <circle className="belly" cx="10.2" cy="10.8" r="5.6" opacity="0.5" />
        <g className="eyes">{eyesFor(props)}</g>
        {mouthFor(props)}
      </g>
    </svg>
  );
}
