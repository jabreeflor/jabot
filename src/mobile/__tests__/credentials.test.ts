/**
 * The production signer, checked against the independent one (#29, #19).
 *
 * `src/mobile/credentials.ts` and `tests/support/pairing.ts` are two
 * implementations of the same documented derivation: WebCrypto and
 * `node:crypto`, written from the protocol docs rather than from each other.
 * That is what makes comparing them worth doing — agreement between two
 * callers of the same function would assert nothing at all.
 *
 * WebCrypto is live in the `unit` (jsdom) project, so this runs offline with
 * no environment change.
 */
import { createHash, createHmac } from "node:crypto";

import { describe, expect, it } from "vitest";

import {
  createDeviceCredentials,
  frameHash,
  helloProof,
  verifyHostProof,
} from "../credentials";
import { TestDevice } from "../../../tests/support/pairing";

const hex = (bytes: Uint8Array) =>
  [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");

describe("frameHash", () => {
  it("is SHA-256 over 4-byte-big-endian length-framed fields", async () => {
    // Spelled out by hand rather than by the other implementation, so this
    // pins the wire format itself and not just the two copies' agreement.
    const framed = Buffer.concat([
      Buffer.from([0, 0, 0, 2]),
      Buffer.from("ab"),
      Buffer.from([0, 0, 0, 1]),
      Buffer.from("c"),
    ]);
    const want = createHash("sha256").update(framed).digest("hex");

    expect(hex(await frameHash(["ab", "c"]))).toBe(want);
  });

  /** The reason for the framing. Without it these two hash the same, and a
      transcript that can be re-cut is one an attacker gets to choose. */
  it("keeps a field from absorbing its neighbour's characters", async () => {
    expect(hex(await frameHash(["ab", "c"]))).not.toBe(
      hex(await frameHash(["a", "bc"])),
    );
    expect(hex(await frameHash([""]))).not.toBe(hex(await frameHash([])));
  });

  it("frames multi-byte characters by their bytes, not their length", async () => {
    // "é" is two UTF-8 bytes. A length written in characters would let a
    // Node and a browser implementation disagree on exactly the inputs a
    // device name makes likely.
    const framed = Buffer.concat([
      Buffer.from([0, 0, 0, 2]),
      Buffer.from("é", "utf8"),
    ]);
    expect(hex(await frameHash(["é"]))).toBe(
      createHash("sha256").update(framed).digest("hex"),
    );
  });
});

describe("the hello proof", () => {
  /** Deterministic inputs, chosen to move every field of the transcript. */
  const table = [
    { token: "dG9rZW4tb25l", hostId: "host-a", counter: 1, protocolVersion: 1 },
    { token: "dG9rZW4tdHdv", hostId: "host-a", counter: 2, protocolVersion: 1 },
    { token: "dG9rZW4tb25l", hostId: "host-b", counter: 2, protocolVersion: 1 },
    { token: "dG9rZW4tb25l", hostId: "host-a", counter: 999_999, protocolVersion: 2 },
    { token: "a-token-with-🔑-in-it", hostId: "hôst", counter: 7, protocolVersion: 1 },
  ];

  it("is byte-identical to the independent Node implementation", async () => {
    for (const row of table) {
      const device = new TestDevice(`phone ${row.hostId}`);
      const theirs = device.helloAuth(
        row.hostId,
        row.token,
        row.counter,
        row.protocolVersion,
      );
      const ours = await helloProof({
        token: row.token,
        hostId: row.hostId,
        deviceId: device.deviceId,
        counter: row.counter,
        protocolVersion: row.protocolVersion,
      });
      expect(ours).toEqual(theirs);
    }
  });

  /**
   * The mistake this module's docs warn about, asserted rather than described.
   * The token is a base64url *string* and the HMAC key is its UTF-8 bytes —
   * the host does `token.as_bytes()`. Decoding it first produces a MAC that is
   * wrong everywhere, so a test that only compared our own two calls would
   * pass while nothing could connect.
   */
  it("keys on the token's characters, not its decoded bytes", async () => {
    const token = "dG9rZW4tb25l";
    const decoded = Buffer.from(token, "base64url").toString("binary");
    const asText = await helloProof({
      token,
      hostId: "host-a",
      deviceId: "dev-1",
      counter: 1,
    });
    const asBytes = await helloProof({
      token: decoded,
      hostId: "host-a",
      deviceId: "dev-1",
      counter: 1,
    });
    expect(asText.mac).not.toBe(asBytes.mac);
    // And it is the *first* one the other implementation produces.
    const device = new TestDevice("phone");
    expect(device.helloAuth("host-a", token, 1).mac).toBe(
      (
        await helloProof({
          token,
          hostId: "host-a",
          deviceId: device.deviceId,
          counter: 1,
        })
      ).mac,
    );
  });
});

/** The host separator, computed with `node:crypto` so this is not the module
    checking its own arithmetic. Kept here rather than in
    `tests/support/pairing.ts` because that file is the *device*'s reference
    implementation and this is the host's half. */
function hostMacFor(
  deviceId: string,
  token: string,
  hostId: string,
  counter: number,
  protocolVersion = 1,
): string {
  const fields = [
    "jabot/hello/host/v1",
    hostId,
    deviceId,
    String(protocolVersion),
    String(counter),
  ];
  const parts: Buffer[] = [];
  for (const field of fields) {
    const bytes = Buffer.from(field, "utf8");
    const len = Buffer.alloc(4);
    len.writeUInt32BE(bytes.length);
    parts.push(len, bytes);
  }
  const digest = createHash("sha256").update(Buffer.concat(parts)).digest();
  return createHmac("sha256", token).update(digest).digest("hex");
}

function flip(mac: string): string {
  const last = mac.slice(-1);
  return `${mac.slice(0, -1)}${last === "0" ? "1" : "0"}`;
}

describe("the host's answering proof", () => {
  const token = "dG9rZW4tb25l";
  const hostId = "host-a";
  const deviceId = "dev-1";
  const input = { token, hostId, deviceId, counter: 3 };
  const hostAuth = { counter: 3, mac: hostMacFor(deviceId, token, hostId, 3) };

  it("accepts what the host derives and refuses everything else", async () => {
    expect(await verifyHostProof(hostAuth, input)).toBe(true);
    expect(
      await verifyHostProof({ ...hostAuth, mac: flip(hostAuth.mac) }, input),
    ).toBe(false);
    expect(await verifyHostProof(hostAuth, { ...input, token: "other" })).toBe(false);
    // A different Mac is a different transcript, which is the property a
    // phone is actually checking: the *same* host as last time.
    expect(await verifyHostProof(hostAuth, { ...input, hostId: "host-b" })).toBe(false);
    expect(await verifyHostProof(hostAuth, { ...input, deviceId: "dev-2" })).toBe(false);
    expect(await verifyHostProof({ ...hostAuth, counter: 4 }, input)).toBe(false);
  });

  /** The case a client is most likely to get wrong: a host that answered with
      nothing looks exactly like a field somebody stripped. */
  it("refuses an absent proof rather than shrugging", async () => {
    expect(await verifyHostProof(undefined, input)).toBe(false);
  });

  /** Domain separation. The token is shared, so a host answering under the
      device's separator would hand back a value the device could compute
      itself — and one replayable as a device proof. */
  it("is not the device's own proof", async () => {
    const mine = await helloProof({ token, hostId, deviceId, counter: 3 });
    expect(mine.mac).not.toBe(hostAuth.mac);
    expect(await verifyHostProof({ counter: 3, mac: mine.mac }, input)).toBe(false);
  });
});

describe("createDeviceCredentials", () => {
  const base = {
    deviceId: "dev-1",
    name: "Jabree's iPhone",
    hostId: "host-a",
    token: () => "dG9rZW4tb25l",
  };

  it("signs with the counter storage handed it", async () => {
    let counter = 0;
    const credentials = createDeviceCredentials({
      ...base,
      nextCounter: () => ++counter,
    });

    const first = await credentials.signHello();
    const second = await credentials.signHello();
    expect(first.counter).toBe(1);
    expect(second.counter).toBe(2);
    expect(first.mac).toEqual(
      (
        await helloProof({
          token: "dG9rZW4tb25l",
          hostId: "host-a",
          deviceId: "dev-1",
          counter: 1,
        })
      ).mac,
    );
    // Same key, different counter, different proof — which is the replay
    // guard doing its job on this side of the wire.
    expect(second.mac).not.toBe(first.mac);
  });

  /**
   * A backstop, and honest about being one: it only sees the counters this
   * process issued, and the counter that matters is the one that survived a
   * restart. What it catches is a `nextCounter` that returns a constant —
   * otherwise that ships as an intermittent, unexplained `UnpairedDevice`.
   */
  it("refuses a counter that does not climb", async () => {
    const credentials = createDeviceCredentials({ ...base, nextCounter: () => 5 });
    await credentials.signHello();
    await expect(credentials.signHello()).rejects.toThrow(/must climb/);
  });

  it("refuses a counter that is not a positive integer", async () => {
    for (const bad of [0, -1, 1.5, Number.NaN]) {
      const credentials = createDeviceCredentials({ ...base, nextCounter: () => bad });
      await expect(credentials.signHello()).rejects.toThrow(/positive integer/);
    }
  });

  it("reads the token per hello rather than holding it", async () => {
    // An enclave that only answers while the device is unlocked is the reason
    // this is a callback and not a string.
    let reads = 0;
    let counter = 0;
    const credentials = createDeviceCredentials({
      ...base,
      token: async () => {
        reads += 1;
        return "dG9rZW4tb25l";
      },
      nextCounter: () => ++counter,
    });

    await credentials.signHello();
    await credentials.signHello();
    expect(reads).toBe(2);
  });
});
