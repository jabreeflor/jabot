# Hermes Agent setup (prior art)

Feeds [findings.md](findings.md). Research snapshot 2026-08-19. `main` reported
package version `0.20.4`. Hermes is changing quickly; some public docs conflict
with current source — flagged below.

## 1. What Hermes Agent is

Hermes Agent (Nous Research) is a complete, provider-agnostic agent **runtime
and product**, not a UI around other coding agents. One `AIAgent` core supplies
model/provider resolution, tools, memory, skills, subagents, cron, messaging
adapters, plus CLI, TUI, desktop, ACP, and HTTP surfaces.
([Docs](https://hermes-agent.nousresearch.com/docs/),
[Repo](https://github.com/nousresearch/hermes-agent))

| | Hermes | JaBot |
|---|---|---|
| Identity | One runtime; profiles are independent agents | Messenger managing Chief + crew across harnesses |
| Harness | Hermes *is* the harness | Wraps Claude/Codex/Pi/**Hermes**/Custom |
| UI | Ships CLI, TUI, Electron desktop, dashboard, gateways | Owns chat-first shell + Inbox |
| Protocol | Exposes ACP, Hermes JSON-RPC, HTTP | Consumes ACP as the waist |

Lesson: adopt patterns (profiles, Bot Mode, memory, skills, outbox) without
making Hermes `state.db` JaBot’s universal store.

Hermes ships `hermes claw migrate` from OpenClaw (`SOUL.md`, `MEMORY.md`,
`USER.md`, MCP, messaging, approvals). OpenClaw multi-agent ≈ Hermes profiles.
JaBot should not inherit either product’s “connect every chat platform” scope.

Hermes can PTY-drive Claude Code via terminal/tmux; that is **not** JaBot’s
integration path — ACP remains the decision.

## 2. Architecture

```text
Classic CLI · Ink TUI · Electron desktop (hermes serve)
IDE / JaBot ACP host (hermes acp)
HTTP + SSE API server
Telegram/Discord/… → GatewayRunner
        │
        ▼
  AIAgent / run_agent.py
  config · tools · providers · state.db
```

- **Messaging gateway:** `gateway/run.py` — adapters, auth, routing, cron, delivery.
- **TUI gateway:** `tui_gateway/server.py` — Hermes-specific JSON-RPC (session,
  slash commands, approvals, steer).
- **ACP adapter:** `acp_adapter/` — stdio JSON-RPC.
- **API server:** OpenAI-compatible HTTP plus Hermes Runs API.
- **Desktop:** Electron + React; launches `hermes serve`. Does **not** embed the
  Ink TUI.
- **Web dashboard Chat tab:** PTY + xterm.js around `hermes --tui` — **wrong
  precedent** for JaBot.

One core, many transports. JaBot equivalent: one crew/thread lifecycle, many
ACP harnesses and device transports.
([Architecture](https://hermes-agent.nousresearch.com/docs/developer-guide/architecture),
[Agent loop](https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop))

## 3. Setup / install / config

Tier-1 install:

```bash
curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash
```

Desktop installers on Apple Silicon macOS and Windows. Docker is tier 1.
General `pip` / Homebrew / AUR are **unsupported** even though ACP registry
flows have used `uvx`. JaBot should discover a managed `hermes` binary, not
install Hermes from PyPI.
([Installation](https://hermes-agent.nousresearch.com/docs/getting-started/installation))

```text
~/.hermes/hermes-agent/     source checkout
~/.local/bin/hermes
~/.hermes/                  default profile
```

```bash
hermes setup                 # Quick (Nous Portal OAuth) | Full | Blank Slate
hermes setup --portal
hermes doctor
hermes acp --check
hermes acp --setup
```

**Blank Slate** enables only provider/model, file tools, and terminal — useful
least-privilege crew preset.

```text
~/.hermes/
├── config.yaml          settings
├── .env                 API keys / bot tokens
├── auth.json            OAuth pools
├── SOUL.md
├── memories/MEMORY.md, USER.md     (some older pages show these at profile root)
├── skills/
├── cron/
├── state.db
├── sessions/            compatibility artifacts — not SoT
├── mcp-tokens/          mode 0600
└── profiles/<name>/     same layout, isolated HERMES_HOME
```

```bash
hermes profile create researcher
hermes -p researcher chat
hermes -p researcher acp
```

Profiles are state isolation, **not** OS sandboxes. Host subprocesses keep the
real `HOME` by default (`terminal.home_mode: profile` is stricter). **Do not
run two writers against one profile.** One persistent ACP server per profile;
multiplex JaBot chats as ACP sessions.
([Configuration](https://hermes-agent.nousresearch.com/docs/user-guide/configuration),
[Profiles](https://hermes-agent.nousresearch.com/docs/user-guide/profiles))

## 4. Agent / bot model

Desktop **Bot Mode** is a UI over profiles: model pin, `SOUL.md`, memory,
skills/toolsets/MCP, credentials, avatar, cron routines, independent sessions,
and a canonical persistent “Bot Chat.” `/new` and `/reset` on that chat are
rewritten to `/compact`. Scratch sessions can still `/new`.
([Bot Mode](https://hermes-agent.nousresearch.com/docs/user-guide/bot-mode))

**Groups** (2–6 Bots): up to three serial rounds, `@` mentions, pass allowed,
ten-message cap, `@user` = needs you. Desktop-only — JaBot must own the
scheduler if Claude/Codex should join.

Local Bot DMs invoke another profile’s canonical chat via `hermes -p … -Q
--query-file`. That must **not** become JaBot’s internal crew protocol.

`delegate_task` = ephemeral child `AIAgent`s (fresh context, inherit-but-not-
broaden tools, default 3 concurrent / 50 iterations, results ownership-scoped).
**Running children do not survive process restart**; completed-but-undelivered
results can. Optional git worktree isolation. Named crew ≠ subagents.
([Delegation](https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation))

JaBot: Chief = dedicated profile/role with a canonical conversation; crew
templates instantiate Hermes profile **or** Claude/Codex/Pi; scratch work is
separate threads.

## 5. Sessions and lifecycle

Canonical store: `~/.hermes/state.db` (WAL). JSONL is export/compatibility, not
source of truth. FTS5 over messages. Compression flushes memory, summarizes
middle, creates a child session in a lineage.
([Session storage](https://hermes-agent.nousresearch.com/docs/developer-guide/session-storage))

```bash
hermes --continue
hermes --resume <session-id>
hermes --resume latest --in ./project
```

Background: `/background …` (separate session, delivers back); `delegate_task`;
`terminal(background=true)`; cron; TUI `prompt.background`. Gateway has a
durable delivery ledger that can resend unconfirmed responses after crash
(marks ambiguous duplicates).

Hermes has hidden-but-working Bots, “Active now,” unread, needs-user — not
JaBot’s exact fold. Combine into:

```text
visible → folded/running → waiting_for_user | completed | failed
        → Inbox → reopened → archived
```

Fold ≠ close the harness session. Track thread id, ACP session, process/profile
ownership, run id, last progress, approval state, completion outbox, unread.

## 6. ACP and other host protocols

```bash
hermes acp
hermes-acp
python -m acp_adapter
hermes acp --check
hermes acp --setup            # interactive provider/model
hermes acp --setup-browser    # ~400 MB Node/Chromium — optional
```

Package pins `agent-client-protocol==0.9.0`. Capabilities: session
create/load/resume/list/fork/cancel, streaming, thinking, tools, diffs,
terminal rendering, permission requests, cwd, model selection, session MCP.
Curated `hermes-acp` toolset: files, terminal, web, browser, memory, session
search, skills, todo, vision, `execute_code`, `delegate_task`. **Excludes**
messaging, cron management, TTS, image gen, clarify UI.
([ACP](https://hermes-agent.nousresearch.com/docs/user-guide/features/acp))

Auth: uses Hermes’s provider resolver. Advertises agent-managed auth when
credentials exist, plus terminal method id `hermes-setup`.

Recommended JaBot preset:

```json
{
  "id": "hermes",
  "command": "hermes",
  "args": ["acp"],
  "env": { "HERMES_ACP_SKIP_CONFIGURED_MCP": "1" }
}
```

Named crew: `args: ["-p", "researcher", "acp"]`. Skip-MCP skips global
`config.yaml` MCP while still accepting `session/new` MCP servers. Docs said
global MCP blocked startup; current `entry.py` starts discovery on a daemon
thread — latency story is partly stale; ownership semantics remain.

**Docs vs source:** ACP user guide says list/load/resume are process-scoped;
`acp_adapter/session.py` persists in `state.db` and restores after restart.
JaBot must keep its own thread→session map, feature-test `load_session`, never
mutate Hermes SQLite.

Permission choices: `allow_once` | `allow_session` | `allow_always` | `deny`.
Timeouts deny. “Always” writes Hermes’s permanent allowlist.

**Buzz warning (do not copy):** Buzz’s bridge answers Hermes permission
requests with `allow_once`. `approvals.mode: manual` does not fix it.
Owner-only access is Buzz’s mitigation — JaBot must surface the prompt, and a
folded thread becomes Inbox “Needs permission.”

| Protocol | Use in JaBot |
|---|---|
| ACP stdio | **Standard harness** |
| TUI gateway | Optional Hermes-specific admin later (`session.steer`, etc.) |
| HTTP API | Automation; weak for interactive permissions |
| Messaging gateway | Hermes product, not JaBot harness protocol |
| In-process `AIAgent` | Avoid |

([Programmatic integration](https://hermes-agent.nousresearch.com/docs/developer-guide/programmatic-integration))

## 7. Tools, MCP, skills, memory

Public “60+” vs source “70+ / ~28 toolsets” — version-dependent. Categories:
web, terminal, files, browser, vision, todo, memory/search, skills, code
exec, delegation, cron, MCP, plugins.

| JaBot capability | Hermes |
|---|---|
| Gmail / Calendar / Drive | Bundled `google-workspace` skill (`gws` CLI) |
| GitHub | Bundled `github/*` skills via `gh` |
| Terminal / Browser | Native toolsets |
| Notion | Bundled `notion` skill (`ntn`) |
| Slack | Messaging **adapter**, not a generic Slack tool |

Distinguish native tools, skills, MCP, messaging transports, and host-side app
actions.

MCP: stdio and HTTP/SSE; OAuth 2.1/PKCE; include/exclude globs; registered as
`mcp_<server>_<tool>`. Hermes sanitizes TAG Unicode and strips `_meta`.
([MCP](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp))

Skills: agentskills.io `SKILL.md` packages; progressive disclosure
(`skills_list` / `skill_view`); `/learn`; project skills need explicit repo
trust; scan for injection; `skills.write_approval` stages writes.
([Skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills))

**Memory is not officially a “four-layer architecture.”** Accurate parts:

1. `MEMORY.md` — bounded facts (~2200 chars)
2. `USER.md` — bounded user profile (~1375)
3. `state.db` FTS5 — unbounded transcripts
4. Optional external provider (Honcho, Mem0, …) — one at a time

Skills are separate procedural memory. Snapshots freeze into the system prompt
at session start (prefix cache); writes land immediately but are not visible
until rebuild. `memory.write_approval` can stage writes.
([Memory](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory))

## 8. Messaging gateway and remote

Long-running gateway: Telegram, Discord, Slack, WhatsApp, Signal, Matrix, email,
SMS, Teams, … `hermes gateway install` (user or `--system`). Token locks prevent
two profiles polling the same platform token.
([Messaging](https://hermes-agent.nousresearch.com/docs/user-guide/messaging/))

Unknown DM → 8-character pairing code; owner `hermes pairing approve …`. Default
deny. Copy the UX; implement device-key pairing (SAS, revoke), not reusable
bearer codes.

Desktop roster is the union of profiles on registered machines; a Bot’s state
stays on its owning machine. Remote protocols are fragmented (`hermes serve`,
dashboard WS, API server, GatewayRunner, peer keys). JaBot should present **one**
daemon contract and keep ACP local to the selected host.

## 9. Approvals and security

```yaml
approvals:
  mode: smart       # smart | manual | off
  timeout: 300
  cron_mode: deny
```

Interactive: once | session | always | deny. User `approvals.deny` cannot be
bypassed by YOLO. Timeouts deny. **Do not make an auxiliary LLM the security
authority** — annotate/escalate only.

`write_file`/`patch` can use `HERMES_WRITE_SAFE_ROOT`; **shell is not bound by
that**. Sandbox backends: local/ssh (approvals on), docker/singularity/modal/
daytona/vercel_sandbox (approvals skipped; container is the boundary).
([Security](https://hermes-agent.nousresearch.com/docs/user-guide/security))

Treat separately: can it run (policy), where can it damage (sandbox), which
secrets are visible, who initiated (user/bot/cron/remote).

## 10. App shell

Hermes Desktop: Electron main owns install/backend lifecycle; React renderer
speaks WebSocket JSON-RPC to `hermes serve`. Validates JaBot’s “own the chat
UI, structured protocol” decision. Dashboard PTY chat does not.

Electron vs Tauri does not change the agent architecture. Need: renderer-owned
presentation, host daemon, one ACP subprocess per Hermes profile, persist
events before render, local socket auth, remote transport terminating at the
JaBot daemon.

## 11. What JaBot should port

Crew as isolated definition; canonical relationship chats; one ACP process per
Hermes identity; fold as presentation; durable completion outbox; explicit
lifecycle including `unknown` after crash; ACP preset with `--check`/`--setup`
and skip ambient MCP; capability provenance; layered memory; agentskills
packages; bounded crew deliberation; durable crew vs ephemeral delegation;
routines as jobs; scoped approvals with folded resurfacing; least-privilege
templates; remote host abstraction; Active/unread/hidden affordances; Doctor
diagnostics.

Full numbered list in [findings.md](findings.md).

## 12. What JaBot should not port

Hermes profiles as the universal crew DB; reading `state.db`; in-process
`AIAgent`; PTY wrap; shell files as bot-to-bot protocol; 20-platform gateway;
profile-as-sandbox; full ACP terminal for every template; auto-approve when
folded; smart-approval as a boundary; silent skill/memory writes; unbounded
roundtables; assuming background work survives restart; HTTP API as the
interactive adapter; exposing Hermes dashboard/API/ACP to phones.

## 13. Open questions / risks

1. ACP `use_unstable_protocol=True` and pin `0.9.0` — need negotiation.
2. Persistence docs vs source.
3. Concurrent profile writers vs “one process per chat.”
4. Whether `platform_toolsets.acp` / `disabled_toolsets` actually strip terminal.
5. Host must not repeat Buzz `allow_once`.
6. Async MCP discovery after initialize — capability drift.
7. Opaque/changing model IDs — store ACP-returned IDs verbatim.
8. Bot Mode not exposed through ACP — JaBot implements crew itself.
9. Credential isolation (shared `HOME`, token pools).
10. Skill supply chain; memory correctness; cost multiplication (cron, groups,
    delegation); large optional Chromium install.

## 14. Sources

- https://hermes-agent.nousresearch.com/docs/
- https://hermes-agent.nousresearch.com/docs/user-guide/features/acp
- https://hermes-agent.nousresearch.com/docs/user-guide/bot-mode
- https://hermes-agent.nousresearch.com/docs/user-guide/profiles
- https://hermes-agent.nousresearch.com/docs/user-guide/configuration
- https://hermes-agent.nousresearch.com/docs/user-guide/security
- https://hermes-agent.nousresearch.com/docs/user-guide/features/memory
- https://hermes-agent.nousresearch.com/docs/user-guide/features/skills
- https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp
- https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation
- https://hermes-agent.nousresearch.com/docs/user-guide/desktop
- https://hermes-agent.nousresearch.com/docs/developer-guide/architecture
- https://hermes-agent.nousresearch.com/docs/developer-guide/programmatic-integration
- https://hermes-agent.nousresearch.com/docs/developer-guide/session-storage
- https://hermes-agent.nousresearch.com/docs/guides/migrate-from-openclaw
- https://github.com/NousResearch/hermes-agent
- https://raw.githubusercontent.com/NousResearch/hermes-agent/main/acp_adapter/entry.py
- https://raw.githubusercontent.com/NousResearch/hermes-agent/main/acp_adapter/session.py
