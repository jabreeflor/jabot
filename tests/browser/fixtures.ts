/**
 * Playwright fixtures for a real-host browser test.
 *
 * `jabot` owns Vite, jabot-hostd, a temp data directory, and a dedicated
 * port. Sister issues (#232–#234) should import `{ test, expect }` from here
 * rather than standing up their own server. `scripts/live.sh` is not used.
 *
 * Fixture options:
 * - `seedChief` — RPC `crew/update` onto `fake-acp` before the page loads
 *   (prerequisite; the click/send under test still goes through the UI).
 */
import { test as base, expect } from "@playwright/test";

import {
  attachHostLogs,
  seedChiefOnFakeAcp,
  startJabotApp,
  type JabotApp,
} from "./host";

export type BrowserFixtures = {
  jabot: JabotApp;
  seedChief: boolean;
};

export const test = base.extend<BrowserFixtures>({
  seedChief: [true, { option: true }],

  jabot: async ({ seedChief }, use, testInfo) => {
    const app = await startJabotApp();
    try {
      if (seedChief) await seedChiefOnFakeAcp(app.baseURL);
      await use(app);
    } finally {
      if (testInfo.status !== testInfo.expectedStatus) {
        await attachHostLogs(testInfo, app);
      }
      await app.close();
    }
  },

  page: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => {
      errors.push(err.message);
    });
    await use(page);
    expect(errors, `unexpected page errors:\n${errors.join("\n")}`).toEqual([]);
  },
});

export { expect };
