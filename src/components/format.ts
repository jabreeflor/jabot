//! Display formatting for timestamps the host stores as ISO text.
//!
//! Inbox and Pull Request rows show *when*, not *what date* — a clock time for
//! today, a weekday inside the week, a date beyond it. The clock half defers to
//! the user's locale (12h vs 24h is a system preference, not a design choice).

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function formatWhen(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return "";

  const delta = now.getTime() - then.getTime();
  if (delta < MINUTE) return "now";
  if (delta < HOUR) return `${Math.floor(delta / MINUTE)}m`;

  const startOfToday = new Date(now).setHours(0, 0, 0, 0);
  const thenDay = new Date(then).setHours(0, 0, 0, 0);
  const daysBack = Math.round((startOfToday - thenDay) / DAY);

  if (daysBack <= 0) {
    return then.toLocaleTimeString(undefined, {
      hour: "numeric",
      minute: "2-digit",
    });
  }
  if (daysBack === 1) return "Yesterday";
  if (daysBack < 7) {
    return then.toLocaleDateString(undefined, { weekday: "short" });
  }
  return then.toLocaleDateString(undefined, { month: "numeric", day: "numeric" });
}

/** "JB" for the me-row avatar. Falls back to one letter for a mononym. */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  const first = words[0][0];
  const last = words.length > 1 ? words[words.length - 1][0] : "";
  return (first + last).toUpperCase();
}
