#!/usr/bin/env node
// Check a release's update feed before it is published.
//
//   node scripts/release-verify-feed.mjs <dir> <version>
//
// <dir> holds the draft's latest.json and the .app.tar.gz it points at
// (`gh release download`). Publishing a release is what makes every installed
// copy fetch it, so this is the last gate, and it answers the one question a
// green build cannot: will an installed JaBot actually *accept* this update?
//
// That depends on three things the bundler never cross-checks:
//   * latest.json's version is the version this run stamped (a mismatch is a
//     feed installed copies either ignore or loop on);
//   * both darwin-aarch64 and darwin-x86_64 are present (the updater looks up
//     its own target and has no universal fallback);
//   * the signature verifies against the pubkey compiled into the app
//     (src-tauri/tauri.conf.json). A TAURI_SIGNING_PRIVATE_KEY secret that is
//     not the pair of that pubkey signs every archive happily, and every
//     installed copy then refuses every update, silently, forever.
//
// The signature is minisign (what tauri's signer writes), verified with
// node:crypto alone so this runs on a bare ubuntu runner with no installs.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const REQUIRED_PLATFORMS = ["darwin-aarch64", "darwin-x86_64"];

/** Tauri stores both key and signature as base64 of the minisign text file. */
function minisignLines(b64, what) {
  const text = Buffer.from(String(b64).trim(), "base64").toString("utf8");
  const lines = text.split("\n").map((l) => l.replace(/\r$/, ""));
  if (!lines[0]?.startsWith("untrusted comment:")) {
    throw new Error(
      `${what} is not a base64 minisign file (no "untrusted comment:" line)`,
    );
  }
  return lines;
}

export function parsePublicKey(pubkeyB64) {
  const lines = minisignLines(pubkeyB64, "pubkey");
  const raw = Buffer.from(lines[1] ?? "", "base64");
  if (raw.length !== 42 || raw.subarray(0, 2).toString("latin1") !== "Ed") {
    throw new Error("pubkey is not a minisign Ed25519 key");
  }
  return { keyId: raw.subarray(2, 10), key: raw.subarray(10, 42) };
}

function ed25519(rawKey) {
  return crypto.createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: rawKey.toString("base64url") },
    format: "jwk",
  });
}

const hexId = (buf) => Buffer.from(buf).reverse().toString("hex").toUpperCase();

/**
 * Verify `data` against a tauri/minisign signature. Throws with a reason on
 * any mismatch; returns the trusted comment on success.
 */
export function verifySignature(data, signatureB64, pubkeyB64) {
  const pub = parsePublicKey(pubkeyB64);
  const lines = minisignLines(signatureB64, "signature");
  const sig = Buffer.from(lines[1] ?? "", "base64");
  if (sig.length !== 74)
    throw new Error("signature is not a minisign Ed25519 signature");
  const alg = sig.subarray(0, 2).toString("latin1");
  if (alg !== "Ed" && alg !== "ED")
    throw new Error(`unknown signature algorithm ${JSON.stringify(alg)}`);
  const keyId = sig.subarray(2, 10);
  if (!keyId.equals(pub.keyId)) {
    throw new Error(
      `signed by key ${hexId(keyId)}, but the app trusts key ${hexId(pub.keyId)} — ` +
        "TAURI_SIGNING_PRIVATE_KEY is not the pair of plugins.updater.pubkey",
    );
  }
  const key = ed25519(pub.key);
  // "ED" is minisign's prehashed mode: the signature covers BLAKE2b-512(data).
  const message =
    alg === "ED" ? crypto.createHash("blake2b512").update(data).digest() : data;
  if (!crypto.verify(null, message, key, sig.subarray(10))) {
    throw new Error(
      "signature does not verify — the archive is not the one that was signed",
    );
  }
  const trustedLine = lines[2] ?? "";
  if (!trustedLine.startsWith("trusted comment: "))
    throw new Error("signature has no trusted comment");
  const trusted = trustedLine.slice("trusted comment: ".length);
  const globalSig = Buffer.from(lines[3] ?? "", "base64");
  const signedComment = Buffer.concat([
    sig.subarray(10),
    Buffer.from(trusted, "utf8"),
  ]);
  if (
    globalSig.length !== 64 ||
    !crypto.verify(null, signedComment, key, globalSig)
  ) {
    throw new Error("the trusted comment's signature does not verify");
  }
  return trusted;
}

/**
 * Check a parsed latest.json against the expected version and the files in
 * `dir`. `read(name)` returns the bytes of a downloaded asset or null.
 */
export function verifyFeed(feed, { version, pubkey, read }) {
  const errors = [];
  const got = String(feed.version ?? "").replace(/^v/, "");
  if (got !== version)
    errors.push(
      `latest.json says version ${JSON.stringify(feed.version)}, expected ${version}`,
    );
  const platforms = feed.platforms ?? {};
  for (const name of REQUIRED_PLATFORMS) {
    const entry = platforms[name];
    if (!entry) {
      errors.push(
        `latest.json has no ${name} entry — those installs would never update`,
      );
      continue;
    }
    const file = decodeURIComponent(
      String(entry.url ?? "")
        .split("/")
        .pop() ?? "",
    );
    if (!String(entry.url ?? "").includes(`/releases/download/v${version}/`)) {
      errors.push(`${name}: url ${entry.url} is not an asset of v${version}`);
    }
    const bytes = file ? read(file) : null;
    if (!bytes) {
      errors.push(
        `${name}: ${file || "(no url)"} is not among the release's assets`,
      );
      continue;
    }
    try {
      verifySignature(bytes, entry.signature ?? "", pubkey);
    } catch (e) {
      errors.push(`${name}: ${e.message}`);
    }
  }
  return errors;
}

function main(argv) {
  const [dir, version] = argv;
  if (!dir || !version) {
    process.stderr.write("usage: release-verify-feed.mjs <dir> <X.Y.Z>\n");
    return 2;
  }
  const conf = JSON.parse(
    fs.readFileSync(path.join(ROOT, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const pubkey = conf.plugins?.updater?.pubkey ?? "";
  const feedPath = path.join(dir, "latest.json");
  if (!fs.existsSync(feedPath)) {
    process.stderr.write(
      `${feedPath} is missing — the macOS job did not upload a feed\n`,
    );
    return 1;
  }
  const feed = JSON.parse(fs.readFileSync(feedPath, "utf8"));
  const read = (name) => {
    const p = path.join(dir, name);
    return fs.existsSync(p) ? fs.readFileSync(p) : null;
  };
  const errors = verifyFeed(feed, { version, pubkey, read });
  if (errors.length) {
    for (const e of errors) process.stderr.write(`::error::${e}\n`);
    return 1;
  }
  process.stdout.write(
    `latest.json ${version}: ${REQUIRED_PLATFORMS.join(", ")} signed by the key the app trusts\n`,
  );
  return 0;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exit(main(process.argv.slice(2)));
}
