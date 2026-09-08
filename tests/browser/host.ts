/**
 * One owned Vite + jabot-hostd for a browser test.
 *
 * Spawns `vite` on a dedicated loopback port with `JABOT_DEV_DATA_DIR` pointed
 * at a temp directory. The `jabot-host` plugin then starts the real host and
 * registers `fake-acp-agent` — the same bridge `scripts/live.sh up` uses,
 * without touching `.jabot-dev/data` or port 1420.
 *
 * Teardown kills the process group (vite, esbuild, hostd, adapters) and
 * removes only the directory this helper created.
 */
import { type ChildProcess, spawn } from "node:child_process";
import {
  createWriteStream,
  existsSync,
  mkdtempSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { TestInfo } from "@playwright/test";

import {
  CREW_THREAD,
  CREW_UPDATE,
  JSONRPC_VERSION,
  THREAD_TRANSCRIPT,
  type JsonRpcResponse,
  type ThreadStateResult,
  type ThreadTranscriptResult,
} from "../../src/host/protocol";
import { fakeAcpAgentPath, hostdBinaryPath } from "../support/hostd";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);

/** Vite's default and `scripts/live.sh` — never bind this from a test. */
export const DEVELOPER_PORT = 1420;

export interface HostStatus {
  running: boolean;
  pid: number | null;
  hello: { hostName: string; version: string; hostId: string } | null;
  binary: string;
  dataDir: string;
  requests: number;
  exit: { code: number | null; signal: string | null } | null;
  stderr: string[];
}

export interface JabotApp {
  baseURL: string;
  port: number;
  dataDir: string;
  logPath: string;
  rpc: <T = unknown>(method: string, params?: unknown) => Promise<T>;
  hostStatus: () => Promise<HostStatus>;
  /** Kill Vite + host and start them again on the same port and data dir. */
  restart: () => Promise<void>;
  close: () => Promise<void>;
}

export function listenFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      if (!addr || typeof addr === "string") {
        server.close();
        reject(new Error("listen(0) did not yield a port"));
        return;
      }
      const { port } = addr;
      if (port === DEVELOPER_PORT) {
        server.close(() => {
          listenFreePort().then(resolve, reject);
        });
        return;
      }
      server.close((err) => (err ? reject(err) : resolve(port)));
    });
  });
}

export async function waitForHostReady(
  baseURL: string,
  timeoutMs = 60_000,
): Promise<HostStatus> {
  const deadline = Date.now() + timeoutMs;
  let last = "no response";
  while (Date.now() < deadline) {
    try {
      const res = await fetch(new URL("/__jabot/host", baseURL));
      const json = (await res.json()) as HostStatus;
      last = JSON.stringify(json);
      // HTTP 200 is not enough: the plugin answers this route as soon as
      // Vite is up, before jabot-hostd has said hello.
      if (json.running === true && json.hello && json.hello.hostName) {
        return json;
      }
    } catch (err) {
      last = err instanceof Error ? err.message : String(err);
    }
    await sleep(200);
  }
  throw new Error(`host not ready at ${baseURL}/__jabot/host: ${last}`);
}

export async function hostRpc<T = unknown>(
  baseURL: string,
  method: string,
  params?: unknown,
): Promise<T> {
  const body: {
    jsonrpc: typeof JSONRPC_VERSION;
    id: string;
    method: string;
    params?: unknown;
  } = {
    jsonrpc: JSONRPC_VERSION,
    id: `browser-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    method,
  };
  if (params !== undefined) body.params = params;
  const res = await fetch(new URL("/__jabot/rpc", baseURL), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = (await res.json()) as JsonRpcResponse<T>;
  if (json.error) {
    throw new Error(
      `rpc ${method}: ${json.error.message} (${json.error.code})`,
    );
  }
  return json.result as T;
}

/** Prerequisite only — put Chief on the scriptable agent before the page loads. */
export async function seedChiefOnFakeAcp(baseURL: string): Promise<void> {
  await hostRpc(baseURL, CREW_UPDATE, {
    botId: "chief",
    harnessId: "fake-acp",
  });
}

export async function chiefTranscript(
  baseURL: string,
): Promise<ThreadTranscriptResult> {
  const thread = await hostRpc<ThreadStateResult>(baseURL, CREW_THREAD, {
    botId: "chief",
  });
  return hostRpc<ThreadTranscriptResult>(baseURL, THREAD_TRANSCRIPT, {
    threadId: thread.threadId,
  });
}

export async function startJabotApp(): Promise<JabotApp> {
  const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-browser-"));
  const port = await listenFreePort();
  const logPath = path.join(dataDir, "vite.log");
  let child = spawnVite({ dataDir, port, logPath });
  const baseURL = `http://127.0.0.1:${port}`;
  await waitForProcessAndHost(child, baseURL, logPath);

  const app: JabotApp = {
    baseURL,
    port,
    dataDir,
    logPath,
    rpc: (method, params) => hostRpc(baseURL, method, params),
    hostStatus: () =>
      fetch(new URL("/__jabot/host", baseURL)).then(
        (r) => r.json() as Promise<HostStatus>,
      ),
    async restart() {
      await stopVite(child);
      child = spawnVite({ dataDir, port, logPath });
      await waitForProcessAndHost(child, baseURL, logPath);
    },
    async close() {
      await stopVite(child);
      rmSync(dataDir, { recursive: true, force: true });
    },
  };
  return app;
}

export async function attachHostLogs(
  testInfo: TestInfo,
  app: JabotApp,
): Promise<void> {
  if (existsSync(app.logPath)) {
    await testInfo.attach("vite.log", {
      path: app.logPath,
      contentType: "text/plain",
    });
  }
  const adapterDir = path.join(app.dataDir, "adapter-logs");
  if (!existsSync(adapterDir)) return;
  for (const name of readdirSync(adapterDir)) {
    await testInfo.attach(name, {
      path: path.join(adapterDir, name),
      contentType: "text/plain",
    });
  }
}

function spawnVite(options: {
  dataDir: string;
  port: number;
  logPath: string;
}): ChildProcess {
  const viteJs = path.join(repoRoot, "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(viteJs)) {
    throw new Error(`vite is not installed at ${viteJs}`);
  }
  const hostd = hostdBinaryPath();
  const fake = fakeAcpAgentPath();
  if (!existsSync(hostd)) {
    throw new Error(`${hostd} is not built — run npm run host:build`);
  }

  const log = createWriteStream(options.logPath, { flags: "a" });
  const child = spawn(
    process.execPath,
    [
      viteJs,
      "--port",
      String(options.port),
      "--strictPort",
      "--host",
      "127.0.0.1",
    ],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        JABOT_DEV_DATA_DIR: options.dataDir,
        JABOT_SECRETS_BACKEND: "memory",
        JABOT_HOSTD_BIN: hostd,
        JABOT_FAKE_ACP_BIN: fake,
        // Never inherit a live.sh disable, and never share the developer port.
        JABOT_LIVE_HOST: "1",
      },
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
    },
  );
  child.stdout?.pipe(log, { end: false });
  child.stderr?.pipe(log, { end: false });
  child.once("exit", () => log.end());
  return child;
}

async function waitForProcessAndHost(
  child: ChildProcess,
  baseURL: string,
  logPath: string,
): Promise<void> {
  const died = new Promise<never>((_, reject) => {
    child.once("exit", (code, signal) => {
      reject(
        new Error(
          `vite exited before the host came up (code ${code}, signal ${signal}); see ${logPath}`,
        ),
      );
    });
    child.once("error", reject);
  });
  await Promise.race([waitForHostReady(baseURL), died]);
}

async function stopVite(child: ChildProcess): Promise<void> {
  const pid = child.pid;
  if (pid === undefined || child.exitCode !== null) return;

  await new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      killTree(pid, "SIGKILL");
    }, 5_000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
    if (!killTree(pid, "SIGTERM")) {
      clearTimeout(timer);
      resolve();
    }
  });
}

function killTree(pid: number, signal: NodeJS.Signals): boolean {
  try {
    if (process.platform !== "win32") {
      process.kill(-pid, signal);
    } else {
      process.kill(pid, signal);
    }
    return true;
  } catch {
    return false;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
