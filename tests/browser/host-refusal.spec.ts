import { expect, test } from "./fixtures";
import { captureEvidence, openConnectedApp } from "./ui";

test.describe("host refusal", () => {
  test("invalid cron keeps the draft, leaves the list empty, and shows an actionable alert", async ({
    page,
    jabot,
  }) => {
    await openConnectedApp(page, jabot.baseURL);
    await page.getByRole("button", { name: /^Schedules/ }).click();

    await page.getByLabel("What should it do?").fill("never fire this job");
    await page.getByLabel("When").selectOption("Custom…");
    const cron = page.getByLabel("CRON");
    await expect(cron).toBeVisible();
    await cron.fill("0 99 * * *");
    await page.getByRole("button", { name: "Create schedule" }).click();

    const alert = page.getByRole("alert");
    await expect(alert).toBeVisible();
    await expect(alert).toContainText(/hour/i);
    await expect(page.getByLabel("What should it do?")).toHaveValue("never fire this job");
    await expect(page.getByLabel("CRON")).toHaveValue("0 99 * * *");
    await expect(page.getByRole("button", { name: /never fire/i })).toHaveCount(0);
    await captureEvidence(page, "host-refusal");

    const listed = await jabot.rpc<{ schedules: unknown[] }>("schedule/list");
    expect(listed.schedules).toEqual([]);
  });
});
