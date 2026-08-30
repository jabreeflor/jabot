//! The device half of `host/hello`, in WebCrypto — the one a real app can run.
//!
//! `MobileSession` takes a [`DeviceCredentials`] and never sees the token, and
//! that is still right: which enclave, keystore or file the token lives in is
//! a device question. But until now nothing under `src/` could produce the
//! proof at all. The only working implementation was `tests/support/pairing.ts`,
//! which is `node:crypto` and `Buffer` — unusable from a browser, a webview or
//! a React Native shell, which is every device this client is for.
//!
//! So this is the production signer, and `tests/support/pairing.ts` stays
//! exactly what it was: a second, independent implementation written from the
//! protocol docs, which is what makes a byte-for-byte comparison between them
//! mean something. The unit tests assert equality across a table of inputs;
//! agreement between two implementations that shared code would assert
//! nothing.
//!
//! **The one thing that is easy to get wrong.** The token is a base64url
//! string, and the HMAC key is the **UTF-8 bytes of that string**, not its 32
//! decoded bytes. That is what the host does (`token.as_bytes()`) and what the
//! Node implementation does (`createHmac("sha256", token)`), so decoding it
//! first produces a MAC that is wrong everywhere and looks right nowhere.
//!
//! Storage and the counter stay injected. The counter in particular has to
//! survive the app being killed — the host refuses one it has already accepted
//! — and no module that is reconstructed on launch can promise that.

import type { DeviceAuth, HostAuth } from "../host/protocol";
import type { DeviceCredentials } from "./session";

/** The device proving itself. */
const HELLO_DOMAIN = "jabot/hello/v1";
/** The host proving itself back (#19's mutual half). */
const HOST_HELLO_DOMAIN = "jabot/hello/host/v1";

const encoder = new TextEncoder();

/**
 * SHA-256 over length-framed fields: 4-byte big-endian length, then bytes.
 *
 * The framing is the whole reason two adjacent fields cannot be re-cut. Without
 * it `["ab", "c"]` and `["a", "bc"]` hash the same, and a transcript that can
 * be re-cut is a transcript an attacker chooses.
 */
export async function frameHash(fields: readonly string[]): Promise<Uint8Array> {
  let total = 0;
  const parts = fields.map((field) => {
    const bytes = encoder.encode(field);
    total += 4 + bytes.length;
    return bytes;
  });
  const buffer = new Uint8Array(total);
  const view = new DataView(buffer.buffer);
  let at = 0;
  for (const bytes of parts) {
    view.setUint32(at, bytes.length, false);
    at += 4;
    buffer.set(bytes, at);
    at += bytes.length;
  }
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return new Uint8Array(digest);
}

async function hmacKey(token: string): Promise<CryptoKey> {
  // `encode`, not a base64url decode. See the module docs: the key is the
  // bytes of the token *string*.
  return crypto.subtle.importKey(
    "raw",
    encoder.encode(token),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
}

function hex(bytes: Uint8Array): string {
  let out = "";
  for (const byte of bytes) out += byte.toString(16).padStart(2, "0");
  return out;
}

async function mac(
  token: string,
  fields: readonly string[],
): Promise<string> {
  const key = await hmacKey(token);
  const signature = await crypto.subtle.sign("HMAC", key, await frameHash(fields));
  return hex(new Uint8Array(signature));
}

export interface HelloProofInput {
  token: string;
  hostId: string;
  deviceId: string;
  counter: number;
  protocolVersion?: number;
}

/**
 * `mac = HMAC-SHA256(token, H["jabot/hello/v1", hostId, deviceId,
 * protocolVersion, counter])`, hex — the proof `host/hello` carries.
 */
export async function helloProof(input: HelloProofInput): Promise<DeviceAuth> {
  const { token, hostId, deviceId, counter, protocolVersion = 1 } = input;
  return {
    counter,
    mac: await mac(token, [
      HELLO_DOMAIN,
      hostId,
      deviceId,
      String(protocolVersion),
      String(counter),
    ]),
  };
}

/**
 * Check the host's answering proof — the mirror of [`helloProof`], under its
 * own separator so the two are not interchangeable.
 *
 * `false` for an **absent** `hostAuth`, which is the case that matters: a
 * stripped field is what a host that stopped answering looks like on a wire,
 * and a client that read absence as consent would have made the exchange
 * optional. It is also what a host older than the field returns, so a device
 * that requires the guarantee has to refuse rather than shrug.
 *
 * Not constant-time. There is nothing to protect: this compares a value the
 * caller already holds against one it was just handed, and a device that
 * leaked the timing of its own check would be leaking to itself.
 */
export async function verifyHostProof(
  hostAuth: HostAuth | undefined,
  input: HelloProofInput,
): Promise<boolean> {
  if (!hostAuth) return false;
  if (hostAuth.counter !== input.counter) return false;
  const expected = await mac(input.token, [
    HOST_HELLO_DOMAIN,
    input.hostId,
    input.deviceId,
    String(input.protocolVersion ?? 1),
    String(input.counter),
  ]);
  return hostAuth.mac === expected;
}

export interface DeviceCredentialsOptions {
  deviceId: string;
  name?: string;
  /** The host this device paired with. Part of the transcript, so a proof made
      for one Mac is not a proof for another. */
  hostId: string;
  /** Read from wherever the token actually lives. Called per hello rather than
      held, so an app can keep it in an enclave that only answers while the
      device is unlocked. */
  token: () => string | Promise<string>;
  /** The next counter, from storage that survives the app being killed. It
      must strictly climb: the host refuses one it has already accepted, which
      is what makes a captured frame worthless. */
  nextCounter: () => number | Promise<number>;
  protocolVersion?: number;
}

/**
 * A [`DeviceCredentials`] a real app can hand `MobileSession`.
 *
 * The monotonic check here is a **backstop, not the guarantee**. It can only
 * see the counters this process issued, and the counter that matters is the
 * one that survived a restart — that is the caller's job. What it does catch
 * is a `nextCounter` that returns a constant, which is the shape of the bug
 * that would otherwise look like an intermittent, unexplained `UnpairedDevice`
 * from the host.
 */
export function createDeviceCredentials(
  options: DeviceCredentialsOptions,
): DeviceCredentials {
  let last: number | null = null;
  return {
    deviceId: options.deviceId,
    name: options.name,
    async signHello(): Promise<DeviceAuth> {
      const counter = await options.nextCounter();
      if (!Number.isSafeInteger(counter) || counter <= 0) {
        throw new Error(`hello counter must be a positive integer, not ${counter}`);
      }
      if (last !== null && counter <= last) {
        throw new Error(
          `hello counter must climb: ${counter} does not follow ${last}`,
        );
      }
      last = counter;
      return helloProof({
        token: await options.token(),
        hostId: options.hostId,
        deviceId: options.deviceId,
        counter,
        protocolVersion: options.protocolVersion,
      });
    },
  };
}
