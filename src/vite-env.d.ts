/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_APP_TITLE: string;
  /**
   * "1" when the `jabot-host` Vite plugin is serving this page and a real
   * `jabot-hostd` is reachable over the HMR socket (scripts/dev/host-plugin.ts).
   * Set by `define`, so it is absent — not "0" — everywhere else.
   */
  readonly JABOT_LIVE_HOST?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
