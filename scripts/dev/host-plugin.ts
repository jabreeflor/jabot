//! `jabot-host`: the Vite plugin that puts a real host behind `npm run dev`.
//!
//! With this in `vite.config.ts`, a plain `vite` — on Linux, in a container,
//! anywhere without a webview — serves a renderer that talks to a live
//! `jabot-hostd` instead of showing "Host unreachable". The renderer side is
//! `src/host/devTransport.ts`; the process side is `host-bridge.ts`; this file
//! is the wiring between them and Vite:
//!
//! - frames from `import.meta.hot.send(HOST_BRIDGE_EVENT, …)` go to the bridge,
//!   and answers come back on the same client;
//! - host notifications are broadcast to every tab;
//! - `define` sets `import.meta.env.JABOT_LIVE_HOST = "1"`, which is how
//!   `defaultTransport()` knows to use the bridge at all;
//! - `GET /__jabot/host` reports whether the host is up (what
//!   `scripts/live.sh up` waits for), and `POST /__jabot/rpc` forwards one
//!   JSON-RPC request from outside the browser (seeding folders and threads
//!   from a script, without clicking through the UI);
//! - when `fake-acp-agent` is built, it is registered as the `fake-acp`
//!   harness, so a bot can be put on it and a thread can run end to end on a
//!   machine with no `claude` installed — the same trick
//!   `tests/e2e/chief.test.ts` uses.
//!
//! It applies to `vite serve` only, and steps aside under `tauri dev`, where
//! the app's own in-process host is the one that should answer.

import { existsSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import path from "node:path";

import type { Plugin } from "vite";

import { HOST_BRIDGE_EVENT } from "../../src/host/devTransport";
import { JSONRPC_VERSION, RPC_ERROR } from "../../src/host/protocol";
import {
  createHostBridge,
  type BridgeFrame,
  type CustomHarness,
  type HostBridge,
} from "./host-bridge";

export interface JabotHostOptions {
  /** Defaults to `$JABOT_HOSTD_BIN`, else `src-tauri/target/debug/jabot-hostd`. */
  binary?: string;
  /** Defaults to `$JABOT_DEV_DATA_DIR`, else `.jabot-dev/data` under the root. */
  dataDir?: string;
  /**
   * Defaults to `$JABOT_FAKE_ACP_BIN`, else `src-tauri/target/debug/fake-acp-agent`;
   * registered as the `fake-acp` harness when the file exists. `false` to skip.
   */
  fakeAgent?: string | false;
}

/** The harness id `fake-acp-agent` is registered under. */
export const FAKE_HARNESS_ID = "fake-acp";

/** The status route, so a shell can poll readiness without a browser. */
export const HOST_STATUS_PATH = "/__jabot/host";
/** The out-of-band request route: one JSON-RPC request per POST. */
export const HOST_RPC_PATH = "/__jabot/rpc";

function sendJson(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let body = "";
    req.setEncoding("utf8");
    req.on("data", (chunk: string) => {
      body += chunk;
    });
    req.on("end", () => resolve(body));
    req.on("error", reject);
  });
}

export function jabotHost(options: JabotHostOptions = {}): Plugin {
  let bridge: HostBridge | null = null;

  return {
    name: "jabot-host",
    // `tauri dev` runs this same config for its webview, which has the real
    // in-process host; spawning a second one there would only compete for
    // the data directory. Tauri sets TAURI_ENV_* for its dev command.
    apply: (_config, env) =>
      env.command === "serve" &&
      !process.env.TAURI_ENV_PLATFORM &&
      process.env.JABOT_LIVE_HOST !== "0",

    config: () => ({
      define: { "import.meta.env.JABOT_LIVE_HOST": JSON.stringify("1") },
    }),

    configureServer(server) {
      const root = server.config.root;
      const binary =
        options.binary ??
        process.env.JABOT_HOSTD_BIN ??
        path.join(root, "src-tauri", "target", "debug", "jabot-hostd");
      const dataDir =
        options.dataDir ??
        process.env.JABOT_DEV_DATA_DIR ??
        path.join(root, ".jabot-dev", "data");

      const harnesses: CustomHarness[] = [];
      const fakeAgent =
        options.fakeAgent ??
        process.env.JABOT_FAKE_ACP_BIN ??
        path.join(root, "src-tauri", "target", "debug", "fake-acp-agent");
      if (fakeAgent !== false && existsSync(fakeAgent)) {
        harnesses.push({ id: FAKE_HARNESS_ID, label: "Fake ACP", command: fakeAgent, args: [] });
      }

      const logger = server.config.logger;
      bridge = createHostBridge({
        binary,
        dataDir,
        harnesses,
        env: {
          // Linux has no Keychain; the in-RAM vault is what CI uses too.
          JABOT_SECRETS_BACKEND: process.env.JABOT_SECRETS_BACKEND ?? "memory",
        },
        log: (line) => logger.info(`[jabot-host] ${line}`, { timestamp: true }),
      });
      const active = bridge;

      active.onNotification((notification) => {
        server.ws.send(HOST_BRIDGE_EVENT, notification);
      });

      server.ws.on(HOST_BRIDGE_EVENT, (data: unknown, client) => {
        active.handle(data, {
          send: (frame: BridgeFrame) => client.send(HOST_BRIDGE_EVENT, frame),
        });
      });

      server.middlewares.use(HOST_STATUS_PATH, (_req, res) => {
        sendJson(res, 200, active.status());
      });

      server.middlewares.use(HOST_RPC_PATH, (req, res) => {
        if (req.method !== "POST") {
          sendJson(res, 405, { error: "POST one JSON-RPC 2.0 request" });
          return;
        }
        readBody(req)
          .then(async (body) => {
            let request: unknown;
            try {
              request = JSON.parse(body);
            } catch {
              sendJson(res, 400, {
                jsonrpc: JSONRPC_VERSION,
                id: null,
                error: { code: RPC_ERROR.PARSE_ERROR, message: "body is not JSON" },
              });
              return;
            }
            // `handle` validates the shape and answers on the client it is
            // given, so the HTTP route is just a one-shot client.
            const response = await new Promise<BridgeFrame>((resolve) => {
              active.handle(request, { send: resolve });
            });
            sendJson(res, 200, response);
          })
          .catch((err: Error) => sendJson(res, 500, { error: err.message }));
      });

      server.httpServer?.once("close", () => {
        active.close();
        if (bridge === active) bridge = null;
      });
    },
  };
}
