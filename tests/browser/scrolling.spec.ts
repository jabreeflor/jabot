import path from "node:path";

import { openGate, pumpAcpRuntime } from "../support/hostd";
import type { Locator } from "@playwright/test";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  captureEvidence,
  openConnectedApp,
  openThread,
  sendComposer,
  agentBubble,
  userBubble,
} from "./ui";

const CHUNK =
  "Line of streamed output that forces the transcript past the viewport.\n".repeat(
    40,
  );

/** Latest user prompt sits at the top of `.chat-scroll`, not the tail. */
function latestPromptPinnedToTop(scroller: Locator) {
  return scroller.evaluate((el) => {
    const users = el.querySelectorAll<HTMLElement>(".msg.me");
    const prompt = users[users.length - 1];
    if (!prompt) return false;
    const pad = parseFloat(getComputedStyle(el).paddingTop) || 0;
    const delta =
      prompt.getBoundingClientRect().top - el.getBoundingClientRect().top - pad;
    return Math.abs(delta) <= 32;
  });
}

function pinnedToBottom(scroller: Locator) {
  return scroller.evaluate(
    (el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 32,
  );
}

test.describe("scrolling", () => {
  test("pins a new prompt at the top, holds when scrolled away, and jumps to latest", async ({
    page,
    jabot,
  }) => {
    const gate = path.join(jabot.dataDir, "scroll.gate");
    await seedCodeThread(jabot, {
      threadId: "t-scroll",
      title: "Scroll journey",
      runtime: pumpAcpRuntime(gate),
    });
    await page.setViewportSize({ width: 1100, height: 520 });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Scroll journey");

    const scroller = page.locator(".chat-scroll");
    await expect(scroller).toBeVisible();

    await sendComposer(page, "overflow please", "Scroll journey");
    openGate(gate, `chunk:${CHUNK}`);
    await expect(
      agentBubble(page).filter({ hasText: "streamed output" }),
    ).toBeVisible();

    await expect
      .poll(async () =>
        scroller.evaluate((el) => el.scrollHeight - el.clientHeight > 40),
      )
      .toBe(true);

    // New send aligns that prompt at the top. The growing reply must not
    // drag the view to the tail — that is the old bottom-following rule.
    await expect.poll(async () => latestPromptPinnedToTop(scroller)).toBe(true);
    await expect.poll(async () => pinnedToBottom(scroller)).toBe(false);
    await captureEvidence(page, "scroll-following");

    await scroller.evaluate((el) => {
      el.scrollTop = 0;
    });
    await expect(
      page.getByRole("button", { name: "Jump to latest" }),
    ).toBeVisible();

    const held = await scroller.evaluate((el) => el.scrollTop);
    openGate(gate, `chunk:${CHUNK}second-wave`);
    await expect(
      agentBubble(page).filter({ hasText: "second-wave" }),
    ).toBeVisible();

    await expect
      .poll(async () => scroller.evaluate((el) => el.scrollTop))
      .toBe(held);
    await captureEvidence(page, "scroll-held");

    await page.getByRole("button", { name: "Jump to latest" }).click();
    await expect(
      page.getByRole("button", { name: "Jump to latest" }),
    ).toHaveCount(0);
    await expect.poll(async () => pinnedToBottom(scroller)).toBe(true);

    openGate(gate, "end_turn");
    await sendComposer(page, "back to the latest", "Scroll journey");
    await expect(
      userBubble(page).filter({ hasText: "back to the latest" }),
    ).toBeVisible();
    await expect.poll(async () => latestPromptPinnedToTop(scroller)).toBe(true);
    await captureEvidence(page, "scroll-jump-latest");
  });
});
