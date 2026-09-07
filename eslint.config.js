// Shared frontend lint. This file is the one configuration for the renderer,
// its tests, and the TypeScript/JavaScript development tooling that ships
// beside them.
//
// #224 turned on React Rules of Hooks and exhaustive-deps. This change
// (#226) adds `@typescript-eslint/no-explicit-any`. Issue #225 (floating /
// misused promises) adds its rules here too — do not stand up a second
// linter or a second config.
//
// Type-aware typescript-eslint rules are not enabled yet. The parser is
// wired so #225 can set `parserOptions.projectService = true` (or an
// equivalent project) without restructuring the file.

import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

const FIRST_PARTY = [
  "src/**/*.{ts,tsx}",
  "tests/**/*.{ts,tsx}",
  "scripts/dev/**/*.{ts,tsx,js,mjs,cjs}",
  "scripts/tests/**/*.{js,mjs,cjs}",
  "vite.config.ts",
  "vitest.config.ts",
  "eslint.config.js",
];

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
      "**/.git/**",
    ],
  },
  {
    name: "jabot/frontend",
    files: FIRST_PARTY,
    plugins: {
      "react-hooks": reactHooks,
      "@typescript-eslint": tseslint.plugin,
    },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        ecmaFeatures: { jsx: true },
      },
    },
    rules: {
      // Recommended in v7 also turns on React Compiler rules. Those are
      // outside this issue's scope; keep the hooks-correctness rules
      // explicit so later issues can append without inheriting that set.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",
      // tsc --noEmit is strict and rejects implicit any. It still allows
      // `const x: any` and `x as any`. This is the documented no-any policy.
      "@typescript-eslint/no-explicit-any": "error",
    },
  },
);
