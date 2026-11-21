#!/usr/bin/env node
// Automatic release versions for the push-to-main release (.github/workflows/release.yml).
//
//   node scripts/release-version.mjs next            # prints version=0.1.7 and tag=v0.1.7
//   node scripts/release-version.mjs stamp 0.1.7     # writes it into tauri.conf.json + package.json
//
// The version in src-tauri/tauri.conf.json is a *floor*, not the release
// number. Every push to main ships the next patch above the highest `v*` tag;
// to start a new minor or major, raise the floor (say to 0.2.0) in a PR and
// the next release is exactly that. Nothing is committed back to main — the
// stamp happens inside the CI checkout, and the tag is the record.
//
// The updater only installs a version that is semver-greater than the one
// running, so "strictly increasing, never reused" is the whole contract.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CONF = path.join(ROOT, "src-tauri/tauri.conf.json");
const PKG = path.join(ROOT, "package.json");

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** "1.2.3" -> [1, 2, 3], or null for anything that is not plain X.Y.Z. */
export function parse(version) {
  const m = SEMVER.exec(String(version).trim());
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function compare(a, b) {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  return 0;
}

/**
 * The version the next release ships as.
 *
 * `floor` is tauri.conf.json's version; `tags` is every tag in the repo.
 * Tags that are not `vX.Y.Z` (pre-releases, other tooling) are ignored, so
 * they can neither block a release nor pull the number backwards.
 */
export function nextVersion(floor, tags) {
  const base = parse(floor);
  if (!base)
    throw new Error(`floor version ${JSON.stringify(floor)} is not X.Y.Z`);
  let highest = null;
  for (const tag of tags) {
    const t = String(tag).trim();
    if (!t.startsWith("v")) continue;
    const v = parse(t.slice(1));
    if (v && (!highest || compare(v, highest) > 0)) highest = v;
  }
  if (!highest || compare(base, highest) > 0) return base.join(".");
  return [highest[0], highest[1], highest[2] + 1].join(".");
}

/** Rewrite one top-level "version" field without reformatting the file. */
export function stampJson(text, version) {
  let replaced = 0;
  // Top-level only: two-space indent, as both files are formatted.
  const out = text.replace(/^( {2}"version":\s*)"[^"]*"/m, (_, head) => {
    replaced++;
    return `${head}"${version}"`;
  });
  if (replaced !== 1) throw new Error('no top-level "version" field to stamp');
  return out;
}

function readFloor() {
  return JSON.parse(fs.readFileSync(CONF, "utf8")).version;
}

function gitTags() {
  const out = execFileSync("git", ["tag", "--list", "v*"], {
    cwd: ROOT,
    encoding: "utf8",
  });
  return out.split("\n").filter(Boolean);
}

function main(argv) {
  const [cmd, arg] = argv;
  if (cmd === "next") {
    const version = nextVersion(readFloor(), gitTags());
    process.stdout.write(`version=${version}\ntag=v${version}\n`);
    return 0;
  }
  if (cmd === "stamp") {
    if (!parse(arg ?? "")) {
      process.stderr.write(
        `stamp needs an X.Y.Z version (got ${JSON.stringify(arg)})\n`,
      );
      return 2;
    }
    for (const file of [CONF, PKG]) {
      fs.writeFileSync(file, stampJson(fs.readFileSync(file, "utf8"), arg));
    }
    process.stdout.write(
      `stamped ${arg} into tauri.conf.json and package.json\n`,
    );
    return 0;
  }
  process.stderr.write("usage: release-version.mjs next | stamp <X.Y.Z>\n");
  return 2;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exit(main(process.argv.slice(2)));
}
