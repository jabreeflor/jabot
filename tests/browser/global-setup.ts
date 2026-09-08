/**
 * Build `jabot-hostd` and `fake-acp-agent` when they are missing.
 *
 * CI builds them in the job before this runs. Locally a stale tree is the
 * usual miss — say so and compile rather than failing every test on ENOENT.
 */
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { fakeAcpAgentPath, hostdBinaryPath } from "../support/hostd";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export default function globalSetup(): void {
  const hostd = hostdBinaryPath();
  let fake = "";
  try {
    fake = fakeAcpAgentPath();
  } catch {
    fake = "";
  }
  if (existsSync(hostd) && fake && existsSync(fake)) return;

  execFileSync(
    "cargo",
    ["build", "--manifest-path", "src-tauri/Cargo.toml", "--features", "dev-bins", "--bins"],
    { cwd: repoRoot, stdio: "inherit" },
  );
}
