/**
 * End-to-end: pairing a second device (#19) over the wire.
 *
 * `src-tauri/src/host/pairing/` makes these claims in-process. This file makes
 * them through the production `HostClient`, a real `jabot-hostd` and real
 * SQLite — and, importantly, with the device half computed by
 * `tests/support/pairing.ts`, which is a separate implementation written from
 * the protocol docs. So "the safety numbers match" here means two independent
 * programs agreed, not that one function was called twice.
 *
 * The host runs with an in-process secrets vault (`JABOT_SECRETS_BACKEND`),
 * because Linux CI has no Keychain and `put` otherwise fails closed. That
 * makes the *token* half of a pairing process-local here, so the restart case
 * below asserts what SQLite owns — the grant and the revoke — and does not
 * pretend a device can reconnect to a host whose vault was never persisted.
 */
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  DEVICE_LIST,
  DEVICE_REVOKE,
  HOST_HELLO,
  PAIRING_CLAIM,
  PAIRING_CONFIRM,
  PAIRING_START,
  RPC_ERROR,
  type PairingQr,
  type PairingStartResult,
} from "../../src/host/protocol";
import { normalizeCode, TestDevice } from "../support/pairing";
import { HostdProcess, type HostdOptions } from "../support/hostd";

const running: HostdProcess[] = [];
const dataDirs: string[] = [];

/** The vault has to work in-process; see the file docs. */
const HOST_ENV = { JABOT_SECRETS_BACKEND: "memory" };

async function connected(options: HostdOptions = {}) {
  const host = new HostdProcess({ persistent: true, env: HOST_ENV, ...options });
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

function ownDataDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-pairing-"));
  dataDirs.push(dir);
  return dir;
}

function qrOf(start: PairingStartResult): PairingQr {
  return JSON.parse(start.qrPayload) as PairingQr;
}

/** Run the whole handshake and come back with what both ends ended up with. */
async function pair(
  client: HostClient,
  device: TestDevice,
  role: "full" | "approver" = "approver",
) {
  const start = await client.startPairing();
  const qr = qrOf(start);
  const derived = device.derive(qr, qr.secret);

  const claim = await client.claimPairing({
    pairingId: qr.pairingId,
    secret: qr.secret,
    device: device.descriptor(),
    mac: derived.claimMac,
  });
  // The host proves it holds the secret off its own screen. Without this a
  // phone has only its own word that it is talking to the right machine.
  expect(claim.hostMac).toBe(derived.hostMac);

  const onTheHost = (await client.pairingStatus()).offers[0];
  // The assertion this whole issue exists for: the number the phone derived
  // and the number the host derived are the same number.
  expect(onTheHost.sas).toBe(derived.sas);

  await client.confirmPairing({
    pairingId: qr.pairingId,
    side: "device",
    sas: derived.sas,
    mac: derived.confirmMac,
  });
  const done = await client.confirmPairing({
    pairingId: qr.pairingId,
    side: "host",
    sas: derived.sas,
    role,
  });
  return { start, qr, derived, done };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of dataDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("pairing a second device", () => {
  it("advertises the methods a client pairs through", async () => {
    const { hello } = await connected();
    // The drift guard: a method the TS client can call has to be one the Rust
    // host admits to, or the two halves of the protocol have already parted.
    for (const method of [PAIRING_START, PAIRING_CLAIM, PAIRING_CONFIRM, DEVICE_LIST, DEVICE_REVOKE]) {
      expect(hello.methods).toContain(method);
    }
  });

  it("still refuses a device id nobody has ever paired", async () => {
    const { host } = await connected();
    // #8's promise, unweakened by #19: an id on its own is not a credential,
    // and neither is one with a made-up proof attached.
    const bare = await host.call(HOST_HELLO, {
      protocolVersion: 1,
      device: { deviceId: "phone-not-paired-yet" },
    });
    expect(bare.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);

    const forged = await host.call(HOST_HELLO, {
      protocolVersion: 1,
      device: { deviceId: "phone-not-paired-yet" },
      auth: { counter: 1, mac: "00".repeat(32) },
    });
    expect(forged.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);
  });

  it("pairs a phone from a QR, then holds it to the role the host granted", async () => {
    const { host, client, hello } = await connected();
    const phone = new TestDevice();
    const { qr, derived, done } = await pair(client, phone, "approver");

    expect(done.state).toBe("paired");
    expect(done.device).toMatchObject({
      deviceId: phone.deviceId,
      name: phone.name,
      role: "approver",
    });
    // The offer is spent: the QR on the screen is now worth nothing.
    expect((await client.pairingStatus()).offers).toEqual([]);

    const listed = await client.listDevices();
    expect(listed.devices.map((d) => d.deviceId)).toContain(phone.deviceId);
    const row = listed.devices.find((d) => d.deviceId === phone.deviceId);
    expect(row).toMatchObject({
      role: "approver",
      pairedVia: "qr",
      // What the two humans compared, kept so the list can say what was verified.
      sas: derived.sas,
      local: false,
    });
    expect(row?.revokedAt).toBeUndefined();
    // The console that spawned the host is device #1 and is not revocable.
    const local = listed.devices.find((d) => d.local);
    expect(local?.deviceId).toBe(hello.device.deviceId);
    const refused = await host.call(DEVICE_REVOKE, { deviceId: local?.deviceId });
    expect(refused.error?.code).toBe(RPC_ERROR.INVALID_PARAMS);

    // Now the phone connects, with the proof its pairing derived.
    const phoneHello = await host.call<{ device: { role: string } }>(HOST_HELLO, {
      protocolVersion: 1,
      device: { deviceId: phone.deviceId, role: "full" },
      auth: phone.helloAuth(qr.hostId, derived.token, 1),
    });
    expect(phoneHello.error).toBeUndefined();
    // It asked for `full`. It got what the human on the host granted.
    expect(phoneHello.result?.device.role).toBe("approver");

    // Scope, enforced on the host for the connection that is now a phone.
    const asPhone = await host.call(PAIRING_START, {});
    expect(asPhone.error?.code).toBe(RPC_ERROR.DEVICE_SCOPE);
    expect((asPhone.error?.data as { role: string }).role).toBe("approver");
    // What a phone is actually for still works.
    const pending = await host.call(DEVICE_LIST);
    expect(pending.error?.code).toBe(RPC_ERROR.DEVICE_SCOPE);
    const asks = await host.call("permission/pending", {});
    expect(asks.error).toBeUndefined();

    // Replaying the phone's own hello frame does not work either.
    const replay = await host.call(HOST_HELLO, {
      protocolVersion: 1,
      device: { deviceId: phone.deviceId },
      auth: phone.helloAuth(qr.hostId, derived.token, 1),
    });
    expect(replay.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);
  });

  it("refuses to pair when the two safety numbers disagree", async () => {
    const { host, client } = await connected();
    const honest = new TestDevice();
    const attacker = new TestDevice("Jabree's iPhone");

    const start = await client.startPairing();
    const qr = qrOf(start);
    const honestView = honest.derive(qr, qr.secret);
    const attackerView = attacker.derive(qr, qr.secret);
    // A number that ignored the device's key material would be theatre.
    expect(attackerView.sas).not.toBe(honestView.sas);

    // The attacker saw the QR, so its proof verifies. That is exactly the case
    // the safety number exists for.
    await client.claimPairing({
      pairingId: qr.pairingId,
      secret: qr.secret,
      device: attacker.descriptor(),
      mac: attackerView.claimMac,
    });

    // The human on the host is reading the number off the phone in their hand.
    const refused = await host.call(PAIRING_CONFIRM, {
      pairingId: qr.pairingId,
      side: "host",
      sas: honestView.sas,
      role: "approver",
    });
    expect(refused.error?.code).toBe(RPC_ERROR.PAIRING_FAILED);
    expect((refused.error?.data as { reason: string }).reason).toBe("sas");
    // Nothing was admitted.
    const listed = await client.listDevices();
    expect(listed.devices.filter((d) => !d.local)).toEqual([]);
  });

  it("burns an offer after three wrong credentials", async () => {
    const { host, client } = await connected();
    const phone = new TestDevice();
    const start = await client.startPairing();
    const qr = qrOf(start);

    for (let attempt = 1; attempt <= 3; attempt += 1) {
      const wrong = await host.call(PAIRING_CLAIM, {
        pairingId: qr.pairingId,
        code: "00000000",
        device: phone.descriptor(),
        mac: "00".repeat(32),
      });
      expect(wrong.error?.code).toBe(RPC_ERROR.PAIRING_FAILED);
      expect((wrong.error?.data as { reason: string }).reason).toBe("credential");
    }

    // Spent. Even the real secret off the real screen is now worthless — the
    // answer to a burned offer is the answer to an offer that never existed.
    const derived = phone.derive(qr, qr.secret);
    const late = await host.call(PAIRING_CLAIM, {
      pairingId: qr.pairingId,
      secret: qr.secret,
      device: phone.descriptor(),
      mac: derived.claimMac,
    });
    expect((late.error?.data as { reason: string }).reason).toBe("unknown");
    expect((await client.pairingStatus()).offers).toEqual([]);
  });

  it("pairs from a typed code, and the channel is part of the number", async () => {
    const { client } = await connected();
    const phone = new TestDevice("The NAS in the closet");
    const start = await client.startPairing();
    const qr = qrOf(start);

    const byCode = phone.derive(qr, normalizeCode(start.code), "code");
    const byQr = phone.derive(qr, qr.secret, "qr");
    // A man in the middle downgrading a scan to a typed code changes what both
    // humans see rather than passing quietly.
    expect(byCode.sas).not.toBe(byQr.sas);

    // Typed the way a human says it out loud.
    const spoken = `${start.code.slice(0, 4).toLowerCase()}-${start.code.slice(4)}`;
    const claim = await client.claimPairing({
      pairingId: qr.pairingId,
      code: spoken,
      device: phone.descriptor(),
      mac: byCode.claimMac,
    });
    expect(claim.via).toBe("code");
    expect(claim.hostMac).toBe(byCode.hostMac);
  });

  it("keeps the grant, and the revoke, across a restart of the host", async () => {
    const dataDir = ownDataDir();
    const first = await connected({ dataDir });
    const phone = new TestDevice();
    const { derived } = await pair(first.client, phone, "approver");
    await first.host.stop();

    // A different process, the same SQLite file.
    const second = await connected({ dataDir });
    const afterRestart = (await second.client.listDevices()).devices.find(
      (d) => d.deviceId === phone.deviceId,
    );
    expect(afterRestart).toMatchObject({ role: "approver", sas: derived.sas });
    expect(afterRestart?.revokedAt).toBeUndefined();

    const revoked = await second.client.revokeDevice({ deviceId: phone.deviceId });
    expect(revoked).toMatchObject({ deviceId: phone.deviceId, revoked: true });
    expect(revoked.revokedAt).toBeTruthy();
    // Revoking twice is not an error; the caller's intent already holds.
    expect((await second.client.revokeDevice({ deviceId: phone.deviceId })).revoked).toBe(false);
    await second.host.stop();

    // And a third host, launched cold, still refuses it. This is the half of
    // "revocable" that a restart could quietly undo.
    const third = await connected({ dataDir });
    const tombstoned = (await third.client.listDevices()).devices.find(
      (d) => d.deviceId === phone.deviceId,
    );
    expect(tombstoned?.revokedAt).toBeTruthy();
    const refused = await third.host.call(HOST_HELLO, {
      protocolVersion: 1,
      device: { deviceId: phone.deviceId },
      auth: phone.helloAuth(third.hello.hostId, derived.token, 9),
    });
    expect(refused.error?.code).toBe(RPC_ERROR.UNPAIRED_DEVICE);
  });
});
