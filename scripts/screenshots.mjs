//! Reference screenshots of the desktop app, taken against the real renderer.
//!
//!   npm run dev &                  # the renderer, on :1420
//!   node scripts/screenshots.mjs   # writes docs/img/app/*.png
//!
//! Needs Playwright and a Chromium. Neither is a dependency of this repo —
//! nothing in the gate drives a browser, and adding ~300 MB of browser to
//! `npm install` to regenerate eight PNGs is the wrong trade. Install them
//! where they are convenient (`npm i -g playwright && playwright install
//! chromium`, or `npx playwright@1.56 install chromium`) and point NODE_PATH
//! at them if they are global.
//!
//! **The renderer is not patched for these.** The one thing this script
//! supplies is the webview bridge a Tauri build would: `window.
//! __TAURI_INTERNALS__`, answering `host/hello` so the sidebar can name the
//! host it is talking to. Everything else stays unanswered, which is the
//! shell's documented "the host has not said yet" state — the state a preview
//! build and a unit test are also in — and there it renders the fixtures in
//! `src/views/mock-host.ts`. Those fixtures are the curated picture the design
//! work is drawn against, so they are what the reference images should show.
//!
//! Schedules is the exception, and the reason the fake host is not one line:
//! #25 has no renderer fixtures behind it, so an unanswered `schedule/list`
//! leaves the pane empty. It gets host data here, cron-coherent with the
//! frozen clock below.
//!
//! The clock is fixed at `NOW` so "38m" and "2:23 PM" are the same bytes on
//! every regeneration and a re-shoot diffs as the UI change it was for.

import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = process.env.SHOT_OUT ?? join(ROOT, "docs/img/app");
const URL = process.env.SHOT_URL ?? "http://localhost:1420";

/** The macOS window in tauri.conf.json, at 2x so the text survives a zoom. */
const VIEWPORT = { width: 1180, height: 780 };
const SCALE = 2;

/** Thursday, 2:23 PM — the time on Chief's first message. Rendered in UTC,
    which is what a container has; run this with TZ=UTC anywhere else. */
const NOW = new Date("2026-03-12T14:23:00Z");

const HELLO = {
  protocolVersion: 1,
  hostId: "host-local",
  hostName: "Jabree's MacBook Pro",
  hostMode: "desktop",
  version: "0.1.0",
  platform: "macos",
  device: {
    deviceId: "dev-local",
    name: "Jabree's MacBook Pro",
    kind: "desktop",
    role: "owner",
  },
  methods: [],
  notifications: [],
};

// Three jobs that between them draw every state the row has: due and enabled,
// a run the host caught up after the Mac was shut, and one paused by a
// failure. Times agree with the crons, because a row that says "Every Monday"
// over "Next Mar 15" is a screenshot of a bug.
const SCHEDULES = {
  schedules: [
    {
      scheduleId: "sch-digest",
      botId: "writer",
      botName: "Writer",
      name: "Weekly digest",
      cron: "0 9 * * 1",
      prompt:
        "Draft the weekly digest from this week's threads. My voice, under 1,200 words.",
      enabled: true,
      catchUp: "once",
      nextRunAt: "2026-03-16T09:00:00Z",
      lastRunAt: "2026-03-09T09:00:00Z",
      threadId: "writer",
      lastFire: {
        fireId: "f-digest-9",
        scheduleId: "sch-digest",
        threadId: "writer",
        dueAt: "2026-03-09T09:00:00Z",
        firedAt: "2026-03-09T09:00:00Z",
        state: "delivered",
        caughtUp: false,
        skippedCount: 0,
        detail: "1,240 words · waiting on review",
        deliveredAt: "2026-03-09T09:04:00Z",
      },
      recentFires: [],
      createdAt: "2026-01-19T11:00:00Z",
      updatedAt: "2026-03-09T09:04:00Z",
    },
    {
      scheduleId: "sch-inbox",
      botId: "inboxm",
      botName: "Inbox Mgr",
      name: "Morning inbox sweep",
      cron: "30 7 * * 1-5",
      prompt:
        "Clear Gmail to zero. Park anything that needs my voice as a draft.",
      enabled: true,
      catchUp: "once",
      nextRunAt: "2026-03-13T07:30:00Z",
      lastRunAt: "2026-03-12T09:12:00Z",
      threadId: "inboxm",
      lastFire: {
        fireId: "f-inbox-88",
        scheduleId: "sch-inbox",
        threadId: "inboxm",
        dueAt: "2026-03-12T07:30:00Z",
        firedAt: "2026-03-12T09:12:00Z",
        state: "delivered",
        caughtUp: true,
        skippedCount: 0,
        detail: "Mac was shut at 07:30 — ran on wake",
        deliveredAt: "2026-03-12T09:14:00Z",
      },
      recentFires: [],
      createdAt: "2025-12-08T08:00:00Z",
      updatedAt: "2026-03-12T09:14:00Z",
    },
    {
      scheduleId: "sch-deps",
      botId: "code",
      botName: "Code",
      name: "Dependency bump",
      cron: "0 3 * * 6",
      prompt:
        "Bump dependencies in jabot-app, run the full suite, open a PR if it is green.",
      enabled: false,
      catchUp: "skip",
      lastRunAt: "2026-03-07T03:00:00Z",
      lastFire: {
        fireId: "f-deps-12",
        scheduleId: "sch-deps",
        threadId: "auth",
        dueAt: "2026-03-07T03:00:00Z",
        firedAt: "2026-03-07T03:00:00Z",
        state: "failed",
        caughtUp: false,
        skippedCount: 0,
        detail: "npm audit exited 1 — paused until reviewed",
      },
      recentFires: [],
      createdAt: "2026-02-10T20:00:00Z",
      updatedAt: "2026-03-07T03:02:00Z",
    },
  ],
};

/** Runs before the app's own scripts, in the page. */
function bridge({ hello, schedules, onboarded }) {
  const answers = { "host/hello": hello, "schedule/list": schedules };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd, args) => {
      if (cmd !== "host_rpc") return Promise.resolve(null);
      const request = args?.request ?? {};
      if (request.method in answers) {
        return Promise.resolve({
          jsonrpc: "2.0",
          id: request.id,
          result: answers[request.method],
        });
      }
      // Never settles, on purpose: rejecting would put a host error across
      // panes that have perfectly good fixtures to draw instead.
      return new Promise(() => {});
    },
    transformCallback: (callback) => {
      const id = 1;
      window[`_cb_${id}`] = callback;
      return id;
    },
    unregisterCallback: () => {},
    convertFileSrc: (path) => path,
  };

  const KEY = "jabot.onboarding.v1";
  if (onboarded) {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({
        version: 1,
        userName: "Jabree",
        harnessId: "claude",
        skipped: false,
        completedAt: "2026-01-01T00:00:00.000Z",
      }),
    );
  } else {
    window.localStorage.removeItem(KEY);
  }
}

const browser = await chromium.launch({
  args: [
    "--no-sandbox",
    "--force-color-profile=srgb",
    "--font-render-hinting=none",
    "--hide-scrollbars",
  ],
});

/** A fresh profile, so the first-run pass is not looking at the shell's. */
async function open({ onboarded = true } = {}) {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: SCALE,
    colorScheme: "dark",
    reducedMotion: "reduce",
  });
  await context.addInitScript(bridge, {
    hello: HELLO,
    schedules: SCHEDULES,
    onboarded,
  });
  const page = await context.newPage();
  await page.clock.setFixedTime(NOW);
  const errors = [];
  page.on("pageerror", (err) => errors.push(`pageerror: ${err.message}`));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(`console: ${msg.text()}`);
  });
  await page.goto(URL, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1200);
  return { context, page, errors };
}

async function shot(page, name) {
  await page.waitForTimeout(600);
  await page.screenshot({ path: join(OUT, `${name}.png`) });
  process.stdout.write(`  ${name}.png\n`);
}

/** The left rail's sections, which share their labels with the bot tiles. */
const rail = (page, label) =>
  page.locator("button.nav-row").filter({ hasText: label });

mkdirSync(OUT, { recursive: true });
const complaints = [];

{
  const { context, page, errors } = await open();

  await shot(page, "chat");

  await rail(page, "Pull Requests").click();
  await shot(page, "pull-requests");

  await rail(page, "Inbox").click();
  await shot(page, "inbox");

  await rail(page, "Schedules").click();
  await shot(page, "schedules");

  await page.getByRole("button", { name: /^Crew$/ }).click();
  await shot(page, "crew");

  await page.getByRole("button", { name: /Auth migration/ }).first().click();
  await shot(page, "thread");

  await page.getByRole("button", { name: /^New Chat$/ }).click();
  await shot(page, "new-chat");

  complaints.push(...errors);
  await context.close();
}

{
  const { context, page, errors } = await open({ onboarded: false });
  await shot(page, "first-run");
  complaints.push(...errors);
  await context.close();
}

await browser.close();

// A pane can render its shell while everything under it throws, and a
// screenshot of that looks fine. Say so rather than letting it ship quietly.
if (complaints.length > 0) {
  process.stderr.write(`\nthe page complained:\n  ${complaints.join("\n  ")}\n`);
  process.exit(1);
}
