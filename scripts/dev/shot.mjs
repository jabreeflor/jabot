#!/usr/bin/env node
// Drive the live renderer and take a picture of it.
//
//   node scripts/dev/shot.mjs --out docs/img/<feature>/after.png [steps…]
//
// The page is the one `scripts/live.sh up` serves (http://127.0.0.1:1420), the
// browser is the Chromium that playwright-core knows about, and the shot is
// taken only after the sidebar's host line reports a live host — a picture
// of "Connecting to host…" is not evidence of anything.
//
// Steps run in order, each one a flag:
//
//   --click <selector>          click it (Playwright selector syntax, so
//                               `text=Inbox`, `role=button[name="Add folder"]`
//                               and CSS all work)
//   --fill <selector> <text>    type into it (two arguments)
//   --press <key>               a key on the focused element (Enter, Escape…)
//   --wait <selector>           wait until it is visible
//   --wait-text <text>          wait until the page shows this text
//   --sleep <ms>                a fixed pause, when nothing better exists
//   --rpc <json>                POST one JSON-RPC request to /__jabot/rpc
//                               before the page loads (seeding)
//
// Options:
//
//   --out <path>                where the PNG goes (required)
//   --url <url>                 default http://127.0.0.1:1420
//   --viewport <w>x<h>          default 1280x800
//   --full-page                 the whole scrollable page, not the viewport
//   --first-run                 do not seed the onboarding record; show setup
//   --timeout <ms>              per-step and readiness limit, default 15000
//
// Exit code is 0 only if the shot was written. Anything else prints why.

import { mkdirSync } from "node:fs";
import path from "node:path";

import { chromium } from "playwright-core";

const ONBOARDING_KEY = "jabot.onboarding.v1";
// Mirrors tests/support/onboarding.ts: the profile a unit test seeds so <App/>
// renders the shell instead of first-run setup.
const ONBOARDED = {
  version: 1,
  userName: "Jabree Flor",
  harnessId: null,
  skipped: false,
  completedAt: "2026-01-01T00:00:00.000Z",
};

function usage(message) {
  console.error(`shot: ${message}`);
  console.error("usage: node scripts/dev/shot.mjs --out <png> [--url u] [--viewport WxH] [steps…]");
  process.exit(2);
}

function parse(argv) {
  const options = {
    out: null,
    url: process.env.JABOT_LIVE_URL ?? "http://127.0.0.1:1420",
    viewport: { width: 1280, height: 800 },
    fullPage: false,
    firstRun: false,
    timeout: 15_000,
    rpc: [],
  };
  const steps = [];
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) usage(`${flag} needs a value`);
      return v;
    };
    switch (flag) {
      case "--out":
        options.out = value();
        break;
      case "--url":
        options.url = value();
        break;
      case "--viewport": {
        const m = /^(\d+)x(\d+)$/.exec(value());
        if (!m) usage("--viewport wants WIDTHxHEIGHT");
        options.viewport = { width: Number(m[1]), height: Number(m[2]) };
        break;
      }
      case "--full-page":
        options.fullPage = true;
        break;
      case "--first-run":
        options.firstRun = true;
        break;
      case "--timeout":
        options.timeout = Number(value());
        break;
      case "--rpc":
        options.rpc.push(value());
        break;
      case "--click":
      case "--wait":
      case "--wait-text":
      case "--press":
        steps.push({ kind: flag.slice(2), arg: value() });
        break;
      case "--sleep":
        steps.push({ kind: "sleep", arg: Number(value()) });
        break;
      case "--fill": {
        const selector = value();
        const text = value();
        steps.push({ kind: "fill", selector, text });
        break;
      }
      case "-h":
      case "--help":
        console.log("see the header of scripts/dev/shot.mjs");
        process.exit(0);
      // falls through
      default:
        usage(`unknown flag ${flag}`);
    }
  }
  if (!options.out) usage("--out is required");
  return { options, steps };
}

async function seed(url, requests) {
  for (const body of requests) {
    let request;
    try {
      request = JSON.parse(body);
    } catch {
      usage(`--rpc is not JSON: ${body}`);
    }
    if (request.jsonrpc === undefined) request.jsonrpc = "2.0";
    if (request.id === undefined) request.id = "shot";
    const res = await fetch(new URL("/__jabot/rpc", url), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    });
    const answer = await res.json();
    if (answer.error) {
      throw new Error(`rpc ${request.method}: ${answer.error.message} (${answer.error.code})`);
    }
    console.log(`rpc ${request.method}: ok`);
  }
}

async function main() {
  const { options, steps } = parse(process.argv.slice(2));

  const status = await fetch(new URL("/__jabot/host", options.url))
    .then((r) => r.json())
    .catch((err) => {
      throw new Error(`no dev server at ${options.url} (${err.message}); run scripts/live.sh up`);
    });
  if (!status.running) {
    throw new Error(
      `dev server is up but jabot-hostd is not: ${JSON.stringify(status.exit)} ${status.stderr.join(" | ")}`,
    );
  }

  await seed(options.url, options.rpc);

  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({
      viewport: options.viewport,
      deviceScaleFactor: 2,
      colorScheme: "dark",
    });
    if (!options.firstRun) {
      await context.addInitScript(
        ([key, profile]) => {
          window.localStorage.setItem(key, JSON.stringify(profile));
        },
        [ONBOARDING_KEY, ONBOARDED],
      );
    }
    const page = await context.newPage();
    page.setDefaultTimeout(options.timeout);
    const consoleErrors = [];
    page.on("pageerror", (err) => consoleErrors.push(String(err)));

    await page.goto(options.url, { waitUntil: "networkidle" });

    if (!options.firstRun) {
      // The sidebar's host line is `.host` and gains `.bad` on any failure;
      // "Connecting to host…" is the state before either. Wait for the live
      // one, so a screenshot cannot be taken of a renderer that never reached
      // the host.
      await page.waitForFunction(
        () => {
          const el = document.querySelector(".host");
          return el && !el.classList.contains("bad") && !el.textContent.includes("Connecting");
        },
        undefined,
        { timeout: options.timeout },
      );
      const hostLine = await page.locator(".host").textContent();
      console.log(`host: ${hostLine}`);
    }

    for (const step of steps) {
      switch (step.kind) {
        case "click":
          await page.locator(step.arg).first().click();
          break;
        case "fill":
          await page.locator(step.selector).first().fill(step.text);
          break;
        case "press":
          await page.keyboard.press(step.arg);
          break;
        case "wait":
          await page.locator(step.arg).first().waitFor({ state: "visible" });
          break;
        case "wait-text":
          await page.getByText(step.arg).first().waitFor({ state: "visible" });
          break;
        case "sleep":
          await page.waitForTimeout(step.arg);
          break;
        default:
          throw new Error(`unknown step ${step.kind}`);
      }
      console.log(`step ${step.kind}: ok`);
    }

    mkdirSync(path.dirname(options.out), { recursive: true });
    await page.screenshot({ path: options.out, fullPage: options.fullPage });
    console.log(`wrote ${options.out}`);
    if (consoleErrors.length) {
      console.error(`page errors:\n  ${consoleErrors.join("\n  ")}`);
      process.exitCode = 1;
    }
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(`shot: ${err.message}`);
  process.exit(1);
});
