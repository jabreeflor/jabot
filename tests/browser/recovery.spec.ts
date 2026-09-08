/**
 * Disconnect / recovery. Browser reload, Vite restart, and host restart are
 * three different mechanisms — each test name says which one it is.
 */
import { existsSync } from "node:fs";
import path from "node:path";

import { expect, test } from "./fixtures";
import {
  agentBubble,
  captureEvidence,
  composer,
  openConnectedApp,
  sendComposer,
  userBubble,
  waitForConnected,
} from "./ui";

const USER_LINE = "remember the recovery token alpha-233";

test.describe("host recovery", () => {
  test("host restart: shows disconnected then Reconnect replays transcript without duplicates", async ({
    page,
    jabot,
  }) => {
    await openConnectedApp(page, jabot.baseURL);
    await sendComposer(page, USER_LINE);
    await expect(userBubble(page)).toHaveCount(1);
    await expect(userBubble(page)).toContainText(USER_LINE);
    await expect(agentBubble(page)).toHaveCount(1);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");

    const beforeInbox = await jabot.rpc<{ events: unknown[] }>("inbox/list", {
      limit: 50,
    });

    await jabot.stopHost();
    await expect(page.getByText("Host disconnected")).toBeVisible();
    await expect(page.getByRole("button", { name: "Reconnect" })).toBeVisible();
    await expect(page.locator(".host.bad")).toBeVisible();
    // No false success: the host line is an error, not a healthy empty subtitle.
    await expect(page.getByText("Connecting to host…")).toHaveCount(0);
    await captureEvidence(page, "host-disconnected");

    await page.getByRole("button", { name: "Reconnect" }).click();
    await waitForConnected(page);
    await expect(page.getByText("Host disconnected")).toHaveCount(0);

    await expect(userBubble(page)).toHaveCount(1);
    await expect(userBubble(page)).toContainText(USER_LINE);
    await expect(agentBubble(page)).toHaveCount(1);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");

    const afterInbox = await jabot.rpc<{ events: unknown[] }>("inbox/list", {
      limit: 50,
    });
    expect(afterInbox.events.length).toBe(beforeInbox.events.length);
    expect(existsSync(path.join(jabot.dataDir, "jabot.sqlite"))).toBe(true);
  });

  test("browser reload: replays persisted Chief transcript without duplicates", async ({
    page,
    jabot,
  }) => {
    await openConnectedApp(page, jabot.baseURL);
    await sendComposer(page, USER_LINE);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);

    await expect(composer(page)).toBeVisible();
    await expect(userBubble(page)).toHaveCount(1);
    await expect(userBubble(page)).toContainText(USER_LINE);
    await expect(agentBubble(page)).toHaveCount(1);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");
  });

  test("Vite restart: same data directory restores transcript after a new renderer", async ({
    page,
    jabot,
  }) => {
    await openConnectedApp(page, jabot.baseURL);
    await sendComposer(page, USER_LINE);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");

    await jabot.restart();
    await openConnectedApp(page, jabot.baseURL);

    await expect(userBubble(page)).toHaveCount(1);
    await expect(userBubble(page)).toContainText(USER_LINE);
    await expect(agentBubble(page)).toHaveCount(1);
    await expect(agentBubble(page)).toContainText("hello from fake-acp");
  });
});
