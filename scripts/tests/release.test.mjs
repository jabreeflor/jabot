// Tests for the push-to-main release: automatic versions, the pre-publish
// feed check, and the pins that keep Local and Dev apart.
//
//   node --test scripts/tests/release.test.mjs
//
// The signature fixture under fixtures/release/ was produced by the real
// `tauri signer sign` with a throwaway key (its private half was never
// committed), so the verifier is checked against what the bundler actually
// writes rather than against a format this file invented.

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { compare, nextVersion, parse, stampJson } from "../release-version.mjs";
import {
  parsePublicKey,
  verifyFeed,
  verifySignature,
} from "../release-verify-feed.mjs";

const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const FIX = path.join(ROOT, "scripts/tests/fixtures/release");
const read = (p) => fs.readFileSync(path.join(ROOT, p), "utf8");

const archive = fs.readFileSync(path.join(FIX, "JaBot.app.tar.gz"));
const signature = fs.readFileSync(
  path.join(FIX, "JaBot.app.tar.gz.sig"),
  "utf8",
);
const fixturePub = fs.readFileSync(path.join(FIX, "fixture.key.pub"), "utf8");

// ---------------------------------------------------------------------------
// versions

test("parse accepts X.Y.Z only", () => {
  assert.deepEqual(parse("0.1.12"), [0, 1, 12]);
  assert.equal(parse("v0.1.0"), null);
  assert.equal(parse("0.1"), null);
  assert.equal(parse("0.1.0-beta.1"), null);
  assert.equal(parse("01.2.3"), null);
  assert.equal(compare([0, 10, 0], [0, 9, 99]), 1);
});

test("the first release ships the floor itself", () => {
  assert.equal(nextVersion("0.1.0", []), "0.1.0");
});

test("every later push is the next patch above the highest tag", () => {
  assert.equal(nextVersion("0.1.0", ["v0.1.0"]), "0.1.1");
  assert.equal(
    nextVersion("0.1.0", ["v0.1.0", "v0.1.9", "v0.1.10", "v0.1.2"]),
    "0.1.11",
  );
});

test("raising the floor starts a new line", () => {
  assert.equal(nextVersion("0.2.0", ["v0.1.0", "v0.1.41"]), "0.2.0");
  assert.equal(nextVersion("0.2.0", ["v0.2.0"]), "0.2.1");
  assert.equal(nextVersion("1.0.0", ["v0.9.3"]), "1.0.0");
});

test("a floor below the tags never pulls the number backwards", () => {
  assert.equal(nextVersion("0.1.0", ["v0.3.4"]), "0.3.5");
});

test("tags that are not vX.Y.Z are ignored", () => {
  assert.equal(
    nextVersion("0.1.0", [
      "v0.1.3",
      "v9.9.9-rc.1",
      "nightly",
      "release-5",
      "vX",
    ]),
    "0.1.4",
  );
});

test("a malformed floor is refused, not guessed at", () => {
  assert.throws(() => nextVersion("0.1", []), /not X\.Y\.Z/);
});

test("stamp rewrites only the top-level version", () => {
  const text =
    '{\n  "name": "x",\n  "version": "0.1.0",\n  "dep": {\n    "version": "9.9.9"\n  }\n}\n';
  const out = stampJson(text, "0.1.7");
  assert.equal(JSON.parse(out).version, "0.1.7");
  assert.equal(JSON.parse(out).dep.version, "9.9.9");
  assert.equal(out.length, text.length);
  assert.throws(() => stampJson('{"a": 1}', "0.1.7"), /no top-level/);
});

test("stamp works on the real tauri.conf.json and package.json", () => {
  for (const f of ["src-tauri/tauri.conf.json", "package.json"]) {
    assert.equal(JSON.parse(stampJson(read(f), "7.8.9")).version, "7.8.9", f);
  }
});

// ---------------------------------------------------------------------------
// signatures

test("a real tauri signature verifies against its pubkey", () => {
  assert.match(
    verifySignature(archive, signature, fixturePub),
    /file:JaBot\.app\.tar\.gz/,
  );
});

test("a tampered archive is refused", () => {
  const tampered = Buffer.concat([archive, Buffer.from("x")]);
  assert.throws(
    () => verifySignature(tampered, signature, fixturePub),
    /does not verify/,
  );
});

test("a signature from the wrong key names both key ids", () => {
  const appPub = JSON.parse(read("src-tauri/tauri.conf.json")).plugins.updater
    .pubkey;
  assert.throws(
    () => verifySignature(archive, signature, appPub),
    /32FEEB500BD298FB.*593EC393462762A0/,
  );
});

test("a forged trusted comment is refused", () => {
  const text = Buffer.from(signature, "base64").toString("utf8");
  const forged = text.replace(/file:JaBot\.app\.tar\.gz/, "file:evil.tar.gz");
  const b64 = Buffer.from(forged, "utf8").toString("base64");
  assert.throws(
    () => verifySignature(archive, b64, fixturePub),
    /trusted comment/,
  );
});

test("the non-prehashed Ed mode verifies too", () => {
  const { publicKey, privateKey } = crypto.generateKeyPairSync("ed25519");
  const raw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  const keyId = crypto.randomBytes(8);
  const pub = Buffer.concat([Buffer.from("Ed"), keyId, raw]).toString("base64");
  const pubFile = Buffer.from(`untrusted comment: k\n${pub}\n`).toString(
    "base64",
  );
  const body = crypto.sign(null, archive, privateKey);
  const trusted = "timestamp:1";
  const global = crypto.sign(
    null,
    Buffer.concat([body, Buffer.from(trusted)]),
    privateKey,
  );
  const sig = Buffer.concat([Buffer.from("Ed"), keyId, body]).toString(
    "base64",
  );
  const sigFile = Buffer.from(
    `untrusted comment: s\n${sig}\ntrusted comment: ${trusted}\n${global.toString("base64")}\n`,
  ).toString("base64");
  assert.equal(verifySignature(archive, sigFile, pubFile), trusted);
});

// ---------------------------------------------------------------------------
// the feed as a whole

const goodFeed = (version = "0.1.7") => ({
  version,
  platforms: Object.fromEntries(
    ["darwin-aarch64", "darwin-x86_64"].map((p) => [
      p,
      {
        url: `https://github.com/jabreeflor/jabot/releases/download/v${version}/JaBot.app.tar.gz`,
        signature,
      },
    ]),
  ),
});
const assets = (name) => (name === "JaBot.app.tar.gz" ? archive : null);

test("a complete, correctly signed feed passes", () => {
  assert.deepEqual(
    verifyFeed(goodFeed(), {
      version: "0.1.7",
      pubkey: fixturePub,
      read: assets,
    }),
    [],
  );
});

test("the feed is refused when its version is not the stamped one", () => {
  const errs = verifyFeed(goodFeed(), {
    version: "0.1.8",
    pubkey: fixturePub,
    read: assets,
  });
  assert.ok(errs.some((e) => /expected 0\.1\.8/.test(e)));
});

test("a missing architecture is refused", () => {
  const feed = goodFeed();
  delete feed.platforms["darwin-x86_64"];
  const errs = verifyFeed(feed, {
    version: "0.1.7",
    pubkey: fixturePub,
    read: assets,
  });
  assert.deepEqual(errs, [
    "latest.json has no darwin-x86_64 entry — those installs would never update",
  ]);
});

test("an archive that is not among the assets is refused", () => {
  const errs = verifyFeed(goodFeed(), {
    version: "0.1.7",
    pubkey: fixturePub,
    read: () => null,
  });
  assert.equal(errs.length, 2);
});

// ---------------------------------------------------------------------------
// pins between files

const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const local = JSON.parse(read("src-tauri/tauri.local.conf.json"));

test("the shipped pubkey is a real minisign key, not the placeholder", () => {
  assert.doesNotThrow(() => parsePublicKey(conf.plugins.updater.pubkey));
});

test("Local is a different app from Dev", () => {
  assert.notEqual(local.identifier, conf.identifier);
  assert.notEqual(local.productName, conf.productName);
  assert.ok(local.identifier.startsWith(`${conf.identifier}.`));
});

test("the updater only runs in the app whose identifier ships", () => {
  const rs = read("src-tauri/src/updates.rs");
  const m = /pub const RELEASE_IDENTIFIER: &str = "([^"]+)";/.exec(rs);
  assert.ok(m, "updates.rs no longer declares RELEASE_IDENTIFIER");
  assert.equal(m[1], conf.identifier);
});

test("release.yml ships on main and verifies the feed before publishing", () => {
  const wf = read(".github/workflows/release.yml");
  assert.match(wf, /push:\s*\n\s*branches:\s*\[main\]/);
  assert.match(wf, /release-version\.mjs next/);
  assert.match(wf, /release-version\.mjs stamp/);
  assert.match(wf, /release-verify-feed\.mjs/);
  const publish = wf.slice(wf.search(/^ {2}publish:/m));
  assert.match(publish, /needs:\s*\[version, macos\]/);
  assert.match(publish, /--draft=false/);
  assert.ok(
    publish.indexOf("release-verify-feed.mjs") <
      publish.indexOf("--draft=false"),
    "publish must verify the feed before it flips the draft",
  );
  assert.doesNotMatch(
    wf,
    /^\s+APPLE_[A-Z_]+:\s*\$\{\{/m,
    "no Apple secrets: releases are ad-hoc signed",
  );
});
