//! Candidate 05, Watchers: one, two or three eyes, and they follow the page.
//! A bot that needs you stops looking around and looks straight at you. Fill
//! this in from `prototypes/jabot-avatars-characters.html`, the `watcher`
//! renderer and its EYE_PLAN table; the gaze itself is two custom properties
//! set on the document root, not per-eye maths on every avatar.
//!
//! Placeholder body until then, so the switch has six working entries.

import type { JSX } from "react";
import type { CrewRenderProps } from "./crew";
import { eyesFor, mouthFor } from "./face";

export function Watchers(props: CrewRenderProps): JSX.Element {
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
