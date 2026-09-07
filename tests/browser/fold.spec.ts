import path from "node:path";

import { gatedAcpRuntime, openGate } from "../support/hostd";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  captureEvidence,
  expectSettledAgent,
  openConnectedApp,
  openThread,
  sendComposer,
  threadRow,
  userBubble,
  agentBubble,
} from "./ui";

test.describe("fold → Inbox → reopen", () => {
  test("folds a live turn, surfaces one Inbox card, and recovers the transcript", async ({
    page,
    jabot,
  }) => {
    const gate = path.join(jabot.dataDir, "fold.gate");
    await seedCodeThread(jabot, {
      threadId: "t-fold",
      title: "Auth migration",
      folderName: "globnet-sync",
      runtime: gatedAcpRuntime(gate),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Auth migration");

    await sendComposer(page, "migrate the auth middleware", "Auth migration");
    await expect(userBubble(page).filter({ hasText: "migrate the auth middleware" })).toBeVisible();
    await expect(agentBubble(page).filter({ hasText: "hello from fake-acp" })).toBeVisible();

    await expect
      .poll(async () => {
        const state = await jabot.rpc<{
          latestRun?: { state: string };
          process: { acpState: string };
        }>("thread/state", { threadId: "t-fold" });
        return state.latestRun?.state === "running" && state.process.acpState === "running";
      })
      .toBe(true);

    await page.getByRole("button", { name: "Fold" }).click();
    await page.getByRole("menuitem", { name: /Disappear until done/ }).click();

    await expect(threadRow(page, "Auth migration")).toHaveCount(0);
    await captureEvidence(page, "fold-hidden");

    const folded = await jabot.rpc<{ state: string }>("thread/state", { threadId: "t-fold" });
    expect(folded.state).toBe("folded");

    openGate(gate, "end_turn");

    await expect(page.getByRole("button", { name: /Inbox — 1 waiting/ })).toBeVisible();
    await page.getByRole("button", { name: /Inbox — 1 waiting/ }).click();
    await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
    await expect(page.getByText(/Auth migration finished/)).toBeVisible();
    await captureEvidence(page, "fold-inbox-card");

    const inbox = await jabot.rpc<{
      events: Array<{ kind: string; threadId: string }>;
      unread: number;
    }>("inbox/list");
    expect(inbox.events).toHaveLength(1);
    expect(inbox.events[0]).toMatchObject({ kind: "done", threadId: "t-fold" });
    expect(inbox.unread).toBe(1);

    await page.getByText(/Auth migration finished/).click();
    const open = page.getByRole("button", { name: "Open thread" });
    if (await open.isVisible()) await open.click();

    await expect(page.getByRole("heading", { name: "Auth migration" })).toBeVisible();
    await expect(userBubble(page).filter({ hasText: "migrate the auth middleware" })).toBeVisible();
    await expectSettledAgent(page, "hello from fake-acp");
    await captureEvidence(page, "fold-reopened");
  });
});
