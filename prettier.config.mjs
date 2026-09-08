/** @type {import("prettier").Config} */
const config = {
  // Explicit so a future Prettier default change cannot silently restyle the
  // tree. These match the existing first-party style (double quotes, semis,
  // 80 columns, trailing commas) so the baseline is a small mechanical delta.
  //
  // Prettier owns layout. Frontend lint (#224+) owns correctness (hooks,
  // unused, …) and must not restate style — use eslint-config-prettier when
  // that gate lands so the two cannot disagree.
  printWidth: 80,
  tabWidth: 2,
  useTabs: false,
  semi: true,
  singleQuote: false,
  trailingComma: "all",
  bracketSpacing: true,
  arrowParens: "always",
  endOfLine: "lf",
};

export default config;
