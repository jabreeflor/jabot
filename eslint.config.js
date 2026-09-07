// Shared frontend lint configuration.
//
// One file, one `npm run lint`, one verify.sh gate. Later TypeScript lint
// issues (#226 no-explicit-any, and friends) extend this file rather than
// standing up a second linter. Hooks come from the official
// `eslint-plugin-react-hooks` (the #224 setup); type-aware promise rules
// are the #225 extension.
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
      "dist/**",
      "coverage/**",
      "node_modules/**",
      "src-tauri/**",
      "plugins/**",
      ".jabot-dev/**",
      "**/worktrees/**",
      "**/.worktrees/**",
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
          allowDefaultProject: ["scripts/tests/lint-probe-*.ts"],
        },
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      "@typescript-eslint": tseslint.plugin,
      "react-hooks": reactHooks,
    },
    rules: {
      // Official React Hooks plugin — do not re-implement these.
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "error",

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
