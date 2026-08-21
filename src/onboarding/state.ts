//! The first-run setup record: one versioned localStorage key, written once.
//!
//! Presence of a readable record is the completion flag — there is no separate
//! boolean. The failure directions are deliberate and asymmetric: a store that
//! cannot be *read* is treated as already onboarded (the wrong way to fail is
//! the one that traps the user in setup with no way out), while a store that
//! answered with garbage is treated as a first run (replaying three panes once
//! is cheaper than booting a half-configured shell).
//!
//! localStorage is the wrong long-term home for a display name — a second
//! device or a reinstall loses it, and the host will own a profile eventually.
//! This module is the seam for that: when a `user/profile` RPC exists,
//! `loadOnboarding`/`saveOnboarding` become the fallback path and nothing else
//! in the tree changes.

export const ONBOARDING_KEY = "jabot.onboarding.v1";

/** What a skipper who typed nothing is called. Never stored as "". */
export const DEFAULT_USER_NAME = "You";

export type OnboardingProfile = {
  version: number;
  userName: string;
  /** `null` is "no opinion" — NewChatModal treats "" as a real, unmatchable
      id, so the empty string must never reach `defaultHarnessId`. */
  harnessId: string | null;
  /** Provenance for a future "finish setup" affordance; nothing reads it yet. */
  skipped: boolean;
  completedAt: string;
};

/**
 * The single constructor, and the only place the blank-name loop can be
 * closed: the flow calls this before `onFinish`, and `saveOnboarding` runs it
 * again, so a `userName` of "" or "   " is not producible by either path.
 */
export function makeProfile({
  userName,
  harnessId,
  skipped,
}: {
  userName: string;
  harnessId: string | null;
  skipped: boolean;
}): OnboardingProfile {
  const name = userName.trim();
  return {
    version: 1,
    userName: name === "" ? DEFAULT_USER_NAME : name,
    harnessId: harnessId ? harnessId : null,
    skipped,
    completedAt: new Date().toISOString(),
  };
}

/**
 * `null` means "genuine first run", and the rules are ordered so the only way
 * to get it is a store that answered and had nothing usable in it. Any record
 * with `version >= 1` counts as complete — including versions this build has
 * never heard of, because a downgrade must not re-onboard, and because setup
 * not running means no write, so a newer build's extra fields survive.
 *
 * Pure and side-effect free on purpose: `App` calls it as a lazy `useState`
 * initializer and StrictMode invokes that twice.
 */
export function loadOnboarding(): OnboardingProfile | null {
  let raw: string | null;
  try {
    raw = window.localStorage.getItem(ONBOARDING_KEY);
  } catch {
    // A store that cannot be read (hardened webview, site data disabled)
    // would replay the takeover forever. A nameless-but-usable shell is
    // strictly better than that, so fail toward "already onboarded".
    return {
      version: 1,
      userName: DEFAULT_USER_NAME,
      harnessId: null,
      skipped: true,
      completedAt: "",
    };
  }
  if (raw === null) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return null;
  }
  const record = parsed as Record<string, unknown>;
  const version = record.version;
  if (typeof version !== "number" || !Number.isFinite(version) || version < 1) {
    return null;
  }

  // Fields read defensively: a hand-edited blank name resolves to the default
  // rather than looping back into setup.
  const userName =
    typeof record.userName === "string" && record.userName.trim() !== ""
      ? record.userName.trim()
      : DEFAULT_USER_NAME;
  const harnessId =
    typeof record.harnessId === "string" && record.harnessId !== ""
      ? record.harnessId
      : null;
  return {
    version,
    userName,
    harnessId,
    skipped: record.skipped === true,
    completedAt: typeof record.completedAt === "string" ? record.completedAt : "",
  };
}

/** A storage failure is swallowed: the user still enters the app, and the
    cost is that setup replays next launch — which beats a modal about
    localStorage. */
export function saveOnboarding(profile: OnboardingProfile): void {
  const record = makeProfile({
    userName: profile.userName,
    harnessId: profile.harnessId,
    skipped: profile.skipped,
  });
  try {
    window.localStorage.setItem(ONBOARDING_KEY, JSON.stringify(record));
  } catch {
    // Setup replays next launch; nothing else to do.
  }
}

/** The "Run setup again" entry point. */
export function resetOnboarding(): void {
  try {
    window.localStorage.removeItem(ONBOARDING_KEY);
  } catch {
    // A store that cannot be written cannot be holding a record either.
  }
}
