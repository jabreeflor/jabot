import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/**
 * Node 25+ installs a stub `globalThis.localStorage` (undefined unless
 * `--localstorage-file` is set). Vitest's jsdom environment then skips
 * installing Storage, and every unit test that touches
 * `window.localStorage` crashes. Disable Node's experimental Web Storage
 * so jsdom owns the globals — same behavior as Node 22/24.
 */
const noNodeWebStorage = ["--no-experimental-webstorage"];

/**
 * Two projects, because they need different worlds:
 *
 * - `unit` runs the renderer in jsdom (React components, host client wiring).
 * - `e2e` runs in node and spawns the real `jabot-hostd` binary, driving the
 *   real Rust host over the real NDJSON protocol. It is serial and slower;
 *   `scripts/verify.sh` builds the binary before invoking it.
 */
export default defineConfig({
  plugins: [react()],
  test: {
    // Project configs omit poolOptions; this has to live on the root so
    // every `npx vitest` worker (verify.sh, npm test, a11y) gets the flag.
    poolOptions: {
      forks: { execArgv: noNodeWebStorage },
      threads: { execArgv: noNodeWebStorage },
      vmThreads: { execArgv: noNodeWebStorage },
    },
    projects: [
      {
        plugins: [react()],
        test: {
          name: "unit",
          environment: "jsdom",
          globals: true,
          setupFiles: ["./tests/support/setup-dom.ts"],
          include: [
            "src/**/*.test.{ts,tsx}",
            "src/**/__tests__/**/*.{ts,tsx}",
            "tests/support/**/*.test.ts",
          ],
        },
      },
      {
        test: {
          name: "e2e",
          environment: "node",
          globals: true,
          include: ["tests/e2e/**/*.test.ts"],
          testTimeout: 30_000,
          hookTimeout: 30_000,
        },
      },
    ],
  },
});
