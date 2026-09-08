// Shared frontend lint. This file is the one configuration for the renderer,
// its tests, and the TypeScript development tooling that ships beside them.
//
// #224 turned on React Rules of Hooks and exhaustive-deps. #226 adds
// `@typescript-eslint/no-explicit-any`. #225 adds type-aware promise
// rules. Do not stand up a second linter or a second config.
//
// Type-aware rules need a tsconfig. `projectService` picks
// `tsconfig.json` for app/tests/dev tooling and `tsconfig.node.json` for
// `vite.config.ts`. Files that are not TypeScript are not linted here.

import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    name: "jabot/ignores",
    ignores: [
      "**/node_modules/**",
      "dist/**",
      "dist-ssr/**",
      "coverage/**",
      ".jabot-dev/**",
      // Rust crate, cargo output, and the bundled ACP adapters.
      "src-tauri/**",
      // Vendored plugin snapshot — not first-party product code.
      "plugins/**",
      // Host-owned git worktrees (and any nested checkout that uses the
      // same directory name).
      "**/worktrees/**",
      "**/.worktrees/**",
      "**/.git/**",
    ],
  },
  {
    name: "jabot/typescript",
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        projectService: {
          // Throwaway probes from scripts/tests/lint-rules.mjs, fed on
          // stdin so they never land in the tree. They are not in a
          // tsconfig; the default project is enough to type Promise and
          // MouseEventHandler.
          allowDefaultProject: [
            "scripts/tests/lint-probe-*.ts",
            "src/__lint-probe-*.tsx",
          ],
        },
        tsconfigRootDir: import.meta.dirname,
        ecmaFeatures: { jsx: true },
      },
    },
    plugins: {
      "@typescript-eslint": tseslint.plugin,
      "react-hooks": reactHooks,
    },
    rules: {
      // Official React Hooks plugin — do not re-implement these.
      // Recommended in v7 also turns on React Compiler rules; keep these
      // two explicit so later issues can append without inheriting that set.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",

      // tsc --noEmit is strict and rejects implicit any. It still allows
      // `const x: any` and `x as any`. This is the documented no-any policy.
      "@typescript-eslint/no-explicit-any": "error",

      // A discarded Promise is almost always a missed rejection. `void`
      // marks deliberate background work; the callee, or a `.catch` on
      // the same expression, is the error strategy. Do not set
      // `ignoreVoid: false` without a replacement marker — the codebase
      // already uses `void` that way.
      "@typescript-eslint/no-floating-promises": [
        "error",
        { ignoreVoid: true },
      ],

      // Passing an async function where a void callback is expected
      // hides rejections the same way. Event-handler attributes stay
      // on: `onClick={async () => …}` is the bug this rule exists for.
      // There is no `checksVoidReturn.attributes: false` exemption.
      "@typescript-eslint/no-misused-promises": [
        "error",
        {
          checksConditionals: true,
          checksSpreads: true,
          checksVoidReturn: {
            arguments: true,
            attributes: true,
            inheritedMethods: true,
            properties: true,
            returns: true,
            variables: true,
          },
        },
      ],
    },
  },
);
