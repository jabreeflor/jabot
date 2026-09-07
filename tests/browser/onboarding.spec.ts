import { ONBOARDING_KEY } from "../../src/onboarding/state";
import { test, expect } from "./fixtures";
import {
  captureEvidence,
  completeOnboarding,
  openFirstRun,
  settingsButton,
  skipOnboarding,
  waitForConnected,
} from "./ui";

test.describe("first launch", () => {
  test("completes onboarding through the controls and keeps the profile after reload", async ({
    page,
    jabot,
  }) => {
    await openFirstRun(page, jabot.baseURL);
    await completeOnboarding(page, "Ada Lovelace");
    await waitForConnected(page);

    await expect(page.getByText("Ada Lovelace")).toBeVisible();
    await expect(page.getByRole("button", { name: /^Chief\b/ })).toBeVisible();
    await captureEvidence(page, "onboarding-complete");

    const stored = await page.evaluate((key) => window.localStorage.getItem(key), ONBOARDING_KEY);
    expect(stored).toBeTruthy();
    expect(JSON.parse(stored as string)).toMatchObject({
      userName: "Ada Lovelace",
      skipped: false,
    });

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await expect(page.getByText("Ada Lovelace")).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /What should the crew call you/ }),
    ).toHaveCount(0);
  });

  test("skip persists a profile and does not trap the next launch", async ({ page, jabot }) => {
    await openFirstRun(page, jabot.baseURL);
    await page.getByLabel("YOUR NAME").fill("Ada");
    await skipOnboarding(page);
    await waitForConnected(page);

    await expect(page.getByText("Ada", { exact: true })).toBeVisible();
    await captureEvidence(page, "onboarding-skip");

    const stored = await page.evaluate((key) => window.localStorage.getItem(key), ONBOARDING_KEY);
    expect(JSON.parse(stored as string)).toMatchObject({
      userName: "Ada",
      skipped: true,
    });

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await expect(page.getByText("Ada", { exact: true })).toBeVisible();
  });

  test("Settings rerun keeps the saved profile if the flow is aborted", async ({
    page,
    jabot,
  }) => {
    await openFirstRun(page, jabot.baseURL);
    await completeOnboarding(page, "Ada Lovelace");
    await waitForConnected(page);

    await settingsButton(page).click();
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await captureEvidence(page, "settings-rerun-entry");
    await page.getByRole("button", { name: "Run setup again" }).click();

    await expect(
      page.getByRole("heading", { name: /What should the crew call you/ }),
    ).toBeVisible();
    await expect(page.getByLabel("YOUR NAME")).toHaveValue("Ada Lovelace");
    const midRun = await page.evaluate((key) => window.localStorage.getItem(key), ONBOARDING_KEY);
    expect(midRun).toBeTruthy();

    await page.keyboard.press("Escape");
    await waitForConnected(page);
    await expect(page.getByText("Ada Lovelace")).toBeVisible();

    const stored = await page.evaluate((key) => window.localStorage.getItem(key), ONBOARDING_KEY);
    expect(JSON.parse(stored as string)).toMatchObject({ userName: "Ada Lovelace" });
  });
});
