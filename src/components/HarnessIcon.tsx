//! A mark per harness, where the picker used to draw a coloured dot.
//!
//! The dot carried one bit — *which* accent — and made every card look the
//! same until you read it. These are the same accents (`--dot`, still set by
//! the caller) drawn as the shape each engine is actually known by, so New
//! Chat is scannable at a glance and the header chip says what is behind a
//! thread without being read.
//!
//! **Drawn here, not shipped from a brand kit.** Every path below was written
//! for this file, in the stroke style the rest of `Icon.tsx` uses, and each is
//! a simplification rather than an official asset — a 16px glyph cannot be
//! anything else. They identify third-party products in a picker that exists
//! to choose between them; if a vendor's real mark is ever vendored in, it
//! replaces one function here and nothing else.
//!
//! **An unknown id still gets a glyph.** Tier-3 harnesses are whatever a user
//! brought (#13), and there is no mark to know for them, so they get the
//! terminal the harness *is*. Nothing here can fail to render.

import type { ReactNode } from "react";

type MarkProps = { className?: string };

/** The shared frame: stroked, currentColor, and decorative. The control around
    a mark carries the accessible name — the label is right beside it. */
function Mark({
  className,
  width = 1.9,
  children,
}: MarkProps & { width?: number; children: ReactNode }) {
  return (
    <svg
      className={["hmark", className].filter(Boolean).join(" ")}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={width}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

/**
 * The mark for one harness id, and never nothing.
 *
 * Keyed on the catalog's own ids (`harness/catalog.rs`), which are compiled in
 * for tiers 1 and 2 and therefore stable. Anything else — a custom harness, an
 * id from a newer host than this renderer — falls through to the terminal.
 */
export function HarnessMark({
  harnessId,
  className,
}: {
  harnessId: string;
  className?: string;
}) {
  switch (harnessId) {
    case "claude":
      return <ClaudeMark className={className} />;
    case "codex":
      return <CodexMark className={className} />;
    case "pi":
      return <PiMark className={className} />;
    case "hermes":
      return <HermesMark className={className} />;
    case "openclaw":
      return <OpenClawMark className={className} />;
    default:
      return <CustomHarnessMark className={className} />;
  }
}

/** Claude: the burst, as five strokes through one centre. */
function ClaudeMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={2}>
      <path d="M2.5 12h19" />
      <path d="M4.3 6.4l15.4 11.2" />
      <path d="M9.1 3l5.8 18" />
      <path d="M14.9 3L9.1 21" />
      <path d="M19.7 6.4L4.3 17.6" />
    </Mark>
  );
}

/** Codex: OpenAI's knot, as three loops woven around one opening. */
function CodexMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={1.6}>
      <ellipse cx="12" cy="12" rx="9.2" ry="5.4" transform="rotate(30 12 12)" />
      <ellipse cx="12" cy="12" rx="9.2" ry="5.4" transform="rotate(90 12 12)" />
      <ellipse cx="12" cy="12" rx="9.2" ry="5.4" transform="rotate(150 12 12)" />
    </Mark>
  );
}

/** Pi: the letter, which is the whole name. */
function PiMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={2}>
      <path d="M5 7.5h14" />
      <path d="M9.6 7.5c0 5.4-.4 8-2.3 10.6" />
      <path d="M15 7.5v8.2c0 1.8 1 2.5 2.6 2.1" />
    </Mark>
  );
}

/** Hermes: the messenger's winged staff — wings up, one coil, nothing else. */
function HermesMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={1.8}>
      <circle cx="12" cy="4" r="1.6" />
      <path d="M12 6.2V20.5" />
      <path d="M11.2 9.1C9 9.6 6.3 8.6 4.4 6.2" />
      <path d="M12.8 9.1c2.2.5 4.9-.5 6.8-2.9" />
      <path d="M9.4 13.4c1.4 1.7 3.8 1.7 5.2 0" />
      <path d="M9.4 17.2c1.4 1.7 3.8 1.7 5.2 0" />
    </Mark>
  );
}

/** OpenClaw: three talons, closing. */
function OpenClawMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={1.9}>
      <path d="M4.6 4.8c-1 5.6.9 10.8 5.6 14.4" />
      <path d="M12 3.8c-.9 5.6-.6 10.9.6 15.6" />
      <path d="M19.4 4.8c1 5.6-.9 10.8-5.6 14.4" />
    </Mark>
  );
}

/** Anything a user brought: the terminal it runs in. */
function CustomHarnessMark({ className }: MarkProps) {
  return (
    <Mark className={className} width={1.8}>
      <rect x="3" y="5" width="18" height="14" rx="3" />
      <path d="M8 10.5l2.4 2.1L8 14.7" />
      <path d="M13.4 15h3.2" />
    </Mark>
  );
}
