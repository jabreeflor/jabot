//! Open a renderer against an isolated host and wait until it is connected.

import {
  expect,
  type Browser,
  type BrowserContext,
  type Locator,
  type Page,
} from "@playwright/test";

import {
  type CaptureTheme,
  type WindowSize,
  ONBOARDED_PROFILE,
  ONBOARDING_KEY,
  PINNED_LOCALE,
  PINNED_TIMEZONE,
  SCREENSHOT_STYLE,
  THEME_KEY,
  VIEWPORTS,
} from "./constants";

export interface HostLocation {
  url?: string;
  baseURL?: string;
}

export interface OpenOptions {
  theme?: CaptureTheme;
  windowSize?: WindowSize;
  firstRun?: boolean;
}

export interface OpenedPage {
  page: Page;
  context: BrowserContext;
  close(): Promise<void>;
}

export async function openApp(
  browser: Browser,
  app: HostLocation,
  options: OpenOptions = {},
): Promise<OpenedPage> {
  const theme = options.theme ?? "dark";
  const windowSize = options.windowSize ?? "desktop";
  const firstRun = options.firstRun ?? false;

  const context = await browser.newContext({
    viewport: VIEWPORTS[windowSize],
    deviceScaleFactor: 1,
    colorScheme: theme,
    locale: PINNED_LOCALE,
    timezoneId: PINNED_TIMEZONE,
    reducedMotion: "reduce",
    serviceWorkers: "block",
  });

  if (!firstRun) {
    await context.addInitScript(
      ([key, profile]) => {
        window.localStorage.setItem(key, JSON.stringify(profile));
      },
      [ONBOARDING_KEY, ONBOARDED_PROFILE] as const,
    );
  }
  await context.addInitScript(
    ([key, value]) => {
      window.localStorage.setItem(key, value);
    },
    [THEME_KEY, theme] as const,
  );

  const page = await context.newPage();
  page.setDefaultTimeout(20_000);
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => {
    pageErrors.push(error.message);
  });

  const origin = app.url ?? app.baseURL;
  if (!origin) {
    throw new Error("openApp needs app.url or app.baseURL");
  }
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await page.addStyleTag({ content: SCREENSHOT_STYLE });
  await page.evaluate(() => document.fonts.ready);

  if (firstRun) {
    await expect(
      page.getByRole("heading", { name: /What should the crew call you/ }),
    ).toBeVisible();
  } else {
    await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
    await expect(page.getByText("Connecting to host…")).toHaveCount(0);
    await expect(page.locator(".host.bad")).toHaveCount(0);
  }

  if (pageErrors.length > 0) {
    await context.close();
    throw new Error(`unexpected page errors:\n${pageErrors.join("\n")}`);
  }

  return {
    page,
    context,
    close: async () => {
      if (pageErrors.length > 0) {
        throw new Error(`unexpected page errors:\n${pageErrors.join("\n")}`);
      }
      await context.close();
    },
  };
}

export async function settle(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.addStyleTag({ content: SCREENSHOT_STYLE });
  // Two frames after fonts and reduced-motion styles, so layout is stable.
  await page.waitForTimeout(50);
}

export function variableMasks(page: Page): Locator[] {
  return [page.locator(".when"), page.locator(".sched-next")];
}
