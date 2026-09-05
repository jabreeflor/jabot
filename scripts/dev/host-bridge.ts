//! One `jabot-hostd` process, shared by every browser tab the dev server has.
//!
//! This is the server half of `src/host/devTransport.ts`, kept free of Vite
//! so `tests/e2e/dev-bridge.test.ts` can drive it against the real binary the
//! way the rest of the e2e suite drives the host: nothing here is mocked. The
//! plugin in `host-plugin.ts` is the thin part — it plugs this into
//! `server.ws` and two HTTP routes.
//!
//! The host is spawned on stdio exactly as `tests/support/hostd.ts` spawns it,
//! and speaks the same NDJSON. Two things are added on the way through:
//!
//! **Ids are rewritten per client.** Every tab's transport already tags its
//! own ids, but the bridge cannot trust a tab to be unique, so it substitutes
//! a wire id of its own and answers each response on the connection that
//! asked. Notifications have no id and no addressee: they are handed to every
//! `onNotification` subscriber, which is how the plugin broadcasts them.
//!
//! **The bridge says hello first.** stdio is the host's local console, and
//! the host refuses everything on a connection until `host/hello` has been
//! answered on it. The bridge sends that itself the moment the process is
//! up, so a request from `/__jabot/rpc` works before any tab has loaded,
//! and `status()` can report the host's name and version as proof of life.
//!
//! **A dead host answers instead of hanging.** If the process exits, every
//! in-flight request gets an error response naming the exit and the stderr
//! tail, and the next request tries to start it again. A missing binary is
//! the same shape of answer, pointing at `scripts/live.sh setup`.

import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import {
  HOST_HELLO,
  JSONRPC_VERSION,
  RPC_ERROR,
  type HelloResult,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type RequestId,
} from "../../src/host/protocol";

export type BridgeFrame = JsonRpcResponse | JsonRpcNotification;

/** One attached browser tab, as far as the bridge is concerned. */
export interface BridgeClient {
  send(frame: BridgeFrame): void;
}

export interface BridgeStatus {
  running: boolean;
  pid: number | null;
  /** The host's own answer to the bridge's `host/hello`; null until it comes. */
  hello: { hostName: string; version: string; hostId: string } | null;
  binary: string;
  dataDir: string;
  /** Frames forwarded to the host since the bridge was created. */
  requests: number;
  /** How the last process ended, if one has. */
  exit: { code: number | null; signal: string | null } | null;
  /** The last few stderr lines from the current or last process. */
  stderr: string[];
}

/**
 * A harness the host should know about that is not on PATH. Written to
 * `<dataDir>/custom_harnesses/<id>.json` before the host starts, which is
 * the same file a person would drop there and the host syncs into its
 * catalog at load (`src-tauri/src/host/harness/catalog.rs`).
 */
export interface CustomHarness {
  id: string;
  label: string;
  command: string;
  args?: string[];
}

export interface HostBridgeOptions {
  binary: string;
  dataDir: string;
  /** Merged over `process.env`. `JABOT_SECRETS_BACKEND=memory` is the usual one. */
  env?: Record<string, string>;
  /** Installed into the data directory on every start, so a fresh one has them too. */
  harnesses?: CustomHarness[];
  log?: (line: string) => void;
}

export interface HostBridge {
  /** A frame from a tab. Malformed input is answered, never thrown. */
  handle(frame: unknown, client: BridgeClient): void;
  /** A request from the bridge's own side: the `/__jabot/rpc` route, a test. */
  request(request: JsonRpcRequest): Promise<JsonRpcResponse>;
  onNotification(handler: (notification: JsonRpcNotification) => void): () => void;
  status(): BridgeStatus;
  /** Stop the host. Pending requests are answered with an error. */
  close(): void;
}

const STDERR_TAIL = 20;

interface Waiter {
  id: RequestId;
  client: BridgeClient;
}

function errorResponse(id: RequestId, code: number, message: string): JsonRpcResponse {
  return { jsonrpc: JSONRPC_VERSION, id, error: { code, message } };
}

function isRequest(value: unknown): value is JsonRpcRequest {
  if (typeof value !== "object" || value === null) return false;
  const frame = value as Record<string, unknown>;
  if (frame.jsonrpc !== JSONRPC_VERSION) return false;
  if (typeof frame.method !== "string") return false;
  if (!("id" in frame)) return false;
  const id = frame.id;
  return typeof id === "number" || typeof id === "string" || id === null;
}

export function createHostBridge(options: HostBridgeOptions): HostBridge {
  const log = options.log ?? (() => {});
  const pending = new Map<string, Waiter>();
  const listeners = new Set<(notification: JsonRpcNotification) => void>();
  let child: ChildProcessWithoutNullStreams | null = null;
  let buffer = "";
  let stderrLines: string[] = [];
  let exit: BridgeStatus["exit"] = null;
  let requests = 0;
  let seq = 0;
  let closed = false;
  let startError: string | null = null;
  let hello: BridgeStatus["hello"] = null;

  const failAll = (message: string) => {
    for (const [, waiter] of pending) {
      waiter.client.send(errorResponse(waiter.id, RPC_ERROR.INTERNAL_ERROR, message));
    }
    pending.clear();
  };

  const deliver = (line: string) => {
    let frame: unknown;
    try {
      frame = JSON.parse(line);
    } catch {
      log(`unparseable line from host: ${line.slice(0, 200)}`);
      return;
    }
    if (typeof frame !== "object" || frame === null) return;
    const message = frame as Record<string, unknown>;
    if ("id" in message) {
      const wire = typeof message.id === "string" ? message.id : null;
      const waiter = wire ? pending.get(wire) : undefined;
      if (!waiter) {
        log(`response for unknown id ${String(message.id)}`);
        return;
      }
      pending.delete(wire as string);
      waiter.client.send({ ...(message as unknown as JsonRpcResponse), id: waiter.id });
      return;
    }
    if (typeof message.method === "string") {
      const notification = message as unknown as JsonRpcNotification;
      for (const listener of listeners) listener(notification);
    }
  };

  const start = (): boolean => {
    if (child) return true;
    if (closed) {
      startError = "bridge closed";
      return false;
    }
    if (!existsSync(options.binary)) {
      startError = `${options.binary} is not built — run scripts/live.sh setup`;
      log(startError);
      return false;
    }
    mkdirSync(options.dataDir, { recursive: true });
    for (const harness of options.harnesses ?? []) {
      const dir = path.join(options.dataDir, "custom_harnesses");
      mkdirSync(dir, { recursive: true });
      writeFileSync(path.join(dir, `${harness.id}.json`), `${JSON.stringify(harness, null, 2)}\n`);
    }
    const env = { ...process.env, ...options.env };
    const proc = spawn(options.binary, ["--data-dir", options.dataDir], {
      stdio: ["pipe", "pipe", "pipe"],
      env,
    });
    child = proc;
    buffer = "";
    stderrLines = [];
    exit = null;
    startError = null;
    hello = null;
    proc.stdout.setEncoding("utf8");
    proc.stderr.setEncoding("utf8");
    proc.stdout.on("data", (chunk: string) => {
      buffer += chunk;
      let newline: number;
      while ((newline = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (line) deliver(line);
      }
    });
    proc.stderr.on("data", (chunk: string) => {
      for (const line of chunk.split("\n")) {
        if (!line) continue;
        stderrLines.push(line);
        if (stderrLines.length > STDERR_TAIL) stderrLines.shift();
      }
    });
    const ended = (message: string) => {
      if (child !== proc) return;
      child = null;
      log(message);
      failAll(`${message}${stderrLines.length ? `: ${stderrLines.join(" | ")}` : ""}`);
    };
    proc.on("exit", (code, signal) => {
      exit = { code, signal };
      ended(`jabot-hostd exited (code ${code}, signal ${signal})`);
    });
    proc.on("error", (err) => {
      exit = { code: null, signal: null };
      startError = err.message;
      ended(`jabot-hostd failed to start: ${err.message}`);
    });
    log(`spawned jabot-hostd pid ${proc.pid} (data: ${options.dataDir})`);
    return true;
  };

  // `own` marks the bridge's own frames (its hello), which are not counted
  // as client traffic in `status()`.
  const forward = (request: JsonRpcRequest, client: BridgeClient, own = false) => {
    const wasRunning = child !== null;
    if (!start() || !child) {
      client.send(
        errorResponse(
          request.id,
          RPC_ERROR.INTERNAL_ERROR,
          `jabot-hostd is not running: ${startError ?? "unknown reason"}`,
        ),
      );
      return;
    }
    // A host started on demand (the last one died) has to be greeted before
    // the request that woke it, or that request is refused for want of hello.
    if (!wasRunning && request.method !== HOST_HELLO) greet();
    const wire = `b${++seq}`;
    pending.set(wire, { id: request.id, client });
    if (!own) requests += 1;
    child.stdin.write(`${JSON.stringify({ ...request, id: wire })}\n`);
  };

  const greet = () => {
    forward(
      { jsonrpc: JSONRPC_VERSION, id: "bridge-hello", method: HOST_HELLO, params: {} },
      {
        send(frame) {
          if (!("id" in frame)) return;
          if (frame.error) {
            log(`host/hello failed: ${frame.error.message}`);
            return;
          }
          const result = frame.result as HelloResult;
          hello = { hostName: result.hostName, version: result.version, hostId: result.hostId };
          log(`host/hello: ${result.hostName} v${result.version}`);
        },
      },
      true,
    );
  };

  if (start()) greet();

  return {
    handle(frame, client) {
      if (!isRequest(frame)) {
        client.send(
          errorResponse(null, RPC_ERROR.INVALID_REQUEST, "not a JSON-RPC 2.0 request"),
        );
        return;
      }
      forward(frame, client);
    },
    request(request) {
      return new Promise((resolve) => {
        forward(request, { send: (frame) => resolve(frame as JsonRpcResponse) });
      });
    },
    onNotification(handler) {
      listeners.add(handler);
      return () => {
        listeners.delete(handler);
      };
    },
    status() {
      return {
        running: child !== null,
        pid: child?.pid ?? null,
        hello,
        binary: options.binary,
        dataDir: options.dataDir,
        requests,
        exit,
        stderr: [...stderrLines],
      };
    },
    close() {
      closed = true;
      const proc = child;
      child = null;
      failAll("bridge closed");
      if (proc) {
        proc.stdin.end();
        proc.kill();
      }
    },
  };
}
