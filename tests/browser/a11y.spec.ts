/**
 * Browser axe with contrast enabled (#234).
 *
 * jsdom unit tests keep contrast off. This suite paints. Critical and serious
 * findings fail; every violation is attached to the report. Exceptions live
 * in `support/a11y.ts` and must be named one rule at a time.
 */
import { expect } from "@playwright/test";

import { test } from "./fixtures";
import {
  expectNoSeriousBrowserA11yViolations,
  scanA11y,
} from "./support/a11y";
import { openApp } from "./support/page";
import {
  putChiefOnFakeAcp,
  seedPermissionAsk,
  seedSchedule,
} from "./support/seed";

test.describe("browser axe @a11y @smoke", () => {
  test("fails on a nameless button, so contrast-on axe is not a no-op", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.evaluate(() => {
        const button = document.createElement("button");
        button.type = "button";
        button.style.color = "#ccc";
        button.style.background = "#ddd";
        button.textContent = "";
        document.body.appendChild(button);
      });
      const results = await scanA11y(opened.page);
      const ids = results.violations.map((violation) => violation.id);
      expect(ids.join(" ")).toMatch(/button-name|color-contrast/);
    } finally {
      await opened.close();
    }
  });

  test("onboarding", async ({ app, browser }) => {
    const opened = await openApp(browser, app, { firstRun: true });
    try {
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "onboarding",
      );
    } finally {
      await opened.close();
    }
  });

  test("sidebar and conversation", async ({ app, browser }) => {
    await putChiefOnFakeAcp(app.url);
    const opened = await openApp(browser, app);
    try {
      await expect(opened.page.getByLabel("Message Chief")).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "sidebar-stream",
      );
    } finally {
      await opened.close();
    }
  });

  test("inbox permission card", async ({ app, browser }) => {
    await seedPermissionAsk(app.url);
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: /^Inbox/ }).click();
      await expect(opened.page.getByText("PERMISSION")).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "inbox-permission",
      );
    } finally {
      await opened.close();
    }
  });

  test("new chat and open harness listbox", async ({ app, browser }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: "New Chat" }).click();
      await expect(
        opened.page.getByRole("region", { name: "New Chat" }),
      ).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "new-chat",
      );
      await opened.page.getByRole("button", { name: /^Harness:/ }).click();
      await expect(opened.page.getByRole("listbox")).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "new-chat-harness",
      );
    } finally {
      await opened.close();
    }
  });

  test("settings", async ({ app, browser }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: "Settings" }).click();
      await expect(
        opened.page.getByRole("heading", { name: "Settings" }),
      ).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "settings",
      );
    } finally {
      await opened.close();
    }
  });

  test("schedules", async ({ app, browser }) => {
    await seedSchedule(app.url);
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: "Schedules" }).click();
      await expect(opened.page.getByText("Morning triage")).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "schedules",
      );
    } finally {
      await opened.close();
    }
  });

  test("pr board, sign-in dialog, and error banner", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: /^Pull Requests/ }).click();
      await expect(
        opened.page.getByRole("heading", { name: "Pull Requests" }),
      ).toBeVisible();
      await expectNoSeriousBrowserA11yViolations(
        opened.page,
        test.info(),
        "pr-board",
      );

      const signIn = opened.page.getByRole("button", {
        name: "Sign in with GitHub",
      });
      if (await signIn.isVisible()) {
        await signIn.click();
        const dialog = opened.page.getByRole("dialog");
        await expect(dialog).toBeVisible();
        await expectNoSeriousBrowserA11yViolations(
          opened.page,
          test.info(),
          "pr-signin",
        );

        const token = opened.page.getByLabel(/PASTE IT HERE/);
        if (await token.count()) {
          await token.fill("ghp_not-a-real-token");
          await opened.page.getByRole("button", { name: "Sign in" }).click();
          await expect(opened.page.getByRole("alert")).toBeVisible({
            timeout: 15_000,
          });
          await expectNoSeriousBrowserA11yViolations(
            opened.page,
            test.info(),
            "pr-signin-error",
          );
        }
      }
    } finally {
      await opened.close();
    }
  });
});
