/**
 * End-to-end: the Mobile Inbox client (#29), two clients on one host.
 *
 * This is the case the issue exists for and the one nothing else in the suite
 * covers: **a second device, with a narrower role, answers a permission, and
 * the agent hears it.** Everything is real — one `jabot-hostd`, real SQLite, a
 * real ACP subprocess, the production `HostClient` on the desktop side and the
 * production `MobileSession` on the phone side.
 *
 * What makes it a second *device* rather than a second window:
 *
 * - It connects over its own transport (`--listen`, a Unix socket) and gets
 *   its own connection, so `host/hello` binds it to its own device.
 * - It was paired through the #19 handshake and granted `approver`, and the
 *   device half of that handshake is `tests/support/pairing.ts` — an
 *   implementation written from the protocol docs, not a call into host code.
 * - The host, not the client, decides what it may do and who it answered as.
 *
 * The proof that the answer *arrived* is the adapter's own stderr: the fake
 * agent logs `permission_reply=` when the host hands it the outcome. Asserting
 * on the client's return value alone would pass even if nothing had reached
 * ACP at all.
 *
 * The vault runs in-process (`JABOT_SECRETS_BACKEND=memory`) because Linux CI
 * has no Keychain; see `tests/e2e/pairing.test.ts` for what that costs.
 */
import { mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  HOST_HELLO,
  JSONRPC_VERSION,
  PAIRING_START,
  PERMISSION_ASK,
  PERMISSION_RESOLVED,
  RPC_ERROR,
  SESSION_UPDATE,
  THREAD_DELETE,
  THREAD_TRANSCRIPT,
  type PairingQr,
  type PairingStartResult,
  type PermissionAskParams,
  type PermissionResolvedParams,
} from "../../src/host/protocol";
import { APPROVER_METHODS, checkScope } from "../../src/mobile/scope";
import { MobileSession } from "../../src/mobile/session";
import { createLineTransport, type LineTransport } from "../../src/mobile/transport";
import { fakeAcpRuntime, HostdProcess } from "../support/hostd";
import { TestDevice } from "../support/pairing";
import { connectUnixSocket } from "../support/socket";

const running: HostdProcess[] = [];
const phones: Array<{ session: MobileSession; transport: LineTransport }> = [];
const dataDirs: string[] = [];

/** A host that both a stdio client and a socket client can reach. */
async function twoClientHost() {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-mobile-"));
  dataDirs.push(dir);
  const host = new HostdProcess({
    dataDir: dir,
    socket: path.join(dir, "host.sock"),
    env: { JABOT_SECRETS_BACKEND: "memory" },
  });
  running.push(host);
  const desktop = new HostClient(host);
  await desktop.connect();
  // The binary binds the socket before it reads stdio, so an answer here means
  // the phone can dial in. No polling, no banner to parse.
  const hello = await desktop.hello();
  return { host, desktop, hello };
}

/** Run #19's handshake and come back with what the phone ends up holding. */
async function pairPhone(desktop: HostClient, role: "full" | "approver" = "approver") {
  const phone = new TestDevice();
  const start: PairingStartResult = await desktop.startPairing();
  const qr = JSON.parse(start.qrPayload) as PairingQr;
  const derived = phone.derive(qr, qr.secret);

  await desktop.claimPairing({
    pairingId: qr.pairingId,
    device: phone.descriptor(),
    mac: derived.claimMac,
  });
  await desktop.confirmPairing({
    pairingId: qr.pairingId,
    side: "device",
    sas: derived.sas,
    mac: derived.confirmMac,
  });
  await desktop.confirmPairing({
    pairingId: qr.pairingId,
    side: "host",
    sas: derived.sas,
    role,
  });
  return { phone, qr, derived };
}

/**
 * Attach the phone over the socket, as the paired device.
 *
 * The counter climbs per connection because the host refuses one it has
 * already accepted — that is what makes a captured hello frame worthless.
 */
async function attachPhone(
  host: HostdProcess,
  device: TestDevice,
  hostId: string,
  token: string,
  counter = 1,
) {
  const channel = connectUnixSocket(host.socketPath!);
  await channel.ready;
  const transport = createLineTransport(channel);
  const session = new MobileSession({
    transport,
    credentials: {
      deviceId: device.deviceId,
      name: device.name,
      signHello: () => device.helloAuth(hostId, token, counter),
    },
  });
  phones.push({ session, transport });
  const hello = await session.connect();
  return { session, transport, hello };
}

async function openThread(client: HostClient, threadId: string, mode = "permission") {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

/** Poll a client-side condition — the phone applies notifications as they land. */
async function until(check: () => boolean, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (check()) return;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  throw new Error("condition never became true");
}

afterEach(async () => {
  for (const phone of phones.splice(0)) {
    phone.session.disconnect();
    phone.transport.close();
  }
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("the Mobile Inbox client", () => {
  it("serves a second client the same protocol, on its own connection", async () => {
    const { host, desktop, hello } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    const attached = await attachPhone(host, phone, qr.hostId, derived.token);

    // Same host, two devices. The desktop is still itself.
    expect(attached.hello.hostId).toBe(hello.hostId);
    expect(attached.hello.device).toMatchObject({
      deviceId: phone.deviceId,
      role: "approver",
    });

    // The host's own answer to "what may this device call", and the client's
    // copy of it. A role narrowed in Rust fails here rather than in a phone.
    // The set, not the order: the host filters its own method list, and which
    // list something is filtered from is not part of the contract.
    expect([...(attached.hello.scopedMethods ?? [])].sort()).toEqual(
      [...APPROVER_METHODS].sort(),
    );
    expect(checkScope(attached.session.scopedMethods)).toEqual({
      missingHere: [],
      refusedThere: [],
    });

    // The desktop did not say hello again, and is still the console: a
    // `full`-only method on its connection is unaffected by the phone having
    // said hello on another. If one binding served the whole process, this is
    // the call that would come back `DEVICE_SCOPE`.
    const devices = await desktop.listDevices();
    expect(devices.devices.find((d) => d.deviceId === phone.deviceId)).toMatchObject({
      role: "approver",
      connected: true,
    });
  });

  it("answers a permission from the phone, and the agent hears it", async () => {
    const { host, desktop } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    const { session } = await attachPhone(host, phone, qr.hostId, derived.token);

    await openThread(desktop, "t-mobile");
    await desktop.prompt({ threadId: "t-mobile", content: "rm -rf" });

    // The ask is broadcast, so it reaches a client that did not provoke it.
    // The phone builds its card from the notification alone — no round trip,
    // which is the case where the notification is what woke the device.
    await until(() => session.inbox.needs.length > 0);
    const card = session.inbox.needs[0];
    expect(card).toMatchObject({ threadId: "t-mobile", title: "Run ls" });
    // The agent's own options, through the host, onto the phone, unaltered.
    expect(card.ask?.options.map((o) => o.optionId)).toEqual([
      "allow_once",
      "reject_once",
    ]);

    const requestId = card.ask!.requestId;
    const answered = await session.answer(requestId, "allow_once");
    expect(answered).toMatchObject({ delivered: true, alreadyAnswered: false });

    // The claim this whole issue rests on: it reached ACP. The fake agent
    // logs what the host handed it, and the host tees adapter stderr to disk.
    await until(() => host.readAdapterLog("t-mobile").includes("permission_reply="));
    expect(host.readAdapterLog("t-mobile")).toContain("allow_once");

    // The desktop is told, and told *who* — including that it was not itself.
    const resolved = (await host.waitFor(
      (n) =>
        n.method === PERMISSION_RESOLVED &&
        (n.params as PermissionResolvedParams).requestId === requestId,
    )).params as PermissionResolvedParams;
    expect(resolved).toMatchObject({
      threadId: "t-mobile",
      optionId: "allow_once",
      deviceId: phone.deviceId,
    });

    // One answer, not two: the question is gone for everyone.
    expect((await desktop.pendingPermissions({ threadId: "t-mobile" })).requests).toEqual([]);
    expect(session.inbox.needs).toEqual([]);
  });

  it("keeps the phone inside its role, on the host", async () => {
    const { host, desktop } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    const { transport } = await attachPhone(host, phone, qr.hostId, derived.token);
    await openThread(desktop, "t-scope", "echo");

    // Straight down the phone's own transport, past anything the client would
    // refuse locally: the enforcement has to be the host's, or it is not one.
    const deleted = await transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-delete",
      method: THREAD_DELETE,
      params: { threadId: "t-scope" },
    });
    expect(deleted.error?.code).toBe(RPC_ERROR.DEVICE_SCOPE);
    expect((deleted.error?.data as { role: string }).role).toBe("approver");
    // Refused, not "refused and then done anyway".
    expect((await desktop.threadState({ threadId: "t-scope" })).state).toBe("active");

    // What the phone *is* for still works over the same connection.
    const transcript = await transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-transcript",
      method: THREAD_TRANSCRIPT,
      params: { threadId: "t-scope" },
    });
    expect(transcript.error).toBeUndefined();
  });

  it("will not let the phone be recorded as the Mac", async () => {
    const { host, desktop, hello } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    const { session, transport } = await attachPhone(host, phone, qr.hostId, derived.token);

    await openThread(desktop, "t-attrib");
    await desktop.prompt({ threadId: "t-attrib", content: "rm -rf" });
    const asked = (await host.waitFor(
      (n) =>
        n.method === PERMISSION_ASK &&
        (n.params as PermissionAskParams).threadId === "t-attrib",
    )).params as PermissionAskParams;

    // "Who answered" is what the record keeps and what every other client is
    // told. A device that could write somebody else's id into it could be
    // recorded as the console it is deliberately not.
    const forged = await transport.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-reply",
      method: "permission/reply",
      params: {
        requestId: asked.requestId,
        deviceId: hello.device.deviceId,
        optionId: "allow_once",
      },
    });
    expect(forged.error?.code).toBe(RPC_ERROR.INVALID_PARAMS);

    // The ask is untouched by the attempt, and answering honestly still works.
    await until(() => session.inbox.needs.length > 0);
    await session.answer(asked.requestId, "allow_once");
    const pending = await desktop.pendingPermissions({ threadId: "t-attrib" });
    expect(pending.requests).toEqual([]);
  });

  it("shows the phone the same three states the desktop shows", async () => {
    const { host, desktop } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    const { session } = await attachPhone(host, phone, qr.hostId, derived.token);

    // One thread folded away, one holding a question.
    await openThread(desktop, "t-asleep", "echo");
    await desktop.fold({ threadId: "t-asleep" });
    await openThread(desktop, "t-asks");
    await desktop.prompt({ threadId: "t-asks", content: "rm -rf" });
    await until(() => session.inbox.needs.length > 0);

    const inbox = await session.refresh();
    // Needs you: answerable, and the card carries the ask.
    expect(inbox.needs.map((c) => c.threadId)).toContain("t-asks");
    expect(inbox.needs.find((c) => c.threadId === "t-asks")?.ask).toBeTruthy();
    // Still sleeping: present, and with nothing to answer. Decision #5 — a
    // folded thread is not a notification, least of all on a phone.
    expect(inbox.sleeping.map((c) => c.threadId)).toEqual(["t-asleep"]);
    expect(inbox.sleeping[0].ask).toBeUndefined();
  });

  it("does not let a socket client talk its way into being the Mac", async () => {
    const { host, hello } = await twoClientHost();

    // A client with nothing but the socket path: no pairing, no credential.
    const channel = connectUnixSocket(host.socketPath!);
    await channel.ready;
    const raw = createLineTransport(channel);

    // The two names the host already knows. Neither is a credential: a bare
    // hello used to be answered as "the console, because who else would be
    // here", and the console's own id is printed in every hello, health and
    // device/list answer, so it is a public string.
    const bare = await raw.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-hello",
      method: HOST_HELLO,
      params: {},
    });
    expect(bare.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);

    const borrowed = await raw.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-hello-as-mac",
      method: HOST_HELLO,
      params: { device: { deviceId: hello.device.deviceId } },
    });
    expect(borrowed.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);

    // What it would have got if either had worked: `pairing/start` hands back
    // the QR payload — the pairing secret included — so an admitted stranger
    // does not need to defeat #19's handshake, it can run it.
    const started = await raw.request({
      jsonrpc: JSONRPC_VERSION,
      id: "raw-pair",
      method: PAIRING_START,
      params: {},
    });
    expect(started.result).toBeUndefined();
    expect(started.error?.code).toBe(RPC_ERROR.HELLO_REQUIRED);
    raw.close();
  });

  it("streams nothing to a connection that has not said who it is", async () => {
    const { host, desktop } = await twoClientHost();

    // Connected, silent, and listening — the cheapest attack there is.
    const channel = connectUnixSocket(host.socketPath!);
    await channel.ready;
    const overheard: string[] = [];
    channel.onLine((line) => {
      if (line.trim()) overheard.push(line);
    });

    await openThread(desktop, "t-quiet", "echo");
    await desktop.prompt({ threadId: "t-quiet", content: "hello secret" });
    // The desktop is on stdio and always hears it; the broadcast that fed it
    // is the same call that would have fed the socket.
    await host.waitFor(
      (n) =>
        n.method === SESSION_UPDATE &&
        JSON.stringify(n.params).includes("hello secret"),
    );
    // Delivery to the socket would be a separate write on the same lock, so
    // give it longer than it could possibly need to arrive.
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(overheard).toEqual([]);

    // And the gate is "unidentified", not "socket": a paired phone on an
    // identical connection hears the next turn.
    const { phone, qr, derived } = await pairPhone(desktop);
    const { session } = await attachPhone(host, phone, qr.hostId, derived.token);
    await openThread(desktop, "t-loud");
    await desktop.prompt({ threadId: "t-loud", content: "rm -rf" });
    await until(() => session.inbox.needs.length > 0);
    // The silent one still heard none of it.
    expect(overheard).toEqual([]);
    channel.close();
  });

  it("puts the socket somewhere only this user can reach", async () => {
    const { host } = await twoClientHost();
    // `pairing-security-mobile.md` rule 1 lets rung 0 skip TLS *because* the
    // socket is "`0700` in a user dir", and D-016 calls that the whole of the
    // protection. Default umask would make it `0755`: every account on the
    // machine could connect, and the two refusals above would be the only
    // thing standing between them and the host.
    expect(statSync(host.socketPath!).mode & 0o777).toBe(0o600);
    expect(statSync(path.dirname(host.socketPath!)).mode & 0o077).toBe(0);
  });

  it("refuses a phone that was revoked, on its next connection", async () => {
    const { host, desktop } = await twoClientHost();
    const { phone, qr, derived } = await pairPhone(desktop);
    await attachPhone(host, phone, qr.hostId, derived.token, 1);

    await desktop.revokeDevice({ deviceId: phone.deviceId });

    // A fresh connection with a fresh counter: correct proof, revoked device.
    await expect(
      attachPhone(host, phone, qr.hostId, derived.token, 2),
    ).rejects.toBeInstanceOf(HostRpcError);
  });
});
