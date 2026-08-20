import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

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
    projects: [
      {
        plugins: [react()],
        test: {
          name: "unit",
          environment: "jsdom",
          globals: true,
          setupFiles: ["./tests/support/setup-dom.ts"],
          include: ["src/**/*.test.{ts,tsx}", "src/**/__tests__/**/*.{ts,tsx}"],
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
