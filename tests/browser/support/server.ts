//! Isolated Vite + jabot-hostd for one Playwright test or worker.
//!
//! Owns a temp data directory and a dedicated port. Never touches
//! `.jabot-dev/` or calls `live.sh reset/smoke`. Teardown reaps the process
//! group (vite, esbuild, jabot-hostd, adapters) and removes only this dir.

import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { fakeAcpAgentPath, hostdBinaryPath } from "../../support/hostd";
import { FAKE_ACP_ASK_ID } from "./constants";
import { waitForHost, type HostStatus } from "./rpc";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);

export interface BrowserApp {
  url: string;
  port: number;
  dataDir: string;
  logs(): string;
  status: HostStatus;
  close(): Promise<void>;
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (typeof address !== "object" || address === null) {
        server.close();
        reject(new Error("could not bind an ephemeral port"));
        return;
      }
      const { port } = address;
      server.close((error) => (error ? reject(error) : resolve(port)));
    });
  });
}

function requireBinaries(): { hostd: string; fakeAcp: string } {
  const hostd = hostdBinaryPath();
  const fakeAcp = fakeAcpAgentPath();
  if (!existsSync(hostd)) {
    throw new Error(
      `${hostd} is missing; run: npm run host:build  (or ./scripts/test-browser.sh)`,
    );
  }
  if (!existsSync(fakeAcp)) {
    throw new Error(
      `${fakeAcp} is missing; run: npm run host:build  (or ./scripts/test-browser.sh)`,
    );
  }
  return { hostd, fakeAcp };
}

function writeAskHarness(dataDir: string, fakeAcp: string): void {
  const dir = path.join(dataDir, "custom_harnesses");
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    path.join(dir, `${FAKE_ACP_ASK_ID}.json`),
    `${JSON.stringify(
      {
        id: FAKE_ACP_ASK_ID,
        label: "Fake ACP (permission)",
        command: fakeAcp,
        args: ["permission"],
      },
      null,
      2,
    )}\n`,
  );
}

function killTree(child: ChildProcess): void {
  const pid = child.pid;
  if (pid == null) return;
  try {
    process.kill(-pid, "SIGTERM");
  } catch {
    try {
      child.kill("SIGTERM");
    } catch {
      // already gone
    }
  }
}

export async function startBrowserApp(): Promise<BrowserApp> {
  const { hostd, fakeAcp } = requireBinaries();
  const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-browser-"));
  writeAskHarness(dataDir, fakeAcp);
  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  const logChunks: string[] = [];

  const child = spawn(
    process.execPath,
    [
      path.join(repoRoot, "node_modules", "vite", "bin", "vite.js"),
      "--port",
      String(port),
      "--strictPort",
      "--host",
      "127.0.0.1",
    ],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        JABOT_HOSTD_BIN: hostd,
        JABOT_FAKE_ACP_BIN: fakeAcp,
        JABOT_DEV_DATA_DIR: dataDir,
        JABOT_SECRETS_BACKEND: "memory",
        // The plugin reads this to step aside; we want the live host.
        JABOT_LIVE_HOST: "1",
      },
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
    },
  );

  const onLog = (chunk: Buffer | string) => {
    logChunks.push(String(chunk));
    if (logChunks.length > 400) logChunks.shift();
  };
  child.stdout?.on("data", onLog);
  child.stderr?.on("data", onLog);

  let closed = false;
  const close = async () => {
    if (closed) return;
    closed = true;
    killTree(child);
    const deadline = Date.now() + 8_000;
    while (child.exitCode == null && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (child.exitCode == null) {
      try {
        if (child.pid != null) process.kill(-child.pid, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    }
    rmSync(dataDir, { recursive: true, force: true });
  };

  try {
    const status = await waitForHost(url);
    return {
      url,
      port,
      dataDir,
      status,
      logs: () => logChunks.join(""),
      close,
    };
  } catch (error) {
    const dump = logChunks.join("");
    await close();
    throw new Error(
      `${error instanceof Error ? error.message : String(error)}\n--- vite ---\n${dump.slice(-4_000)}`,
    );
  }
}
