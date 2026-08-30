//! Seed and clear the first-run record for unit tests.
//!
//! Built on the imported key and type rather than retyped copies — tsconfig
//! includes `tests`, so a rename in src/onboarding/state.ts fails `tsc
//! --noEmit` instead of silently un-seeding every suite.

import { ONBOARDING_KEY, type OnboardingProfile } from "../../src/onboarding/state";

/**
 * Mark this jsdom as already onboarded. `userName` defaults to the value
 * `App` hardcoded before onboarding existed, so any assertion on the sidebar
 * name still holds.
 */
export function seedOnboarded(override: Partial<OnboardingProfile> = {}): void {
  const profile: OnboardingProfile = {
    version: 1,
    userName: "Jabree Flor",
    harnessId: null,
    skipped: false,
    completedAt: "2026-01-01T00:00:00.000Z",
    ...override,
  };
  window.localStorage.setItem(ONBOARDING_KEY, JSON.stringify(profile));
}

/** Back to a genuine first run — the opt-out for suites exercising setup. */
export function clearOnboarding(): void {
  window.localStorage.removeItem(ONBOARDING_KEY);
}
