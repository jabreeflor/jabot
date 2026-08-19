# UI stack

Question 2 from [`brief.md`](brief.md): React vs Svelte vs keep-it-vanilla,
and what we gain/lose porting [`prototypes/jabot-classic.html`](../../../prototypes/jabot-classic.html).

**Pick React 19 + TypeScript + Vite.** Do not keep the prototype as a
runtime. Do not pick Svelte or Solid for the product.

## What the prototype actually is

One HTML file: mac-style traffic lights, sidebar (crew strip, folders,
threads), chat (bubbles, toolblocks, composer), Inbox, Pull Requests, New
Chat modal, crew editor, context menu. Vanilla DOM + `onclick` + a
`THREADS` object with fake bodies.

It is a **visual and interaction contract**:

- Dark native-feeling chrome, SF-style type, iMessage-ish bubbles
  (`me` cream, `bot` graphite).
- Toolblocks as monospace cards (`▸ read` / `▸ edit` / `▸ bash`).
- Disappearing Threads: row animates out, Inbox badge increments.
- New Chat: folder + harness card (Claude / Codex / Pi / Custom).

It is **not** a renderer for streaming ACP `session/update`, permission
modals, virtualized history, or a live process list. Porting "as vanilla"
means rewriting it in six months anyway.

## Framework comparison (for *this* UI)

JaBot is a **messenger**: sidebar list + transcript that streams + composer
+ overlays. High-frequency updates are (1) token chunks on the last bubble,
(2) toolblock status, (3) sidebar badges. The expensive part later is a
long thread, not the chrome.

| | React 19 | Svelte 5 | Solid | Vanilla (keep the file) |
|---|---|---|---|---|
| Token streaming | Fine if the last message is isolated state. Easy to accidentally re-render the thread. | Fine-grained; less memoization tax. | Best default for per-token updates. | Manual DOM; prototype already does this for mocks. |
| Chat virtualization | [`@tanstack/react-virtual`](https://tanstack.com/virtual/latest/docs/chat) now has **end-anchored** chat (prepend history, follow-on-append, streaming growth). Conductor used `react-virtuoso`. | `@tanstack/svelte-virtual` exists; fewer chat examples. | `@tanstack/solid-virtual` exists. OpenCode's workbench is Solid. | You will write this yourself and get it wrong. |
| Ecosystem we will actually import | Markdown, diffs, Radix/menus, cmdk, TanStack Query, `react-markdown`, Shiki. Buzz + Conductor already live here. | Smaller. SVAR etc. exist; we would still hand-roll agent-chat pieces. | Smaller still. Great compiler; fewer "permission modal + virtual list + markdown" recipes. | Zero. Every overlay is bespoke. |
| ACP / desktop peers | Buzz desktop is React 19. Conductor is React 19. Claude Desktop is React. | No named agent-manager peer. | OpenCode app is Solid — because that *is* their web UI, not because Solid is required. | — |
| Hiring / codegen | Default. Models emit React. | Fine, less common. | Fine, less common. | Irrelevant. |
| Porting cost from the HTML | Mechanical: CSS variables stay, markup becomes JSX. | Same, plus new syntax. | Same, plus signals. | Zero today, maximum later. |

Svelte 5 or Solid would be a *slightly* nicer runtime for a token pump.
They would not be a nicer *product* stack. The transcript renderer maps ACP
updates onto bubbles and toolblocks
([adapter-design.md](../harness-integration/adapter-design.md)); that is
component work, not a reactivity emergency. When the list gets long, use
end-anchored virtualization — framework-agnostic, React-best-documented.

Vanilla is how you freeze a prototype. It is not how you attach a
bidirectional permission RPC and a session store.

## What to keep from jabot-classic.html

**Keep**

- `:root` tokens (`--win`, `--chat`, `--bub-me`, `--amber`, …). They *are*
  the brand.
- Layout: 310px sidebar, `max-width: 760px` thread, pill composer.
- Class names as a glossary (`toolblock`, `fthread`, `ibx-row`, `hcard`)
  until components exist.
- Interaction beats: fold animation, Inbox tabs, New Chat harness cards,
  context menu actions.

**Drop**

- Fake traffic lights. Use a real macOS title bar
  (`titleBarStyle: "overlay"` in Tauri) and `drag` regions.
- Inline `THREADS` / `CREW` fixtures as the store. They become seed data
  for Storybook or a mock host.
- Direct DOM `createElement` in event handlers.
- "Inflection's agent" copy (already flagged in harness findings).

**Rewrite as components (suggested cut)**

```
AppShell
  Sidebar (bot strip, folders, me-row)
  Main
    ChatView (header, transcript, composer)
    InboxView
    PullRequestsView
    CrewView
  Overlays (NewChat, BotEditor, Permission, ContextMenu)
```

Transcript internals should match ACP kinds, not the mock tuple list:

`stamp` / `sys` / `me` / `bot` / `toolblock` / permission card.

Streaming: one `AgentMessage` component whose text is a growing string
(or markdown AST). Do not re-render sibling toolblocks on each token.

## Styling

Prototype CSS is already the design system. Options:

1. **Keep a global CSS file** extracted from the prototype (fastest port,
   zero Tailwind rewrite). Preferred for MVP.
2. Tailwind like Buzz/Conductor — only if we want utility-first from day
   one. Do not mix both as a personality.

Do not introduce a component library that restyles bubbles into Material.
Radix (or vanilla `<dialog>`) for a11y primitives only: menus, modal,
focus trap.

## TypeScript

Yes. The host events are a typed ACP subset. The UI should consume a
narrow `JaBotEvent` union (already sketched in adapter-design), not
`any` JSON.

## Recommendation

React 19 + Vite + TypeScript, CSS extracted from the prototype, TanStack
Virtual when a thread is long enough to jank. Same UI stack as Buzz and
Conductor, so the shell choice (Tauri) and the UI choice reinforce each
other instead of fighting.
