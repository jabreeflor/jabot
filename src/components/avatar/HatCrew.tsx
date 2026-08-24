//! Candidate 02, Hat crew: one body, and the thing on its head is who it is.
//! The hat breaks the circle, so identity lives in the outline — the channel
//! that survives greyscale and 28px without anybody having to read it. Fill
//! this in from `prototypes/jabot-avatars-characters.html`, the `hatcrew`
//! renderer and its HATS table; note the hats are dealt, not hashed, and that
//! a hatted face calls `eyesFor(props, false)`.
//!
//! Placeholder body until then, so the switch has six working entries.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { eyesFor, mouthFor } from "./face";

export function HatCrew(props: CrewRenderProps): JSX.Element {
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
