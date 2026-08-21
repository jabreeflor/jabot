/**
 * The *device* half of the pairing handshake, implemented from the protocol
 * docs alone (`src/host/protocol.ts`, `PairingClaimParams`).
 *
 * This is deliberately a second, independent implementation rather than a
 * call into anything the host shares. The claim #19 makes is that two programs
 * holding only the out-of-band secret arrive at the *same* safety number; a
 * test that asked the host to check its own arithmetic would assert nothing
 * about that, and would still pass if both ends were wrong in the same way.
 *
 * It is also the reference a real phone client is written against — everything
 * here is `node:crypto` plus the framing rule, no JaBot code.
 */
import { createHash, createHmac, randomBytes } from "node:crypto";

import type { PairingQr } from "../../src/host/protocol";

const TRANSCRIPT_DOMAIN = "jabot/pairing/v1";
const CLAIM_DOMAIN = "jabot/pairing/claim/v1";
const HOST_DOMAIN = "jabot/pairing/host/v1";
const CONFIRM_DOMAIN = "jabot/pairing/confirm/v1";
const SAS_DOMAIN = "jabot/pairing/sas/v1";
const TOKEN_DOMAIN = "jabot/pairing/device-token/v1";
const HELLO_DOMAIN = "jabot/hello/v1";
const DEVICE_FINGERPRINT_DOMAIN = "jabot/device-fingerprint/v1";

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** SHA-256 over length-framed fields: 4-byte big-endian length, then bytes.
    The framing is what stops one field absorbing another's characters. */
function frameHash(fields: string[]): Buffer {
  const parts: Buffer[] = [];
  for (const field of fields) {
    const bytes = Buffer.from(field, "utf8");
    const len = Buffer.alloc(4);
    len.writeUInt32BE(bytes.length);
    parts.push(len, bytes);
  }
  return createHash("sha256").update(Buffer.concat(parts)).digest();
}

function fingerprint(domain: string, keyMaterial: string): string {
  return frameHash([domain, keyMaterial]).toString("base64url");
}

/** Eight decimal digits of a MAC, grouped: the safety number. */
function sasDigits(mac: Buffer): string {
  const value = mac.subarray(0, 8).readBigUInt64BE();
  const digits = Number(value % 100_000_000n);
  const text = digits.toString().padStart(8, "0");
  return `${text.slice(0, 4)}-${text.slice(4)}`;
}

/** How a human types a code back: case, spaces and dashes are noise, and
    `I`/`L`/`O` fold onto `1`/`1`/`0`. */
export function normalizeCode(input: string): string {
  let out = "";
  for (const raw of input) {
    if (raw === "-" || raw === " ") continue;
    const upper = raw.toUpperCase();
    const mapped = upper === "I" || upper === "L" ? "1" : upper === "O" ? "0" : upper;
    if (!CROCKFORD.includes(mapped)) throw new Error(`not a pairing code: ${input}`);
    out += mapped;
  }
  return out;
}

export type Channel = "qr" | "code";

/** Everything the device derives once it has scanned or typed. */
export interface Derived {
  transcript: string;
  claimMac: string;
  hostMac: string;
  confirmMac: string;
  /** The number to put on the phone's screen. Derived here, never taken from
      the host's answer — that is the whole point. */
  sas: string;
  /** The shared token, derived on both sides and never transmitted. */
  token: string;
}

/** A phone that has never met this host. */
export class TestDevice {
  readonly deviceId: string;
  readonly name: string;
  readonly nonce: string;
  private readonly keyMaterial: string;

  constructor(name = "Jabree's iPhone") {
    this.deviceId = `dev-${randomBytes(8).toString("hex")}`;
    this.name = name;
    this.nonce = randomBytes(32).toString("base64url");
    this.keyMaterial = randomBytes(32).toString("base64url");
  }

  /** What goes on the wire: a commitment, never the key material itself. */
  get fingerprint(): string {
    return fingerprint(DEVICE_FINGERPRINT_DOMAIN, this.keyMaterial);
  }

  descriptor() {
    return {
      deviceId: this.deviceId,
      name: this.name,
      fingerprint: this.fingerprint,
      nonce: this.nonce,
    };
  }

  /**
   * Derive the whole handshake from a scanned QR (or a typed code) plus this
   * device's own key material.
   *
   * `key` is the out-of-band credential: the QR's `secret`, or the normalized
   * code. Which one was used is part of the transcript, so a downgrade from
   * scan to typed code changes the safety number instead of passing quietly.
   */
  derive(qr: PairingQr, key: string, via: Channel = "qr"): Derived {
    const transcript = frameHash([
      TRANSCRIPT_DOMAIN,
      qr.hostId,
      qr.hostFingerprint,
      qr.hostNonce,
      qr.pairingId,
      this.deviceId,
      this.fingerprint,
      this.nonce,
      via,
    ]).toString("hex");
    const bind = (domain: string) =>
      createHmac("sha256", key).update(frameHash([domain, transcript])).digest();
    return {
      transcript,
      claimMac: bind(CLAIM_DOMAIN).toString("hex"),
      hostMac: bind(HOST_DOMAIN).toString("hex"),
      confirmMac: bind(CONFIRM_DOMAIN).toString("hex"),
      sas: sasDigits(bind(SAS_DOMAIN)),
      token: bind(TOKEN_DOMAIN).toString("base64url"),
    };
  }

  /** The `host/hello` proof. `counter` must climb: the host refuses one it
      has already seen, which is what makes a captured frame useless. */
  helloAuth(hostId: string, token: string, counter: number, protocolVersion = 1) {
    return {
      counter,
      mac: createHmac("sha256", token)
        .update(
          frameHash([
            HELLO_DOMAIN,
            hostId,
            this.deviceId,
            String(protocolVersion),
            String(counter),
          ]),
        )
        .digest("hex"),
    };
  }
}
