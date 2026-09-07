import path from "node:path";

import { pumpAcpRuntime, openGate } from "../support/hostd";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  captureEvidence,
  expectSettledAgent,
  expectStreamingAgent,
  openChief,
  openConnectedApp,
  openThread,
  sendComposer,
  userBubble,
  agentBubble,
  waitForConnected,
} from "./ui";

test.describe("conversation streaming", () => {
  test("shows user text, incremental agent text, and a settled reply", async ({
    page,
    jabot,
  }) => {
    const gate = path.join(jabot.dataDir, "conversation.gate");
    await seedCodeThread(jabot, {
      threadId: "t-stream",
      title: "Streaming turn",
      runtime: pumpAcpRuntime(gate),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Streaming turn");

    await sendComposer(page, "stream please", "Streaming turn");
    await expect(userBubble(page).filter({ hasText: "stream please" })).toBeVisible();
    await expect(agentBubble(page).filter({ hasText: "hello from fake-acp" })).toBeVisible();

    openGate(gate, "chunk:partial-");
    await expectStreamingAgent(page, /hello from fake-acppartial-/);
    await captureEvidence(page, "conversation-streaming");

    openGate(gate, "chunk:reply");
    await expect(
      agentBubble(page).filter({ hasText: "hello from fake-acppartial-reply" }),
    ).toBeVisible();

    openGate(gate, "end_turn");
    await expectSettledAgent(page, "hello from fake-acppartial-reply");
    await captureEvidence(page, "conversation-settled");
  });

  test("rapid submit does not duplicate the user turn", async ({ page, jabot }) => {
    await openConnectedApp(page, jabot.baseURL);
    await openChief(page);

    const box = page.getByRole("textbox", { name: "Message Chief" });
    await expect(box).toBeEnabled();
    await box.fill("once only");
    await Promise.all([box.press("Enter"), box.press("Enter")]);

    await expect(userBubble(page).filter({ hasText: "once only" })).toHaveCount(1);
    await expectSettledAgent(page, "hello from fake-acp");
    await expect(agentBubble(page).filter({ hasText: "hello from fake-acp" })).toHaveCount(1);
  });

  test("reload keeps the durable transcript", async ({ page, jabot }) => {
    await openConnectedApp(page, jabot.baseURL);
    await openChief(page);
    await sendComposer(page, "persist this");
    await expectSettledAgent(page, "hello from fake-acp");

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await openChief(page);
    await expect(userBubble(page).filter({ hasText: "persist this" })).toBeVisible();
    await expectSettledAgent(page, "hello from fake-acp");

    const stored = JSON.stringify(await jabot.rpc("thread/transcript", {
      threadId: (await jabot.rpc<{ threadId: string }>("crew/thread", { botId: "chief" }))
        .threadId,
    }));
    expect(stored).toContain("persist this");
    expect(stored).toContain("hello from fake-acp");
  });
});
