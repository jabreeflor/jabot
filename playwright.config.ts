import { defineConfig, devices } from "@playwright/test";

const ci = !!process.env.CI;

/**
 * Browser tests against the real Rust host (#231 / #256), plus visual and
 * accessibility projects (#234).
 *
 * The suite does **not** start Vite here. Each test owns a foreground Vite
 * process (and therefore a jabot-hostd, SQLite dir, and port) through
 * `tests/browser/fixtures.ts`. Playwright `webServer` would share one
 * developer-shaped process; `scripts/live.sh` is also out — `smoke`/`reset`
 * wipe `.jabot-dev/data`.
 *
 * Functional checks run in Chromium and a WebKit compatibility subset.
 * Screenshot baselines are per-project so the two engines do not overwrite
 * each other. Neither project is proof of native Tauri or WKWebView.
 *
 * CI never auto-approves snapshots (`updateSnapshots: "none"`). Review
 * changed PNGs by hand; see docs/browser-tests.md.
 *
 * `@playwright/test` and `playwright-core` stay on the same version in
 * package.json (shot.mjs uses the latter). Pinned at 1.63+ because 1.56's
 * extract-zip hangs on Node 26 (Playwright #40724). Node follows CI
 * (`verify` and `browser` jobs in `.github/workflows/ci.yml`) — currently 26.
 */
export default defineConfig({
  testDir: "./tests/browser",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: ci,
  timeout: 90_000,
  expect: {
    timeout: 15_000,
    toHaveScreenshot: {
      // No broad tolerance. Unexplained drift is a failure.
      maxDiffPixels: 0,
      animations: "disabled",
      caret: "hide",
      scale: "css",
    },
  },
  updateSnapshots: "none",
  reporter: ci
    ? [
        ["github"],
        ["list"],
        ["html", { open: "never", outputFolder: "playwright-report" }],
        ["junit", { outputFile: "test-results/junit.xml" }],
      ]
    : [
        ["list"],
        ["html", { open: "never", outputFolder: "playwright-report" }],
      ],
  outputDir: "test-results",
  snapshotPathTemplate:
    "{testDir}/__screenshots__/{projectName}/{testFileName}/{arg}{ext}",
  globalSetup: "./tests/browser/global-setup.ts",
  use: {
    locale: "en-US",
    timezoneId: "UTC",
    colorScheme: "dark",
    viewport: { width: 1180, height: 780 },
    deviceScaleFactor: 1,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    ignoreHTTPSErrors: true,
  },
  projects: [
    {
      name: "chromium",
      grepInvert: /@visual/,
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "chromium-visual",
      grep: /@visual/,
      use: { ...devices["Desktop Chrome"] },
    },
    {
      // Renderer compatibility only — not native WKWebView / Tauri.
      name: "webkit",
      grepInvert: /@visual/,
      use: { ...devices["Desktop Safari"] },
    },
    {
      name: "webkit-visual",
      grep: /@visual/,
      grepInvert: /@chromium-only/,
      use: { ...devices["Desktop Safari"] },
    },
  ],
});
