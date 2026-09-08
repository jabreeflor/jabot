/**
 * Renderer helpers for the browser suite.
 *
 * Locators are role/label first. Readiness is the connected shell — Settings
 * visible, "Connecting to host…" gone — not merely HTTP 200 on the page.
 */
import { mkdirSync } from "node:fs";
import path from "node:path";

import { expect, type Page } from "@playwright/test";

import {
  ONBOARDING_KEY,
  type OnboardingProfile,
} from "../../src/onboarding/state";

/** Same profile `scripts/dev/shot.mjs` and unit tests seed so the shell opens. */
export const BROWSER_ONBOARDED: OnboardingProfile = {
  version: 1,
  userName: "Jabree Flor",
  harnessId: null,
  skipped: false,
  completedAt: "2026-01-01T00:00:00.000Z",
};

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

export async function waitForConnected(page: Page): Promise<void> {
  await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
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

/** Write a PNG under `JABOT_BROWSER_EVIDENCE` when that dir is set. */
export async function captureEvidence(page: Page, name: string): Promise<void> {
  const dir = process.env.JABOT_BROWSER_EVIDENCE;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  await page.screenshot({ path: path.join(dir, `${name}.png`) });
}

export async function sendComposer(
  page: Page,
  text: string,
  botName = "Chief",
): Promise<void> {
  const box = composer(page, botName);
  await expect(box).toBeVisible();
  // LiveChatView disables the field until crew/thread resolves; a send before
  // that is a silent no-op (the form clears, the host never hears it).
  await expect(box).toBeEnabled();
  await box.fill(text);
  await box.press("Enter");
}
