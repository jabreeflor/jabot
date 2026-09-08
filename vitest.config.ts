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
 *
 * Coverage is configured once, at the root. It is a *unit* measurement: run
 * with `--project unit --coverage`. That is not whole-product coverage and it
 * is not the e2e suite. `src/host/client.ts` in particular looks thin here
 * because unit tests stub `HostClient`; the host-protocol project exercises
 * the same file over a live socket. See docs/coverage.md.
 *
 * `coverage.all` (default true in Vitest 3) plus an explicit `include` is what
 * makes a newly added, never-imported production file count against the
 * floor. Leaving `include` unset would report only files a test happened to
 * load, which is how a whole view can land untested without moving the number.
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
    coverage: {
      provider: "v8",
      // Unimported files matching `include` still appear in the report.
      all: true,
      reportsDirectory: "./coverage/frontend",
      reporter: ["text", "json", "json-summary", "html", "lcov"],
      // Failed tests still leave HTML/LCOV/JSON behind so CI can upload them.
      reportOnFailure: true,
      // Production renderer only. CSS, assets, and anything outside src/ are
      // out of scope — they are not TypeScript the unit project can execute.
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        // Co-located and directory tests are the measurement, not the product.
        "src/**/*.test.{ts,tsx}",
        "src/**/__tests__/**",
        // Ambient types only; no runtime to cover.
        "src/**/*.d.ts",
        // Vite/Vitest and other tooling — not shipped renderer code.
        "**/{vite,vitest,eslint,prettier}.config.*",
        // Vendored plugin snapshots live next to the app; they are not JaBot
        // renderer source even if a glob ever widened past src/.
        "plugins/**",
        // Nested git worktrees (agent checkouts, local `git worktree add`)
        // must not be scored as if they were this tree's production files.
        "worktrees/**",
        "**/.git/**",
        // Generated / build output if a report is ever pointed at the repo
        // root rather than the include glob.
        "dist/**",
        "src-tauri/target/**",
        "coverage/**",
        "node_modules/**",
      ],
      // Floors from the c898b8f audit on Node 22 (93.0 / 85.5 / 81.2
      // lines/branches/functions), rounded down to the proposed 90/85/80 so
      // a meaningful drop fails the gate without chasing noise. Statements
      // track lines at the same 90. Justification and the unit-vs-e2e split:
      // docs/coverage.md.
      thresholds: {
        lines: 90,
        branches: 85,
        functions: 80,
        statements: 90,
      },
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
