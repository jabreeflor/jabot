//! Candidate 03, Critter kit: body, crest, eyes and mouth dealt from the id,
//! so nobody picks anything and every bot is a different little animal. Fill
//! this in from `prototypes/jabot-avatars-characters.html`, the `critter`
//! renderer and its CBODY and CREST tables.
//!
//! Placeholder body until then, so the switch has six working entries.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { eyesFor, mouthFor } from "./face";

export function CritterKit(props: CrewRenderProps): JSX.Element {
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
