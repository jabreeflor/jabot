//! Pinned environment for browser visual and accessibility checks (#234).
//!
//! Window sizes come from `src-tauri/tauri.conf.json`, not from `shot.mjs`.
//! Chromium and WebKit baselines are stored separately; neither engine is
//! evidence of native Tauri / WKWebView behaviour (that remains #235).

import type { OnboardingProfile } from "../../../src/onboarding/state";
import { ONBOARDING_KEY } from "../../../src/onboarding/state";
import { THEME_KEY, type ThemePreference } from "../../../src/theme";

export { ONBOARDING_KEY, THEME_KEY };

/** Shipped default window. */
export const DESKTOP_VIEWPORT = { width: 1180, height: 780 } as const;

/** `minWidth` / `minHeight` on the Tauri window. */
export const MINIMUM_VIEWPORT = { width: 900, height: 600 } as const;

export const VIEWPORTS = {
  desktop: DESKTOP_VIEWPORT,
  minimum: MINIMUM_VIEWPORT,
} as const;

export type WindowSize = keyof typeof VIEWPORTS;

export const PINNED_LOCALE = "en-US";
export const PINNED_TIMEZONE = "UTC";

/** Profile `shot.mjs` and the unit onboarding helper already trust. */
export const ONBOARDED_PROFILE: OnboardingProfile = {
  version: 1,
  userName: "Jabree Flor",
  harnessId: null,
  skipped: false,
  completedAt: "2026-01-01T00:00:00.000Z",
};

export const FAKE_ACP_REPLY = "hello from fake-acp";
export const CHIEF_BOT_ID = "chief";
export const RECRUITER_BOT_ID = "bot-recruiter";
export const CHIEF_THREAD_ID = "bot-chief";
export const RECRUITER_THREAD_ID = "bot-bot-recruiter";

/** Extra harness the visual suite writes so a permission ask is deterministic. */
export const FAKE_ACP_ASK_ID = "fake-acp-ask";

export type CaptureTheme = Extract<ThemePreference, "dark" | "light">;

export const SCREENSHOT_STYLE = `
  *, *::before, *::after {
    animation-duration: 0s !important;
    animation-delay: 0s !important;
    transition-duration: 0s !important;
  }
`;
