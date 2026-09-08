import { fakeAcpAgentPath, fakeAcpRuntime } from "../support/hostd";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  captureEvidence,
  expectSettledAgent,
  openConnectedApp,
  openThread,
  sendComposer,
  agentBubble,
} from "./ui";

test.describe("failure and cancel", () => {
  test("missing runtime shows an actionable error and settles the composer", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-missing",
      title: "Missing runtime",
      runtime: {
        command: "/nonexistent/jabot-missing-agent",
        args: [],
      },
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Missing runtime");

    const box = page.getByRole("textbox", { name: "Message Missing runtime" });
    await expect(box).toBeEnabled();
    await box.fill("please fail");
    await box.press("Enter");

    const alert = page.getByRole("alert");
    await expect(alert).toBeVisible();
    await expect(alert).toContainText(
      /unavailable|not found|missing|Harness|ENOENT|spawn/i,
    );
    await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
    await captureEvidence(page, "failure-missing-runtime");
  });

  test("empty reply surfaces a failed turn", async ({ page, jabot }) => {
    await seedCodeThread(jabot, {
      threadId: "t-empty",
      title: "Empty reply",
      runtime: { command: fakeAcpAgentPath(), args: ["empty-reply"] },
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Empty reply");

    await sendComposer(page, "say nothing", "Empty reply");
    await expect(
      page.getByRole("status").filter({ hasText: /without a reply/ }),
    ).toBeVisible();
    await expect(
      page.locator(".status", { hasText: /failed: no reply/ }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
    await captureEvidence(page, "failure-empty-reply");
  });

  test("a hanging turn can be cancelled and pending controls settle", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-hang",
      title: "Hanging turn",
      runtime: fakeAcpRuntime("cancellable"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Hanging turn");

    await sendComposer(page, "hold on", "Hanging turn");
    await expect(
      agentBubble(page).filter({ hasText: "hello from fake-acp" }),
    ).toBeVisible();
    const stop = page.getByRole("button", { name: "Stop" });
    await expect(stop).toBeVisible();
    await captureEvidence(page, "failure-hanging");

    await stop.click();
    await expect(
      page.getByRole("status").filter({ hasText: /Cancelled/ }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
    await expectSettledAgent(page, "hello from fake-acp");
    await captureEvidence(page, "failure-cancelled");
  });
});
