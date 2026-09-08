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
 *
 * Suites that need Vite/host env before spawn (a fixture `gh` on PATH)
 * use `browserTest(() => ({ pathPrefix }))` so they still share this
 * fixture rather than forking one.
 */
import { test as base, expect } from "@playwright/test";

import {
  attachHostLogs,
  seedChiefOnFakeAcp,
  startJabotApp,
  type JabotApp,
  type StartJabotOptions,
} from "./host";

/** Transport / host-death noise is expected during recovery tests. */
const IGNORED_PAGE_ERRORS =
  /dev server connection lost|transport closed|jabot-hostd exited|Host disconnected|Failed to fetch|network error|WebSocket/i;

export type BrowserFixtures = {
  jabot: JabotApp;
  seedChief: boolean;
};

export type JabotStartFactory = () => StartJabotOptions;

export function browserTest(start: JabotStartFactory = () => ({})) {
  return base.extend<BrowserFixtures>({
    seedChief: [true, { option: true }],

    jabot: async ({ seedChief }, provide, testInfo) => {
      const app = await startJabotApp(start());
      try {
        if (seedChief) await seedChiefOnFakeAcp(app.baseURL);
        await provide(app);
      } finally {
        if (testInfo.status !== testInfo.expectedStatus) {
          await attachHostLogs(testInfo, app);
        }
        await app.close();
      }
    },

    page: async ({ page }, provide) => {
      const errors: string[] = [];
      page.on("pageerror", (err) => {
        if (IGNORED_PAGE_ERRORS.test(err.message)) return;
        errors.push(err.message);
      });
      await provide(page);
      expect(errors, `unexpected page errors:\n${errors.join("\n")}`).toEqual([]);
    },
  });
}

export const test = browserTest();

export { expect };
