import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

import { seedOnboarded } from "./onboarding";

// The unit default is "this Mac has already been through first-run setup".
// SIX suites render <App/> — app, crew-store, fold, folders, inbox-host,
// notifications — and every one of them wants the shell, not the takeover.
// Do not "optimize" this into a per-file beforeEach; that breaks five of them.
// To exercise first run, call clearOnboarding() in your own beforeEach.
beforeEach(() => {
  window.localStorage.clear();
  seedOnboarded();
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
