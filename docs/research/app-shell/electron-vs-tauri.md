# Electron vs Tauri 2

Question 1 from [`brief.md`](brief.md). Constrained by
[harness-integration](../harness-integration/adapter-design.md): the UI never
talks to Claude/Codex/Pi. A host process spawns ACP adapters over stdio and
forwards structured events.

**Pick Tauri 2.** Electron is the fallback if we later decide the host itself
must be TypeScript.

## What we actually need from a shell

Not "a website in a window." The hard parts:

1. Spawn **many** long-lived child processes (ACP adapters + whatever they
   spawn).
2. Bidirectional stdio (newline-delimited JSON-RPC), logs on stderr, SIGTERM
   process groups on kill.
3. Keep those children alive when the **window** closes.
4. Stream events into a messenger UI without melting idle RAM.
5. Optionally, later, a raw PTY pane (`xterm.js`).
6. Sign, notarize, auto-update on macOS.

Both shells can do all of this. The difference is *where the host lives* and
what you pay when the app is just sitting in the Dock with folded threads.

## Architecture fit

```
JaBot UI (webview)  ←IPC/events→  Host / session supervisor  ←ACP stdio→  adapter
```

| If the host is… | Natural shell |
|---|---|
| A **TypeScript server** you already have (OpenCode's `opencode serve`) | Electron. Node is already in-process. |
| A **process supervisor** speaking ACP, with isolation, PGIDs, later PTY | Tauri. Rust is the host. Official ACP crate is Rust. |

JaBot is the second row. The adapters (`claude-agent-acp`, `codex-acp`,
`pi-acp`, user commands) are already separate processes. We do **not** need
Node in the shell to run them. We need a parent that is good at children.

OpenCode's 2026 move **off Tauri onto Electron** is the clean counterexample
([Brendonovich, DEV](https://dev.to/brendonovich/moving-opencode-desktop-to-electron-4hip)):
their core is TypeScript, the desktop was a wrapper around `opencode serve`,
and bundling a CLI sidecar was slower/flakier than running the server inside
Electron's Node. They also wanted Chromium everywhere for UI consistency.
None of that is our constraint. Our host is the supervisor; our MVP is
macOS; our UI is a chat client, not an IDE workbench.

## What similar tools picked

| App | Shell | Why it matters |
|---|---|---|
| **Buzz** ([block/buzz](https://github.com/block/buzz)) | **Tauri 2 + React 19** | Named prior art. Desktop spawns `buzz-acp` sidecars; UI never talks to Claude/Codex. Rust backend tracks PIDs, logs, orphan sweeps, Unix process groups. |
| **Conductor** ([conductor.build](https://conductor.build/)) | **Tauri + React 19** | Closest product: parallel Claude/Codex/Cursor agents on a Mac, worktrees, chat. Rust core spawns agent CLIs. Rewrite kept Tauri; they virtualized chat (`react-virtuoso`) rather than switching shells. macOS-first, Windows waitlist. |
| **ChatML** | **Tauri 2 + React**, Go sidecar | Parallel coding sessions + real PTYs. Idle RAM cited **80–120 MB** vs Electron **300–500 MB** idle. Same "always-on next to the IDE" constraint we have. ([field report](https://chatml.com/blog/desktop-ai-app-tauri-2-instead-of-electron)) |
| **Claude Desktop** | **Electron** (~41.x) + `node-pty` | Wraps claude.ai, MCP, Cowork VMs. Electron because the product *is* a Chromium app plus Node addons — not a process supervisor that happens to have a UI. |
| **Cursor** | **Electron** (VS Code fork) | An IDE. Irrelevant as a shell template; we are not forking Monaco. |
| **OpenCode Desktop** | **Tauri → Electron** (2026) | TypeScript-all-the-way-down. Proves Electron if the host is Node; does not prove Electron for us. |
| **Warp** | **Custom Rust + GPU UI** | Tried Electron, dumped it. Built WarpUI. Correct for a 60 fps terminal; overkill for iMessage-style chat. Do not do this. |
| **Happy Coder** | CLI **daemon** + mobile/web clients | The UI/host split we want *later* for remote. Not an app-shell pick for MVP1. |

Buzz is the seam to copy. Conductor is the product-shape to copy. Warp is
what happens if you confuse "we might show a PTY someday" with "we are a
terminal."

## Spawning many children

Both can spawn unbounded children. There is no Electron- or Tauri-imposed
cap that matters.

**Electron**

- ACP adapters: `child_process.spawn` with piped stdio. This is the right
  API. Do **not** put adapters in `utilityProcess` — that API forks a
  **Node script** via Chromium's Services ([docs](https://www.electronjs.org/docs/latest/api/utility-process)),
  not an arbitrary `claude-agent-acp` binary.
- `utilityProcess` *is* useful for a TypeScript supervisor if you insist the
  host stay in JS (crash boundary, MessagePorts). Extra Node isolate per
  helper.
- macOS extras that are genuinely nice: `disclaim: true` so TCC prompts from
  child agents are not attributed to JaBot; `allowLoadingUnsignedLibraries`
  for unsigned native addons. We would still need Hardened Runtime exceptions
  to exec unsigned PATH binaries.

**Tauri 2**

- ACP adapters on PATH: spawn from Rust with `std::process::Command` /
  `tokio::process`, or `tauri-plugin-shell` `Command.spawn` with piped
  stdout/stdin ([shell plugin](https://v2.tauri.app/plugin/shell/)).
  Capabilities allowlists apply to the **JS** side. The host should spawn
  from Rust so the webview never gets a generic "run anything" permission.
- Bundled helpers (`jabot-host` if we split later, or a shipped adapter):
  Tauri **sidecar** + `externalBin` with `-$TARGET_TRIPLE` suffixes
  ([sidecar docs](https://v2.tauri.app/develop/sidecar/)). Buzz does this for
  `buzz-acp` / `buzz-agent`.
- Unix process groups (Buzz: spawn with a new PGID, kill the group) are
  trivial in Rust and easy to get wrong in Node.

For MVP, adapters are PATH-probed (Buzz tier 2), not bundled. Sidecar is for
*our* binaries, not for `claude`.

## Memory: shell vs session

The brief asks for "small memory per idle session." Split the bill:

| Cost | Who pays | Idle magnitude (order of) |
|---|---|---|
| Chromium + Node (Electron) or WKWebView + Rust (Tauri) | **Once**, for the app | Electron often **~200–500 MB** idle; Tauri/WebKit apps in this category **~80–150 MB**. ChatML: 80–120 MB Tauri vs 300–500 MB Electron idle. |
| ACP adapter + harness (Claude/Codex/Pi, often Node) | **Per live thread** | Dominates. Hundreds of MB when the agent is actually working; tens to low hundreds idle. The shell does not change this. |
| Extra Node utility process / extra Electron renderer | Per helper / per window | Avoid. One window. |
| PTY + `xterm.js` | Per raw-terminal session | Not MVP. |

Idle **JaBot sessions** (folded, still working) cost whatever the adapter
process costs. Picking Electron does not make Claude cheaper. Picking Tauri
makes the always-on *shell* cheaper, which is the part we control, and the
part that sits next to VS Code / Cursor all day.

Do not trust SEO roundups that claim "96% smaller" as a universal law. Trust
the architectural split (bundled Chromium vs system WebKit) and the field
numbers from apps in our category.

WKWebView on macOS is a feature for a macOS-only MVP, not a bug. OpenCode
hated WebKit because they ship Windows/Linux and want Chrome DevTools. We
can use Safari Web Inspector, and Conductor even ran their React tree in
Chrome during perf work without abandoning Tauri.

## PTY maturity (for the deferred escape hatch)

| | Electron | Tauri |
|---|---|---|
| PTY lib | [`node-pty`](https://github.com/microsoft/node-pty) (Microsoft, VS Code). Mature, native addon, version-locked to Electron/Node ABI. | [`portable-pty`](https://docs.rs/portable-pty/latest/portable_pty/) (WezTerm, 0.9.x). Trait-based, Unix + ConPTY. Used from Tauri apps in the wild. |
| Renderer | [`xterm.js`](https://github.com/xtermjs/xterm.js) in the webview. Official Electron example wires `node-pty` ↔ xterm over IPC. | Same `xterm.js` in the webview. Bytes come from Rust. ChatML and Conductor both do this. Community `tauri-plugin-pty` exists; prefer first-party `portable-pty` in *our* host over an extra plugin. |

`node-pty` is more battle-tested in Electron IDEs. `portable-pty` is the
right crate if the host is already Rust. Neither is needed until we add a
raw terminal runtime type.

Stdio ACP does **not** want a PTY. A PTY injects `\r`, job control, and
WINCH into a JSON-RPC stream. That is how you get "we scrape the TUI"
accidents. Host `Command` with piped stdio, period.

## The honest Electron case

Choose Electron instead if any of these become true:

- The host is rewritten in TypeScript (ACP JS SDK,
  [`@agentclientprotocol/sdk`](https://www.npmjs.com/package/@agentclientprotocol/sdk))
  and we want one language.
- We need Chrome-only web APIs or identical Windows/Linux rendering *this
  year*.
- We are embedding a real IDE (Monaco, VS Code extensions).

Until then, Electron is a 100+ MB Chromium tax and a Node ABI tax (`node-pty`
rebuilds on every Electron bump) for a host we would write in the wrong
language.

## Recommendation

**Tauri 2 + Rust host + React webview.** Copy Buzz's process seam and
Conductor's UI stack. Keep the door open to extract the host into a sidecar
or launchd agent later without changing the UI. See
[process-architecture.md](process-architecture.md).
