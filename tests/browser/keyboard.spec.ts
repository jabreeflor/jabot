/**
 * Keyboard, focus, Escape, and accessible names (#234).
 *
 * Axe does not prove a modal can be used from the keyboard. These tests do.
 */
import { expect } from "@playwright/test";

import { test } from "./fixtures";
import { openApp } from "./support/page";

test.describe("keyboard and focus @keyboard @smoke", () => {
  test("onboarding focuses the name field and Escape skips", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app, { firstRun: true });
    try {
      const name = opened.page.getByLabel(/YOUR NAME/i);
      await expect(name).toBeFocused();
      await expect(name).toHaveAccessibleName(/YOUR NAME/i);
      await opened.page.keyboard.press("Escape");
      await expect(
        opened.page.getByRole("button", { name: "Settings" }),
      ).toBeVisible();
    } finally {
      await opened.close();
    }
  });

  test("tab order reaches New Chat, Inbox, and Settings", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.locator("body").click();
      const seen = new Set<string>();
      for (let i = 0; i < 24; i++) {
        await opened.page.keyboard.press("Tab");
        const name = await opened.page.evaluate(() => {
          const el = document.activeElement;
          if (!(el instanceof HTMLElement)) return "";
          return (
            el.getAttribute("aria-label") ||
            el.textContent?.trim().split("\n")[0] ||
            el.tagName
          );
        });
        if (name) seen.add(name);
      }
      expect([...seen].join(" | ")).toMatch(/New Chat/);
      expect([...seen].some((name) => /Inbox/.test(name))).toBeTruthy();
      expect([...seen].some((name) => /Settings/.test(name))).toBeTruthy();

      const settings = opened.page.getByRole("button", { name: "Settings" });
      await settings.focus();
      await opened.page.keyboard.press("Shift+Tab");
      await opened.page.keyboard.press("Tab");
      await expect(settings).toBeFocused();
      const focusLook = await settings.evaluate((el) => {
        const visible = el.matches(":focus-visible");
        const style = getComputedStyle(el);
        return {
          visible,
          outline: `${style.outlineStyle} ${style.outlineWidth}`,
          boxShadow: style.boxShadow,
        };
      });
      expect(focusLook.visible).toBeTruthy();
      expect(
        focusLook.outline !== "none 0px" || focusLook.boxShadow !== "none",
      ).toBeTruthy();
    } finally {
      await opened.close();
    }
  });

  test("bot editor traps focus, Escape restores it", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: /^Crew/ }).click();
      const opener = opened.page.getByRole("button", { name: "Edit" }).first();
      await expect(opener).toBeVisible();
      await opener.focus();
      await opener.click();

      const dialog = opened.page.getByRole("dialog");
      await expect(dialog).toBeVisible();
      await expect(dialog).toHaveAccessibleName(/Customize /);

      await expect
        .poll(async () =>
          dialog.evaluate((root) => root.contains(document.activeElement)),
        )
        .toBeTruthy();

      await opened.page.keyboard.press("Tab");
      const focusedInside = await dialog.evaluate((root) => {
        const active = document.activeElement;
        return Boolean(active && root.contains(active));
      });
      expect(focusedInside).toBeTruthy();

      await opened.page.keyboard.press("Escape");
      await expect(dialog).toHaveCount(0);
      await expect(opener).toBeFocused();
    } finally {
      await opened.close();
    }
  });

  test("New Chat harness listbox closes on Escape and restores the trigger", async ({
    app,
    browser,
  }) => {
    const opened = await openApp(browser, app);
    try {
      await opened.page.getByRole("button", { name: "New Chat" }).click();
      const trigger = opened.page.getByRole("button", { name: /^Harness:/ });
      await trigger.click();
      const listbox = opened.page.getByRole("listbox");
      await expect(listbox).toBeVisible();
      await expect(listbox).toBeFocused();
      await opened.page.keyboard.press("Escape");
      await expect(listbox).toHaveCount(0);
      await expect(trigger).toBeFocused();
    } finally {
      await opened.close();
    }
  });
});
