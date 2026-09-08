import { expect, test } from "./fixtures";
import { captureEvidence, openConnectedApp, waitForConnected } from "./ui";

test.describe("schedules", () => {
  test("creates, pauses, edits, and Run now through the UI; persists after browser reload", async ({
    page,
    jabot,
  }) => {
    await openConnectedApp(page, jabot.baseURL);
    await page.getByRole("button", { name: /^Schedules/ }).click();
    await expect(
      page.getByRole("heading", { name: "Schedules", exact: true }),
    ).toBeVisible();

    const prompt = page.getByLabel("What should it do?");
    await prompt.fill("summarise overnight mail every weekday at 9am");
    await page.getByLabel("Runs as").selectOption({ label: "Chief" });
    await page.getByRole("button", { name: "Create schedule" }).click();

    const row = page.locator(".sched-row-main");
    await expect(row).toContainText("Summarise overnight mail");

    const listed = await jabot.rpc<{
      schedules: Array<{
        scheduleId: string;
        name: string;
        enabled: boolean;
        lastFire?: { state: string };
        nextRunAt?: string;
      }>;
    }>("schedule/list");
    expect(listed.schedules).toHaveLength(1);
    const created = listed.schedules[0];
    expect(created.enabled).toBe(true);
    const nextRunAt = created.nextRunAt;

    const toggle = page.locator(".sched-switch .track");
    await toggle.click();
    await expect(row.locator(".sched-next")).toHaveText("Paused");
    const paused = await jabot.rpc<{
      schedules: Array<{ enabled: boolean }>;
    }>("schedule/list");
    expect(paused.schedules[0].enabled).toBe(false);

    await toggle.click();
    await row.click();
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    const nameField = page.getByLabel("NAME");
    await expect(nameField).toBeVisible();
    await nameField.fill("Morning brief");
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(row).toContainText("Morning brief");
    if ((await row.getAttribute("aria-expanded")) !== "true") {
      await row.click();
    }
    await page.getByRole("button", { name: "Run now" }).click();
    await expect(page.getByText(/Ran |Running since|Caught up/)).toBeVisible({
      timeout: 20_000,
    });
    await captureEvidence(page, "schedules-run-now");

    const afterRun = await jabot.rpc<{
      schedules: Array<{
        lastFire?: { state: string; threadId?: string };
        nextRunAt?: string;
      }>;
    }>("schedule/list");
    expect(afterRun.schedules[0].lastFire).toBeTruthy();
    expect(afterRun.schedules[0].nextRunAt).toBe(nextRunAt);

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await page.getByRole("button", { name: /^Schedules/ }).click();
    const persisted = page.locator(".sched-row-main");
    await expect(persisted).toContainText("Morning brief");
    await persisted.click();
    await expect(page.getByText(/Ran |Caught up|Running since/)).toBeVisible();
  });
});
