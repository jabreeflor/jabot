import path from "node:path";

import { openGate, pumpAcpRuntime } from "../support/hostd";
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

test.describe("scrolling", () => {
  test("follows the tail, holds when scrolled up, and jumps back", async ({
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

    await expect
      .poll(async () =>
        scroller.evaluate(
          (el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 32,
        ),
      )
      .toBe(true);
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
    await expect
      .poll(async () =>
        scroller.evaluate(
          (el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 32,
        ),
      )
      .toBe(true);

    openGate(gate, "end_turn");
    await sendComposer(page, "back to the tail", "Scroll journey");
    await expect(
      userBubble(page).filter({ hasText: "back to the tail" }),
    ).toBeVisible();
    await expect
      .poll(async () =>
        scroller.evaluate(
          (el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 32,
        ),
      )
      .toBe(true);
    await captureEvidence(page, "scroll-jump-latest");
  });
});
