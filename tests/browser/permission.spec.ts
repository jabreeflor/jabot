import { fakeAcpRuntime } from "../support/hostd";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  allowButton,
  captureEvidence,
  denyButton,
  expectSettledAgent,
  openConnectedApp,
  openThread,
  sendComposer,
  waitForConnected,
} from "./ui";

async function waitForAdapterLog(
  jabot: { adapterLog: (threadId: string) => string },
  threadId: string,
  needle: string | RegExp,
  timeoutMs = 15_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < deadline) {
    last = jabot.adapterLog(threadId);
    if (
      typeof needle === "string" ? last.includes(needle) : needle.test(last)
    ) {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(
    `adapter log for ${threadId} never matched ${needle}; last:\n${last}`,
  );
}

test.describe("permission lifecycle", () => {
  test("approve sends the chosen option once and clears the pending ask", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-perm-allow",
      title: "Permission approve",
      runtime: fakeAcpRuntime("permission"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Permission approve");

    await sendComposer(page, "rm -rf", "Permission approve");
    const allow = allowButton(page);
    await expect(allow).toBeVisible();
    await expect(denyButton(page)).toBeVisible();
    await captureEvidence(page, "permission-ask");

    const pendingBefore = await jabot.rpc<{ requests: unknown[] }>(
      "permission/pending",
      {
        threadId: "t-perm-allow",
      },
    );
    expect(pendingBefore.requests).toHaveLength(1);

    await allow.click();
    await expect(allow).toBeDisabled();
    await expectSettledAgent(page, "allowed");

    await allow.click({ force: true }).catch(() => undefined);
    await expect(allowButton(page)).toBeDisabled();

    const log = await waitForAdapterLog(
      jabot,
      "t-perm-allow",
      "permission_reply=",
    );
    const replies = log
      .split("\n")
      .filter((line) => line.startsWith("permission_reply="));
    expect(replies).toHaveLength(1);
    expect(replies[0]).toMatch(/allow_once/);

    await expect
      .poll(async () => {
        const pending = await jabot.rpc<{ requests: unknown[] }>(
          "permission/pending",
          {
            threadId: "t-perm-allow",
          },
        );
        return pending.requests.length;
      })
      .toBe(0);
    await captureEvidence(page, "permission-approved");
  });

  test("reject sends deny and clears the pending ask", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-perm-deny",
      title: "Permission reject",
      runtime: fakeAcpRuntime("permission"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Permission reject");

    await sendComposer(page, "rm -rf", "Permission reject");
    const deny = denyButton(page);
    await expect(deny).toBeVisible();
    await deny.click();
    await expect(deny).toBeDisabled();

    const log = await waitForAdapterLog(
      jabot,
      "t-perm-deny",
      "permission_reply=",
    );
    expect(log).toMatch(/reject_once/);
    await expectSettledAgent(page, "allowed");

    await expect
      .poll(async () => {
        const pending = await jabot.rpc<{ requests: unknown[] }>(
          "permission/pending",
          {
            threadId: "t-perm-deny",
          },
        );
        return pending.requests.length;
      })
      .toBe(0);
    await captureEvidence(page, "permission-rejected");
  });

  test("an outstanding ask survives reload and can still be answered", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-perm-reload",
      title: "Permission reload",
      runtime: fakeAcpRuntime("permission"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Permission reload");
    await sendComposer(page, "rm -rf", "Permission reload");
    await expect(allowButton(page)).toBeVisible();

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await openThread(page, "Permission reload");
    await expect(allowButton(page)).toBeVisible();
    await captureEvidence(page, "permission-reload");

    await allowButton(page).click();
    await expectSettledAgent(page, "allowed");
    const log = await waitForAdapterLog(
      jabot,
      "t-perm-reload",
      "permission_reply=",
    );
    expect(log).toMatch(/allow_once/);
  });
});
