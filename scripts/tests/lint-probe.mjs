#!/usr/bin/env node
// Proves the shared frontend lint still fails the two cases that justified
// the gate: a hook called behind a condition, and an effect that closes over
// a value it does not list. A config that no longer reports those is not
// the gate — the tree being clean is not enough.
//
// The probes are linted in memory against a path under src/ so they match
// the first-party files glob. Nothing is written into the tree.

import { ESLint } from "eslint";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

const CASES = [
  {
    name: "conditional hook",
    rule: "react-hooks/rules-of-hooks",
    code: `
      import { useState } from "react";
      export function Bad({ ready }: { ready: boolean }) {
        if (ready) {
          const [n] = useState(0);
          return n;
        }
        return 0;
      }
    `,
  },
  {
    name: "missing dependency",
    rule: "react-hooks/exhaustive-deps",
    code: `
      import { useEffect, useState } from "react";
      export function Bad({ id }: { id: string }) {
        const [n, setN] = useState(0);
        useEffect(() => {
          setN(id.length);
        }, []);
        return n;
      }
    `,
  },
];

const eslint = new ESLint({
  cwd: root,
  overrideConfigFile: path.join(root, "eslint.config.js"),
});

let failed = 0;
for (const probe of CASES) {
  const results = await eslint.lintText(probe.code, {
    filePath: path.join(root, "src", `__lint-probe-${probe.name.replace(/\s+/g, "-")}.tsx`),
  });
  const messages = results.flatMap((result) => result.messages);
  if (messages.some((message) => message.ruleId === probe.rule)) {
    console.log(`  lint probe: ${probe.name} fails ${probe.rule}`);
    continue;
  }
  failed += 1;
  console.error(`  lint probe: ${probe.name} did not report ${probe.rule}`);
  if (messages.length === 0) {
    console.error("    (no messages — the file was probably ignored or unmatched)");
  } else {
    for (const message of messages) {
      console.error(`    ${message.ruleId ?? "unknown"}: ${message.message}`);
    }
  }
}

if (failed) process.exit(1);
