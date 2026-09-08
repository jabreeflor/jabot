/**
 * Visual regression for stable product states (#234).
 *
 * Baselines are environment-specific (OS + Playwright project). CI never
 * updates them — see docs/browser-tests.md. Masks cover only relative
 * clocks; content and layout stay in the picture.
 */
import { expect, type Locator, type Page } from "@playwright/test";

import { browserTest, test } from "./fixtures";
import { createFakeGh } from "./helpers/gh";
import {
  type CaptureTheme,
  type WindowSize,
  FAKE_ACP_REPLY,
} from "./support/constants";
import { openApp, settle, variableMasks } from "./support/page";
import {
  putChiefOnFakeAcp,
  seedChiefConversation,
  seedPermissionAsk,
  seedSchedule,
} from "./support/seed";
import { agentBubble } from "./ui";

/** Logged-out fixture `gh` so the board does not inherit a developer login. */
const prVisual = browserTest(() => ({
  pathPrefix: [createFakeGh().dir],
  extraEnv: { GH_TOKEN: "", GITHUB_TOKEN: "" },
}));

const THEMES: readonly CaptureTheme[] = ["dark", "light"];

async function shot(
  page: Page,
  name: string,
  extraMask: Locator[] = [],
): Promise<void> {
  await settle(page);
  await expect(page).toHaveScreenshot(`${name}.png`, {
    mask: [...variableMasks(page), ...extraMask],
    maxDiffPixels: 0,
    animations: "disabled",
    caret: "hide",
  });
}

test.describe("stable states @visual", () => {
  test("onboarding", async ({ app, browser }) => {
    for (const theme of THEMES) {
      const sizes: WindowSize[] = ["desktop", "minimum"];
      for (const windowSize of sizes) {
        const opened = await openApp(browser, app, {
          theme,
          windowSize,
          firstRun: true,
        });
        try {
          await expect(
            opened.page.getByRole("heading", {
              name: /What should the crew call you/,
            }),
          ).toBeVisible();
          await shot(opened.page, `onboarding-${theme}-${windowSize}`);
        } finally {
          await opened.close();
        }
      }
    }
  });

  test("sidebar and streamed conversation", async ({ app, browser }) => {
    await seedChiefConversation(app.url);
    for (const theme of THEMES) {
      const sizes: WindowSize[] = ["desktop", "minimum"];
      for (const windowSize of sizes) {
        const opened = await openApp(browser, app, { theme, windowSize });
        try {
          const page = opened.page;
          await page.getByRole("button", { name: /^Chief/ }).click();
          await expect(
            agentBubble(page).filter({ hasText: FAKE_ACP_REPLY }),
          ).toBeVisible();
          await expect(
            page.getByText("hello from the visual suite", { exact: true }),
          ).toBeVisible();
          await shot(page, `sidebar-stream-${theme}-${windowSize}`);
        } finally {
          await opened.close();
        }
      }
    }
  });

  test("inbox permission card @chromium-only", async ({ app, browser }) => {
    await seedPermissionAsk(app.url);
    for (const theme of THEMES) {
      const opened = await openApp(browser, app, {
        theme,
        windowSize: "desktop",
      });
      try {
        await opened.page.getByRole("button", { name: /^Inbox/ }).click();
        await expect(
          opened.page.getByRole("heading", { name: "Inbox" }),
        ).toBeVisible();
        await expect(opened.page.getByText("PERMISSION")).toBeVisible();
        const allow = opened.page.getByRole("button", { name: "Allow" });
        if (!(await allow.isVisible())) {
          await opened.page.getByRole("button", { name: /Run ls/ }).click();
        }
        await expect(allow).toBeVisible();
        await shot(opened.page, `inbox-permission-${theme}-desktop`);
      } finally {
        await opened.close();
      }
    }
  });

  test("new chat @chromium-only", async ({ app, browser }) => {
    await putChiefOnFakeAcp(app.url);
    for (const theme of THEMES) {
      const opened = await openApp(browser, app, {
        theme,
        windowSize: "desktop",
      });
      try {
        await opened.page.getByRole("button", { name: "New Chat" }).click();
        await expect(
          opened.page.getByRole("region", { name: "New Chat" }),
        ).toBeVisible();
        await shot(opened.page, `new-chat-${theme}-desktop`);
      } finally {
        await opened.close();
      }
    }
  });

  test("settings", async ({ app, browser }) => {
    for (const theme of THEMES) {
      const sizes: WindowSize[] = ["desktop", "minimum"];
      for (const windowSize of sizes) {
        const opened = await openApp(browser, app, { theme, windowSize });
        try {
          await opened.page.getByRole("button", { name: "Settings" }).click();
          await expect(
            opened.page.getByRole("heading", { name: "Settings" }),
          ).toBeVisible();
          await expect(
            opened.page.getByRole("heading", { name: "Appearance" }),
          ).toBeVisible();
          await shot(opened.page, `settings-${theme}-${windowSize}`);
        } finally {
          await opened.close();
        }
      }
    }
  });

  test("schedules @chromium-only", async ({ app, browser }) => {
    await seedSchedule(app.url);
    for (const theme of THEMES) {
      const opened = await openApp(browser, app, {
        theme,
        windowSize: "desktop",
      });
      try {
        await opened.page.getByRole("button", { name: "Schedules" }).click();
        await expect(
          opened.page.getByRole("heading", { name: "Schedules" }),
        ).toBeVisible();
        await expect(opened.page.getByText("Morning triage")).toBeVisible();
        await opened.page
          .getByRole("button", { name: /Morning triage/ })
          .click();
        await expect(
          opened.page.getByRole("button", { name: "Run now" }),
        ).toBeVisible();
        await shot(opened.page, `schedules-${theme}-desktop`);
      } finally {
        await opened.close();
      }
    }
  });

  prVisual("pr workspace @chromium-only", async ({ app, browser }) => {
    for (const theme of THEMES) {
      const opened = await openApp(browser, app, {
        theme,
        windowSize: "desktop",
      });
      try {
        await opened.page
          .getByRole("button", { name: /^Pull Requests/ })
          .click();
        await expect(
          opened.page.getByRole("heading", { name: "Pull Requests" }),
        ).toBeVisible();
        const signIn = opened.page.getByRole("button", {
          name: "Sign in with GitHub",
        });
        await expect(signIn).toBeVisible();
        await shot(opened.page, `pr-board-${theme}-desktop`);
        await signIn.click();
        await expect(opened.page.getByRole("dialog")).toBeVisible();
        await shot(opened.page, `pr-signin-${theme}-desktop`);
      } finally {
        await opened.close();
      }
    }
  });
});
