//! The prototype's icon set, verbatim paths — plus the glyphs that used to be
//! typed as characters (＋, ✓, ✗, ●, 🖥, 🎙, …) and now render as SVG so they
//! look the same on every platform instead of falling back to whatever the
//! system's symbol or emoji font does.
//!
//! They are drawn, not imported, because a sprite dependency would be more
//! machinery than SVG. Every icon is decorative — the control around it
//! carries the accessible name — so all of them are hidden from the
//! accessibility tree.

import type { ReactNode } from "react";

type IconProps = { className?: string };

function Stroke({
  className,
  width = 2.4,
  children,
}: IconProps & { width?: number; children: ReactNode }) {
  return (
    <svg
      className={className}
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

export function SearchIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.5}>
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </Stroke>
  );
}

export function NewChatIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <path d="M11 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5" />
      <path d="M18.4 2.6a2.1 2.1 0 0 1 3 3L13 14l-4 1 1-4Z" />
    </Stroke>
  );
}

export function PullRequestIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <circle cx="6" cy="6" r="2.6" />
      <circle cx="6" cy="18" r="2.6" />
      <circle cx="18" cy="18" r="2.6" />
      <path d="M6 8.6v6.8" />
      <path d="M13.5 6H16a2 2 0 0 1 2 2v7.4" />
      <path d="M15.5 3.5 13 6l2.5 2.5" />
    </Stroke>
  );
}

/** The merged variant curves back into the base branch instead of arrowing in. */
export function PullRequestMergedIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <circle cx="6" cy="6" r="2.6" />
      <circle cx="6" cy="18" r="2.6" />
      <circle cx="18" cy="18" r="2.6" />
      <path d="M6 8.6v6.8" />
      <path d="M6 9a9 9 0 0 0 9 9" />
    </Stroke>
  );
}

export function InboxIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <path d="M22 12h-5.5l-1.5 2.5h-6L7.5 12H2" />
      <path d="M5.4 5.6 2 12v5a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-5l-3.4-6.4A2 2 0 0 0 16.8 4.5H7.2a2 2 0 0 0-1.8 1.1Z" />
    </Stroke>
  );
}

/** Schedules. A clock, because the thing a schedule is about is a time. */
export function ClockIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5.2l3.2 2" />
    </Stroke>
  );
}

export function FolderIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <path d="M3.2 7.2V17a2 2 0 0 0 2 2h13.6a2 2 0 0 0 2-2v-7.2a2 2 0 0 0-2-2h-6.9l-1.9-2.3a2 2 0 0 0-1.5-.7H5.2a2 2 0 0 0-2 2.4Z" />
      <path d="M3.2 9.8h17.6" />
    </Stroke>
  );
}

export function ChevronDownIcon({ className }: IconProps) {
  return (
    <Stroke className={className}>
      <path d="M6 9l6 6 6-6" />
    </Stroke>
  );
}

/** A prompt caret: what a code session is, as opposed to a bot with a face. */
export function CodeSessionIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.9}>
      <path d="M4 17l6-5-6-5" />
      <path d="M12 19h8" />
    </Stroke>
  );
}

export function ArchiveIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <rect x="2" y="4" width="20" height="5" rx="1" />
      <path d="M4 9v9a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9" />
      <path d="M10 13h4" />
    </Stroke>
  );
}

export function TrashIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={1.8}>
      <path d="M3 6h18" />
      <path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
    </Stroke>
  );
}

/** The host affordance in the chat header: this Mac, drawn as a monitor. */
export function MonitorIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2}>
      <rect x="2.5" y="4" width="19" height="13" rx="2" />
      <path d="M8.5 21h7" />
      <path d="M12 17v4" />
    </Stroke>
  );
}

/** The composer's decorative mic. */
export function MicIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2}>
      <rect x="9" y="2.5" width="6" height="11" rx="3" />
      <path d="M5.5 11.5a6.5 6.5 0 0 0 13 0" />
      <path d="M12 18v3.5" />
    </Stroke>
  );
}

/** Stop, in the mic's place while a turn is running. */
export function StopIcon({ className }: IconProps) {
  return (
    <Stroke className={className}>
      <rect
        x="7"
        y="7"
        width="10"
        height="10"
        rx="1.5"
        fill="currentColor"
        stroke="none"
      />
    </Stroke>
  );
}

export function PlusIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </Stroke>
  );
}

/** A check: a passing CI check, a finished tool call. */
export function CheckIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={3}>
      <path d="M4.5 12.5l5 5.5 10-11.5" />
    </Stroke>
  );
}

/** An ✗: a failed check or tool call — and, smaller, the dismiss button. */
export function CrossIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.8}>
      <path d="M6 6l12 12" />
      <path d="M18 6L6 18" />
    </Stroke>
  );
}

/** A filled dot: something running, a thread's status pip. */
export function DotIcon({ className }: IconProps) {
  return (
    <Stroke className={className}>
      <circle cx="12" cy="12" r="7" fill="currentColor" stroke="none" />
    </Stroke>
  );
}

/** A dotted ring: pending — the dot's outline until something is running. */
export function RingIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.2}>
      <circle cx="12" cy="12" r="6.5" strokeDasharray="2.6 3.1" />
    </Stroke>
  );
}

/** The eight-spoked spark on the "Long-running" pill. */
export function SparkIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.6}>
      <path d="M12 3.5v17" />
      <path d="M3.5 12h17" />
      <path d="M6 6l12 12" />
      <path d="M18 6L6 18" />
    </Stroke>
  );
}

/** The toolblock's line marker, in the terminal's ▸ shape. */
export function CaretRightIcon({ className }: IconProps) {
  return (
    <Stroke className={className}>
      <path d="M9 6.2l7.6 5.8L9 17.8Z" fill="currentColor" stroke="none" />
    </Stroke>
  );
}

/** Send: the composer's submit, and the one on the schedule prompt (#25). */
export function ArrowUpIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.6}>
      <path d="M12 19.5V5" />
      <path d="M5.5 11.5L12 5l6.5 6.5" />
    </Stroke>
  );
}

/** Back: out of the schedule prompt and into the list behind it. */
export function ArrowLeftIcon({ className }: IconProps) {
  return (
    <Stroke className={className} width={2.4}>
      <path d="M19 12H5" />
      <path d="M11.5 5.5L5 12l6.5 6.5" />
    </Stroke>
  );
}
