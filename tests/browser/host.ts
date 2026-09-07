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
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { TestInfo } from "@playwright/test";

import {
  CREW_THREAD,
  CREW_UPDATE,
  FOLDER_REGISTER,
  HOST_HEALTH,
  JSONRPC_VERSION,
  THREAD_OPEN,
  THREAD_TRANSCRIPT,
  type JsonRpcResponse,
  type RuntimeSpec,
  type ThreadStateResult,
  type ThreadTranscriptResult,
} from "../../src/host/protocol";
import { fakeAcpAgentPath, hostdBinaryPath } from "../support/hostd";
import { FAKE_ACP_ASK_ID } from "./support/constants";

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

export interface StartJabotOptions {
  /** Directories prepended to PATH (a fixture `gh`, never a real token). */
  pathPrefix?: string[];
  extraEnv?: Record<string, string>;
}

export interface JabotApp {
  baseURL: string;
  port: number;
  dataDir: string;
  logPath: string;
  rpc: <T = unknown>(method: string, params?: unknown) => Promise<T>;
  hostStatus: () => Promise<HostStatus>;
  adapterLog: (threadId: string) => string;
  /** Kill Vite + host and start them again on the same port and data dir. */
  restart: () => Promise<void>;
  /**
   * SIGKILL the owned jabot-hostd only. Vite stays up. The bridge will not
   * spawn a replacement until the next RPC (Reconnect, a poll, or `startHost`).
   */
  stopHost: () => Promise<void>;
  /** Trigger the bridge to spawn a new host on the same data directory. */
  startHost: () => Promise<void>;
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

/** Prerequisite only — put Chief on a scriptable agent before the page loads. */
export async function seedChiefOnFakeAcp(
  baseURL: string,
  harnessId = "fake-acp",
): Promise<void> {
  await hostRpc(baseURL, CREW_UPDATE, { botId: "chief", harnessId });
}

export async function chiefThreadId(baseURL: string): Promise<string> {
  const thread = await hostRpc<ThreadStateResult>(baseURL, CREW_THREAD, {
    botId: "chief",
  });
  return thread.threadId;
}

/** Register a repo folder and open a code thread. RPC prerequisite only. */
export async function seedCodeThread(
  app: JabotApp,
  spec: {
    threadId: string;
    title: string;
    folderName?: string;
    harnessId?: string;
    runtime?: RuntimeSpec;
  },
): Promise<{ folderId: string; cwd: string }> {
  const cwd = path.join(app.dataDir, "repos", spec.threadId);
  mkdirSync(cwd, { recursive: true });
  const folder = await app.rpc<{ folderId: string; cwd: string }>(
    FOLDER_REGISTER,
    {
      path: cwd,
      name: spec.folderName ?? spec.title,
    },
  );
  await app.rpc(THREAD_OPEN, {
    threadId: spec.threadId,
    title: spec.title,
    cwd: folder.cwd,
    folderId: folder.folderId,
    harnessId: spec.harnessId ?? "fake-acp",
    runtime: spec.runtime,
  });
  return folder;
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

export async function startJabotApp(
  options: StartJabotOptions = {},
): Promise<JabotApp> {
  const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-browser-"));
  writeAskHarness(dataDir, fakeAcpAgentPath());
  const port = await listenFreePort();
  const logPath = path.join(dataDir, "vite.log");
  const spawn = () => spawnVite({ dataDir, port, logPath, options });
  let child = spawn();
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
    adapterLog: (threadId) => {
      const file = path.join(dataDir, "adapter-logs", `${threadId}.stderr.log`);
      return existsSync(file) ? readFileSync(file, "utf8") : "";
    },
    async restart() {
      await stopVite(child);
      child = spawn();
      await waitForProcessAndHost(child, baseURL, logPath);
    },
    async stopHost() {
      const status = await app.hostStatus();
      if (!status.pid) throw new Error("no host pid to stop");
      process.kill(status.pid, "SIGKILL");
      const deadline = Date.now() + 10_000;
      while (Date.now() < deadline) {
        const next = await app.hostStatus();
        if (!next.running) return;
        await sleep(50);
      }
      throw new Error("host pid did not exit after SIGKILL");
    },
    async startHost() {
      await hostRpc(baseURL, HOST_HEALTH, {});
      await waitForHostReady(baseURL);
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

function spawnVite(args: {
  dataDir: string;
  port: number;
  logPath: string;
  options: StartJabotOptions;
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

  const pathValue = [...(args.options.pathPrefix ?? []), process.env.PATH ?? ""]
    .filter(Boolean)
    .join(path.delimiter);

  const log = createWriteStream(args.logPath, { flags: "a" });
  const child = spawn(
    process.execPath,
    [
      viteJs,
      "--port",
      String(args.port),
      "--strictPort",
      "--host",
      "127.0.0.1",
    ],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        ...args.options.extraEnv,
        PATH: pathValue,
        JABOT_DEV_DATA_DIR: args.dataDir,
        JABOT_SECRETS_BACKEND: "memory",
        JABOT_HOSTD_BIN: hostd,
        JABOT_FAKE_ACP_BIN: fake,
        // Never inherit a live.sh disable, and never share the developer port.
        JABOT_LIVE_HOST: "1",
        // Default off so a fixture `gh` is not polled in the background.
        JABOT_PR_POLL_MS: args.options.extraEnv?.JABOT_PR_POLL_MS ?? "0",
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
