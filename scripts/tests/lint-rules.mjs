#!/usr/bin/env node
// Proves the shared frontend lint rules actually fire.
//
// `npm run lint` already runs eslint over the tree. That only shows the
// tree is clean. This feeds representative probes on stdin (so they
// never land in the tree) and expects discarded promises, a misused
// async callback, a JSX async handler, a conditional hook, a missing
// effect dependency, and both `any` forms to fail, and a handled flow
// to pass. A rule rename or a `checksVoidReturn.attributes: false`
// exemption makes this fail.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

function lint(name, source) {
  const result = spawnSync(
    "npx",
    [
      "eslint",
      "--max-warnings=0",
      "--stdin",
      "--stdin-filename",
      `scripts/tests/lint-probe-${name}.ts`,
    ],
    { cwd: ROOT, encoding: "utf8", input: source },
  );
  return {
    status: result.status ?? 1,
    out: `${result.stdout}${result.stderr}`,
  };
}

function check({ name, source, expect, rule }) {
  const { status, out } = lint(name, source);
  const hit = rule ? out.includes(rule) : false;
  if (expect === "fail") {
    if (status === 0 || !hit) {
      throw new Error(
        `${name}: expected ${rule} to fail the gate\n${out || "(no eslint output)"}`,
      );
    }
    return;
  }
  if (status !== 0) {
    throw new Error(`${name}: expected a handled flow to pass\n${out}`);
  }
}

check({
  name: "floating-bad",
  source: `export function discarded(): void {
  Promise.resolve();
}
`,
  expect: "fail",
  rule: "no-floating-promises",
});

check({
  name: "misused-bad",
  source: `const onClick: () => void = async () => {
  await Promise.resolve();
};
export const probe: () => void = onClick;
`,
  expect: "fail",
  rule: "no-misused-promises",
});

check({
  name: "jsx-handler-bad",
  source: `import type { MouseEventHandler } from "react";

export const onClick: MouseEventHandler<HTMLButtonElement> = async () => {
  await Promise.resolve();
};
`,
  expect: "fail",
  rule: "no-misused-promises",
});

check({
  name: "hooks-conditional-bad",
  source: `import { useState } from "react";

export function Broken(): null {
  if (true) useState(0);
  return null;
}
`,
  expect: "fail",
  rule: "react-hooks/rules-of-hooks",
});

check({
  name: "hooks-deps-bad",
  source: `import { useEffect } from "react";

export function Broken({ n }: { n: number }): null {
  useEffect(() => {
    console.log(n);
  }, []);
  return null;
}
`,
  expect: "fail",
  rule: "react-hooks/exhaustive-deps",
});

check({
  name: "explicit-any-bad",
  source: `export const probe: any = 1;
`,
  expect: "fail",
  rule: "no-explicit-any",
});

check({
  name: "as-any-bad",
  source: `export const probe = 1 as any;
`,
  expect: "fail",
  rule: "no-explicit-any",
});

check({
  name: "handled-good",
  source: `export async function awaited(): Promise<void> {
  await Promise.resolve();
}

export function background(): void {
  void Promise.resolve().catch((error: unknown) => {
    console.error(error);
  });
}
`,
  expect: "pass",
  rule: null,
});

console.log(
  "  lint-rules: discarded promises, misused async callbacks, hook mistakes, and explicit any fail; handled flows pass",
);
