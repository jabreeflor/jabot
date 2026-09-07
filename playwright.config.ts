import { defineConfig, devices } from "@playwright/test";

/**
 * Browser tests against the real Rust host.
 *
 * The suite does **not** start Vite here. Each test owns a foreground Vite
 * process (and therefore a jabot-hostd, SQLite dir, and port) through
 * `tests/browser/fixtures.ts`. Playwright `webServer` would share one
 * developer-shaped process; `scripts/live.sh` is also out — `smoke`/`reset`
 * wipe `.jabot-dev/data`.
 *
 * `@playwright/test` and `playwright-core` stay on the same version in
 * package.json (shot.mjs uses the latter). Node is whatever CI uses
 * (`.github/workflows/ci.yml`); #215 may bump that major and this file
 * must not pin a different one.
 */
export default defineConfig({
  testDir: "./tests/browser",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: process.env.CI
    ? [
        ["github"],
        ["list"],
        ["html", { open: "never", outputFolder: "playwright-report" }],
        ["junit", { outputFile: "test-results/junit.xml" }],
      ]
    : [["list"], ["html", { open: "never", outputFolder: "playwright-report" }]],
  outputDir: "test-results",
  globalSetup: "./tests/browser/global-setup.ts",
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    ignoreHTTPSErrors: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      // Renderer compatibility only — not native WKWebView / Tauri.
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
    },
  ],
});
