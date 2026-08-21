//! Reading an ACP permission request that the host passed through verbatim.
//!
//! `PendingPermissionView.options` and `.subject` are `unknown` on purpose:
//! the host "never invents an option the agent did not offer" (#20), so what
//! arrives is whatever this particular harness sent. A phone therefore has to
//! parse defensively — an agent that ships a new field must not blank the
//! screen of the only device that can answer it.
//!
//! The rule everywhere below: render what we understand, drop what we do not,
//! never substitute. An option with no `optionId` cannot be answered with, so
//! it is not shown; a subject with no title falls back to the thread's name
//! rather than to an invented description of what the agent wants to do.

import type { PendingPermissionView } from "../host/protocol";

/** One button. `kind` is the agent's hint — `allow_once`, `reject_always`, … */
export interface AskOption {
  optionId: string;
  name: string;
  kind?: string;
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}

/** The agent's options, in the agent's order, minus any we could not answer with. */
export function parseAskOptions(options: unknown): AskOption[] {
  if (!Array.isArray(options)) return [];
  const parsed: AskOption[] = [];
  for (const raw of options) {
    const option = record(raw);
    if (!option) continue;
    const optionId = text(option.optionId) ?? text(option.id);
    if (!optionId) continue;
    parsed.push({
      optionId,
      name: text(option.name) ?? text(option.label) ?? optionId,
      kind: text(option.kind),
    });
  }
  return parsed;
}

/**
 * Which option a "reject" affordance should send.
 *
 * The phone never invents a decline: if the agent offered no rejecting option,
 * the honest answer is `cancelled: true` on `permission/reply`, which is a
 * different thing and the host records it as one.
 */
export function rejectOption(options: readonly AskOption[]): AskOption | undefined {
  return options.find((option) => option.kind?.startsWith("reject"));
}

/** The one an "allow" affordance should send, preferring the narrowest grant. */
export function allowOption(options: readonly AskOption[]): AskOption | undefined {
  return (
    options.find((option) => option.kind === "allow_once") ??
    options.find((option) => option.kind?.startsWith("allow"))
  );
}

/**
 * The ask's headline, when it did not come from `permission/pending`.
 *
 * A live `permission/ask` notification carries the agent's `subject` but no
 * title — the host computes one when it writes the record. This is the same
 * derivation (`subject_title` in `host/permission/mod.rs`) so a card built
 * from the notification and the same card after a refresh read the same.
 */
export function askTitle(subject: unknown): string {
  return text(record(subject)?.title) ?? "waiting on your answer";
}

/**
 * A one-line description of what is being asked.
 *
 * The host already computed a title for the record; this only has to cope with
 * the case where it could not, and with the extra context an ACP `toolCall`
 * carries that is worth putting on a small screen — the command, mostly,
 * because "Run ls" and "Run rm -rf /" are the same title.
 */
export function askDetail(ask: PendingPermissionView): string | undefined {
  const subject = record(ask.subject);
  if (!subject) return undefined;
  const direct = text(subject.command) ?? text(subject.description);
  if (direct) return direct;
  const locations = subject.locations;
  if (Array.isArray(locations)) {
    const paths = locations
      .map((location) => text(record(location)?.path))
      .filter((path): path is string => Boolean(path));
    if (paths.length > 0) return paths.join(", ");
  }
  const raw = record(subject.rawInput);
  return raw ? text(raw.command) : undefined;
}
