/**
 * Renderer helpers for the browser suite.
 *
 * Locators are role/label first. Readiness is the connected shell — Settings
 * visible, "Connecting to host…" gone — not merely HTTP 200 on the page.
 */
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, type Locator, type Page } from "@playwright/test";

import {
  ONBOARDING_KEY,
  type OnboardingProfile,
} from "../../src/onboarding/state";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

/** Same profile `scripts/dev/shot.mjs` and unit tests seed so the shell opens. */
export const BROWSER_ONBOARDED: OnboardingProfile = {
  version: 1,
  userName: "Jabree Flor",
  harnessId: null,
  skipped: false,
  completedAt: "2026-01-01T00:00:00.000Z",
};

export const JOURNEY_SHOT_DIR = path.join(
  repoRoot,
  "docs",
  "img",
  "browser-journeys",
);

export async function seedOnboarding(
  page: Page,
  profile: OnboardingProfile = BROWSER_ONBOARDED,
): Promise<void> {
  await page.addInitScript(
    ([key, record]) => {
      window.localStorage.setItem(key, JSON.stringify(record));
    },
    [ONBOARDING_KEY, profile] as const,
  );
}

/** Sidebar Settings — exact, so "Folder settings for …" does not match. */
export function settingsButton(page: Page): Locator {
  return page.getByRole("button", { name: "Settings", exact: true });
}

export async function waitForConnected(page: Page): Promise<void> {
  await expect(settingsButton(page)).toBeVisible();
  await expect(page.getByText("Connecting to host…")).toHaveCount(0);
  await expect(page.locator(".host.bad")).toHaveCount(0);
}

export async function openConnectedApp(
  page: Page,
  baseURL: string,
): Promise<void> {
  await seedOnboarding(page);
  await page.goto(baseURL, { waitUntil: "domcontentloaded" });
  await waitForConnected(page);
}

export async function openFirstRun(page: Page, baseURL: string): Promise<void> {
  await page.goto(baseURL, { waitUntil: "domcontentloaded" });
  await expect(page.getByText("Connecting to host…")).toHaveCount(0, {
    timeout: 30_000,
  });
  await expect(
    page.getByRole("heading", { name: /What should the crew call you/ }),
  ).toBeVisible({ timeout: 30_000 });
}

export function chiefRow(page: Page) {
  return page.getByRole("button", { name: /^Chief\b/ }).first();
}

export function composer(page: Page, botName = "Chief") {
  return page.getByRole("textbox", { name: `Message ${botName}` });
}

export function userBubble(page: Page) {
  return page.locator(".msg.me .bubble");
}

export function agentBubble(page: Page) {
  return page.locator(".msg.bot .bubble");
}

export function threadRow(page: Page, title: string) {
  return page.getByRole("button", {
    name: new RegExp(`^${escapeRegExp(title)},`),
  });
}

export async function openChief(page: Page): Promise<void> {
  await chiefRow(page).click();
  await expect(
    page.getByRole("heading", { level: 2, name: "Chief" }),
  ).toBeVisible();
  await expect(composer(page)).toBeVisible();
}

export async function openThread(page: Page, title: string): Promise<void> {
  await threadRow(page, title).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
}

/** Write a PNG under docs/img/browser-journeys, and also to JABOT_BROWSER_EVIDENCE. */
export async function captureEvidence(page: Page, name: string): Promise<void> {
  mkdirSync(JOURNEY_SHOT_DIR, { recursive: true });
  await page.screenshot({
    path: path.join(JOURNEY_SHOT_DIR, `${name}.png`),
    fullPage: true,
  });
  const extra = process.env.JABOT_BROWSER_EVIDENCE;
  if (extra && extra !== JOURNEY_SHOT_DIR) {
    mkdirSync(extra, { recursive: true });
    await page.screenshot({
      path: path.join(extra, `${name}.png`),
      fullPage: true,
    });
  }
}

export async function sendComposer(
  page: Page,
  text: string,
  botName = "Chief",
): Promise<void> {
  const box = composer(page, botName);
  await expect(box).toBeVisible();
  // LiveChatView disables the field until crew/thread (or thread/open) resolves.
  await expect(box).toBeEnabled();
  const bubble = userBubble(page).filter({ hasText: text });
  // One retry: a submit in the enable/threadId race is swallowed rather than queued.
  for (let attempt = 0; attempt < 2; attempt++) {
    await box.fill(text);
    await box.press("Enter");
    try {
      await expect(bubble.first()).toBeVisible({ timeout: 4_000 });
      return;
    } catch (err) {
      if (attempt === 1) throw err;
    }
  }
}

export async function completeOnboarding(
  page: Page,
  name = "Ada Lovelace",
): Promise<void> {
  await expect(
    page.getByRole("heading", { name: /What should the crew call you/ }),
  ).toBeVisible();
  await page.getByLabel("YOUR NAME").fill(name);
  await page.getByRole("button", { name: "Continue" }).click();

  await expect(
    page.getByRole("heading", { name: "Pick your default engine" }),
  ).toBeVisible();
  // Default card is already Fake ACP. Extra custom harnesses also match
  // /Fake ACP/, so keep the default rather than clicking a card.
  await page.getByRole("button", { name: "Continue" }).click();

  const adapter = page.getByRole("region", { name: "Adapter readiness" });
  if (await adapter.isVisible()) {
    const skip = adapter.getByRole("button", { name: /Skip for now|Continue/ });
    await expect(skip).toBeEnabled({ timeout: 30_000 });
    await skip.click();
  }

  await expect(page.getByRole("heading", { name: "Chief" })).toBeVisible();
  await page.getByRole("button", { name: "Enter JaBot" }).click();
}

export async function skipOnboarding(page: Page): Promise<void> {
  await expect(
    page.getByRole("heading", { name: /What should the crew call you/ }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Skip setup" }).click();
}

export async function expectSettledAgent(
  page: Page,
  text: string | RegExp,
): Promise<void> {
  const bubble = agentBubble(page).filter({ hasText: text }).last();
  await expect(bubble).toBeVisible();
  await expect(bubble).not.toHaveAttribute("data-streaming", "true");
}

export async function expectStreamingAgent(
  page: Page,
  text: string | RegExp,
): Promise<void> {
  const bubble = page
    .locator(".msg.bot .bubble[data-streaming]")
    .filter({ hasText: text });
  await expect(bubble).toBeVisible();
}

export function allowButton(page: Page): Locator {
  return page.getByRole("button", { name: "Allow", exact: true });
}

export function denyButton(page: Page): Locator {
  return page.getByRole("button", { name: "Deny", exact: true });
}

export async function chooseSelectOption(
  page: Page,
  triggerName: string | RegExp,
  optionName: string | RegExp,
): Promise<void> {
  await page.getByRole("button", { name: triggerName }).click();
  await page.getByRole("option", { name: optionName }).click();
}

export async function startFolderSession(
  page: Page,
  folderName: string,
  task: string,
): Promise<void> {
  await page.getByRole("button", { name: "New Chat" }).click();
  await expect(page.getByRole("region", { name: "New Chat" })).toBeVisible();
  await chooseSelectOption(page, /Workspace:/, folderName);
  await chooseSelectOption(page, /Harness:/, /^Fake ACP Custom/);
  await page.getByLabel("Plan, build, or describe a change").fill(task);
  await page.getByRole("button", { name: "Send" }).click();
}

export async function archiveThread(page: Page, title: string): Promise<void> {
  await page
    .getByRole("button", { name: new RegExp(`^${escapeRegExp(title)}`) })
    .click({
      button: "right",
    });
  await page.getByRole("menuitem", { name: "Archive" }).click();
}

export async function deleteThread(page: Page, title: string): Promise<void> {
  await page
    .getByRole("button", { name: new RegExp(`^${escapeRegExp(title)}`) })
    .click({
      button: "right",
    });
  await page.getByRole("menuitem", { name: "Delete" }).click();
}

export function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
