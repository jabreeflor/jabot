# Buzz setup (prior art)

Feeds [findings.md](findings.md). Research date 2026-08-19 against `block/buzz`
`main`. ACP-only notes already live in
[harness-integration/buzz.md](../harness-integration/buzz.md); this file is the
full product setup. Vision docs and a few READMEs disagree with implementation
— called out below.

## 1. What Buzz is vs JaBot

Buzz (Block) is a self-hostable **team workspace** where humans and agents share
one relay, identity system, and signed Nostr event log. A relay URL selects a
community. Agents are members with their own keypairs.
([README](https://github.com/block/buzz),
[ARCHITECTURE.md](https://github.com/block/buzz/blob/main/ARCHITECTURE.md))

| Buzz | JaBot |
|---|---|
| Team/community workspace | Personal bot-crew messenger |
| Relay is source of truth | Local host is source of truth (initially) |
| Human + agent Nostr identities | One owner + local crew identities |
| Channels, forums, huddles, git forge | Chats, fold, Inbox, harnesses, PRs |
| Postgres / Redis / S3 | SQLite / files / keychain |
| Mobile copies a relay identity | Mobile pairs with a **revocable** host grant |
| ACP is one subsystem | Harness execution is central |

Copy the **seam**, not the product:

```text
chat/event system → durable host supervisor → ACP → harness → MCP/tools
```

## 2. Architecture

Not peer-to-peer despite Nostr’s event format: no gossip. All reads/writes go
through `buzz-relay` (Axum WS + REST).

```text
React/Tauri desktop · Flutter mobile · buzz CLI
        │  WS / REST / signed events
        ▼
Desktop native supervisor
        └─ buzz-acp ── stdio ACP ── Claude/Codex/Goose/Hermes/…
                │                         └─ optional MCP
                └──────── WS/REST ── buzz-relay
                                         ├─ Postgres
                                         ├─ Redis
                                         └─ S3/MinIO
```

Renderer does **not** own Claude/Codex. Tauri owns `buzz-acp`. Closing a chat
view does not inherently stop work.

Event pipeline (stored events): authorize → verify pubkey/signature → membership
→ idempotent insert → Redis publish → fan-out → search → audit → workflows.
**Persist before UI notify** — the ordering JaBot should keep for Inbox.

Crate map (abbrev.): `buzz-core`, `buzz-relay`, `buzz-db`, `buzz-auth`,
`buzz-pubsub`, `buzz-search`, `buzz-audit`, `buzz-workflow`, `buzz-acp`,
`buzz-agent`, `buzz-dev-mcp`, `buzz-persona`, `buzz-cli`, `buzz-pair-relay`,
`buzz-admin`. Subsystems do not call each other; the relay coordinates.

JaBot analog: one host coordinator over conversation store, run supervisor,
harness registry, permission broker, Inbox projection, repo/worktree, remote
transport.

## 3. Setup / install / run

Packaged desktop defaults to `ws://localhost:3000` (`BUZZ_RELAY_URL`). Desktop
does not replace the need for a relay.

```bash
git clone https://github.com/block/buzz.git
. ./bin/activate-hermit
just setup && just build
just dev          # or just relay + just desktop-dev
```

Production: `deploy/compose` (Postgres, Redis, MinIO, git volume).

Each agent gets a Nostr keypair (`buzz-admin generate-key`) printed once, then
`add-member`. Relay needs stable `BUZZ_RELAY_PRIVATE_KEY`.

```bash
export BUZZ_PRIVATE_KEY="nsec1..."
export BUZZ_RELAY_URL="ws://localhost:3000"
export BUZZ_ACP_AGENT_COMMAND="claude-agent-acp"   # default goose
export BUZZ_ACP_AGENT_ARGS="acp"                   # comma-delimited
buzz-acp
```

| Variable | Default | Meaning |
|---|---|---|
| `BUZZ_PRIVATE_KEY` | required | Agent Nostr key |
| `BUZZ_ACP_AGENT_COMMAND` | `goose` | ACP executable |
| `BUZZ_ACP_AGENT_ARGS` | `acp` | Comma-delimited; **no commas in custom args** |
| `BUZZ_ACP_AGENTS` | `1` | Subprocess count 1–32 |
| `BUZZ_ACP_IDLE_TIMEOUT` | `620`s | Stdout idle cancel |
| `BUZZ_ACP_MAX_TURN_DURATION` | `7200`s | Hard turn limit |
| `BUZZ_ACP_RESPOND_TO` | `owner-only` | Inbound author policy |
| `BUZZ_ACP_PERMISSION_MODE` | `bypass-permissions` in current source | **Do not copy as JaBot default** |

Root `.env.example` still mentions deprecated `BUZZ_ACP_TURN_TIMEOUT=320`.
Prefer crate README + `config.rs`.

Desktop stores nsecs in the OS keyring; `0o600` file fallback. JaBot should
copy that discipline, not the proliferation of signing keys.
([buzz-acp README](https://github.com/block/buzz/blob/main/crates/buzz-acp/README.md),
[SECURITY.md](https://github.com/block/buzz/blob/main/SECURITY.md))

## 4. Harness catalog and BYOH

[PR #2773](https://github.com/block/buzz/pull/2773) — three tiers.

**Tier 1 (compiled-in):** `goose` (`goose acp`, `GOOSE_MODE=auto`); `claude`
(`claude-agent-acp` / fallback `claude-code-acp`, probe `claude auth status`);
`codex` (`codex-acp`, probe `codex login status`); `buzz-agent` (bundled).
Catalog states: `Available` | `CliMissing` | `AdapterMissing` |
`AdapterOutdated` | `NotInstalled`.

**Tier 2 presets** (`presets.rs` is SoT; README prose can lag). All current
preset `env` maps are empty:

| ID | Command | Args | Notes |
|---|---|---|---|
| `devin` | `devin` | `acp` | |
| `cursor` | `cursor-agent` | `acp` | |
| `omp` | `omp` | `acp` | False-ready risk: needs `omp setup` |
| `grok` | `grok` | `agent --always-approve stdio` | **Do not inherit always-approve** |
| `opencode` | `opencode` | `acp` | |
| `kimi` | `kimi` | `acp` | |
| `amp` | `amp-acp` | — | `amp` CLI present + adapter missing → `AdapterMissing` |
| `hermes` | `hermes-acp` | — | Preset env empty; **`buzz-acp` still sets `HERMES_ACP_SKIP_CONFIGURED_MCP=1`** |
| `openclaw` | `openclaw` | `acp` | PATH “Available” ≠ Gateway running; `BUZZ_*` reach the bridge, not the Gateway exec env |

Presets currently get `AuthStatus::NotApplicable` — installed ≠ logged in.

**Tier 3 custom JSON** in app-data `custom_harnesses/`:

```json
{
  "id": "my-agent",
  "label": "My Agent",
  "command": "my-agent-bin",
  "args": ["acp"],
  "env": { "MY_AGENT_MODE": "acp" },
  "installInstructionsUrl": "https://example.com/docs",
  "installHint": "Download from example.com"
}
```

`id`: `[a-z0-9_][a-z0-9_-]*`. No install scripts. No remote icons. Reserved
ids = all tier 1+2. Host-reserved env (`BUZZ_MANAGED_AGENT`, …) stripped.
Invalid files skipped.

GUI PATH is starved: Buzz merges login-shell PATH, `~/.local/bin`, nvm, sidecar
dir. Auth probes run in parallel against the resolved absolute binary.

JaBot should make **readiness probes and capabilities data-driven for every
tier**, not only special-case builtins.

## 5. Process / session supervisor

```text
Buzz Desktop
  └─ buzz-acp
       ├─ ACP agent process(es)
       └─ optional MCP
```

Desktop injects relay identity, agent command/args, timeouts, `BUZZ_MANAGED_AGENT`
+ start nonce, optional git-credential-nostr. Failed readiness → trusted
`BUZZ_ACP_SETUP_PAYLOAD` (reserved key; user env cannot forge it).

Runtime receipts: PID, instance id, start time, scoped to agent identity +
relay URL. Logs pair-scoped.

Unix: `process_group(0)`; signal negative PGID. Windows: Job Object
`KILL_ON_JOB_CLOSE`; after Desktop restart, `taskkill /T /F`. Ownership proof
is the **env marker**, not the executable name (custom binaries are arbitrary).
Orphan sweep: two consecutive ticks before kill.

**macOS close-window:** `prevent_close` + hide; tray keeps agents. Explicit
Quit: SIGTERM → 2s → SIGKILL → sweep. Hide-on-close is macOS-gated in reviewed
source; Windows/Linux keep-alive **unknown**.

`buzz-acp`: 1–32 subprocesses; one in-flight prompt per channel; batch pending
events; `steer` default for mid-turn input; respawn crashes; reconnect with
`since` cursor.

**Critical gap:** channel→ACP session map is **in memory**. Restart →
`session/new` (Codex may fork a new cloud conversation).
[Issue #5342](https://github.com/block/buzz/issues/5342).
`buzz-agent` advertises `"loadSession": false`.
[PR #6088](https://github.com/block/buzz/pull/6088) (open) proposes durable
receipts + `session/load`. Review requirements (fingerprint, atomic update,
don’t delete on timeout) are JaBot requirements — do not wait for a naïve
map-to-ID.

Folding: Buzz validates **process hosting**, not the UX. Separate visibility,
execution, provider session, Inbox projection. True app exit currently **stops**
local agents — does not solve “work survives desktop process death.”

## 6. Agent identity and personas

Every Buzz agent: secp256k1 keypair, membership, authorship, optional owner
tag, channel access. JaBot needs stable crew IDs for attribution; not Nostr
until remote auth requires host/device keys.

**Persona packs** (Open Plugin Spec-ish):

```text
.plugin/plugin.json
agents/*.persona.md
skills/*/SKILL.md
.mcp.json
hooks/
instructions.md
```

`.persona.md` = YAML frontmatter (`name`, `runtime`, `model`, `skills`,
`mcp_servers`, `subscribe`, `triggers`, hooks) + Markdown body. Unknown
frontmatter fields rejected. Prompt layering: platform/base vs persona vs pack
vs dynamic context — worth porting so Chief behavior does not tangle with
harness mechanics. Some pack behavior is still aspirational (skill copy, true
system-prompt injection).
([PERSONA_PACK_SPEC.md](https://github.com/block/buzz/blob/main/crates/buzz-persona/PERSONA_PACK_SPEC.md))

JaBot: template (role, prompt, skills, default harness, tools, Inbox rules) vs
instance (id, secrets, history). `subscribe`/`triggers` → Inbox/folder routing,
not team channels. Hooks disabled for untrusted downloads.

## 7. Event model / UX

Typed `kind` integers (messages, agent jobs `43001–43006`, workflow lifecycle,
git/PR `1617–1633`, pairing `24134`, huddles, …). Copy **typed projections +
extensible log**, not Nostr integers.

Working: streams/threads, DMs, forums, search, canvases, media, agent activity,
workflows, git hosting, huddles. Mobile in active development. Workflow
**approval schema exists but executor does not durably suspend** — warning for
JaBot: don’t ship approval UX without durable suspend/resume.

Minimal JaBot “workflows”: run completed/failed → Inbox; permission → urgent
Inbox; PR changed → wake reviewer; schedule → wake Chief.

Git: Buzz hosts repos (smart HTTP + NIP-34). Forge vision (branch-as-room) is
partially designed. JaBot: link conversation ↔ repo/worktree/PR/checks; **do
not become a git host**.

No direct equivalent of disappearing threads. Precedent: process activity is
projected **separately** from the visible chat (tray / agent activity).

## 8. ACP host behavior

```text
relay event → buzz-acp queue → session/prompt → session/update
           → agent uses buzz-cli → signed relay message → UIs
```

Required: `initialize`, `session/new`, `session/prompt`, `session/update`,
final `stopReason`. Mid-turn: `queue` | `steer` | `interrupt` |
`owner-interrupt`. If adapter lacks steer, cancel and redispatch merged
context. Maps to “user opens a folded thread and adds guidance.”

**Permissions:** current CLI default `bypass-permissions`. If
`session/request_permission` arrives, pick `allow_once` else `reject_once`.
Cancel resolves outstanding permission as `cancelled` first. **JaBot Inbox
should own this surface.** Persist request, tool, risk, run, expiry, decision.

Hermes: `HERMES_ACP_SKIP_CONFIGURED_MCP=1` so host-selected MCP wins. Generalize
to every harness with ambient tools.

Inbound gate: `owner-only` (default) | allowlist | anyone | nobody. JaBot:
authenticated owner + trusted local automation, still gated once mobile exists.

`buzz-agent`: small ACP loop, MCP from `session/new` only, no networked MCP, no
`session/load`. Lesson: ACP = agent boundary, MCP = tool boundary, don’t import
one SDK through the app.

## 9. App shell

Tauri 2 + Rust + React 19 + Vite + TanStack + TipTap. Sidecars: `buzz-acp`,
`buzz-agent`, `buzz-dev-mcp`, `buzz`, `git-credential-nostr`, …

Tauri fits Buzz because the protocol/server is already Rust. **JaBot must not
switch to Tauri merely because Buzz did.** Essential split:

```text
renderer: presentation
native/main: process + secret owner
background host: durable runs + remote API
```

Electron can preserve that (IPC, Job Object helper, background service).

Mobile: Flutter, secure storage, QR, WebSockets, biometrics. Treat as active
dev, not a finished reference. Split is sound: phone is a host client; it does
not spawn coding harnesses.

## 10. Data, auth, remote

Production Buzz: Postgres + Redis + S3 + git volume. README vs `.env.example`
disagree on search (Postgres FTS vs Typesense vars) — drift.

JaBot local: SQLite (threads, messages, runs, run_events, inbox, harnesses,
session receipts, device grants) + files (logs, attachments, worktrees) +
keychain (provider creds, host key, device grants).

Auth: NIP-42 (WS challenge) / NIP-98 (HTTP). JaBot equivalent: host device key,
per-client grant, TLS, replay protection, per-run tool policy, audit — **not**
Nostr.

**Pairing (NIP-AB):** QR `nostrpair://<ephemeral-pub>?secret=&relay=&v=1`;
ephemeral ECDH; NIP-44; 6-digit SAS; 120s expiry; Confirm on both screens.
**Current payload includes `nsec`.** Spec: no revocation, no multi-device
coordination, physical-presence assumption.

Copy the **state machine** (ephemeral keys, SAS, expiry, biometric). Do **not**
copy payload semantics. Issue a revocable device grant; host keeps the master
key.

Remote-agent vision (Kubernetes provider, control via relay) still marked
planned. JaBot: explicit host control API (health, revoke, logs, upgrade);
relay-only control is not automatically simpler.

## 11. What JaBot should port

Renderer/host split; three-tier catalog; one typed harness descriptor;
PATH-aware Doctor; reserved env namespace; process-tree containment + marker
orphan sweep; pair-scoped logs/receipts; durable run-event journal; **scoped**
session receipts; steer/queue/interrupt; owner-facing permission broker;
persona/template separation; host-authoritative MCP; JSON CLI; PR as work
container (not git host); tray/background activity; pairing state machine with
revocable grants; remote provider boundary; tiny workflow engine
(complete/fail/permission/PR/schedule).

Numbered list in [findings.md](findings.md).

## 12. What JaBot should not port

Nostr relay as local core; per-bot cryptographic keypairs; Postgres/Redis/S3
for local-first; team membership as auth; forums/huddles/canvases/tenancy;
git forge; `bypass-permissions`; PATH-only readiness; unscoped session maps;
master-secret QR; high parallelism without admission; user-defined installers;
framework rewrite to match Buzz.

## 13. Open questions / risks

1. What survives true Quit?
2. Which ACP runtimes actually `session/load`?
3. Session compatibility fingerprint contents.
4. Indeterminate resume (timeout ≠ gone — don’t fork).
5. Distinct Inbox cards for permission vs clarify vs merge vs auth.
6. Electron Windows process trees.
7. Custom harness = user’s OS privileges; catalog ≠ sandbox.
8. Worktree per run vs shared cwd.
9. Provider login vs API key vs gateway — different Doctor surfaces.
10. Mobile scopes and grant rotation (NIP-AB does not solve revocation).
11. Multi-host ownership if we ever have more than one daemon.
12. Don’t ship approval workflows without durable suspend (Buzz’s gap).
13. Cross-platform hide-on-close.
14. Source drift (timeouts, preset count, search).
15. Treat PR #6088 review findings as JaBot requirements.

## 14. Sources

- https://github.com/block/buzz
- https://github.com/block/buzz/blob/main/ARCHITECTURE.md
- https://github.com/block/buzz/blob/main/README.md
- https://github.com/block/buzz/blob/main/crates/buzz-acp/README.md
- https://github.com/block/buzz/pull/2773
- https://github.com/block/buzz/issues/5342
- https://github.com/block/buzz/pull/6088
- https://github.com/block/buzz/blob/main/crates/buzz-persona/PERSONA_PACK_SPEC.md
- https://github.com/block/buzz/blob/main/desktop/src-tauri/src/managed_agents/discovery/presets.rs
- https://github.com/block/buzz/blob/main/desktop/src-tauri/src/managed_agents/custom_harnesses.rs
- https://github.com/block/buzz/blob/main/crates/buzz-core/src/pairing/NIP-AB.md
- https://github.com/block/buzz/blob/main/SECURITY.md
