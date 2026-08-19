# Setup Porting — OpenClaw, Hermes Agent, Buzz

**Prior art for the whole product, not a new harness.** JaBot already decided
to speak ACP ([harness-integration findings](../harness-integration/findings.md)).
This topic asks what else to copy from the three products that already ship
personal-agent / agent-workspace setups:

| Source | What it is | Why it is here |
|---|---|---|
| [OpenClaw](https://docs.openclaw.ai/) | Personal assistant gateway: one daemon, many channels, multi-agent bindings, ACP host *and* ACP agent | Closest match to JaBot's Chief + crew + remote host |
| [Hermes Agent](https://hermes-agent.nousresearch.com/docs/) | Provider-agnostic agent runtime with profiles, Bot Mode, memory, skills, cron, ACP | Closest match to named crew bots + memory/skills; also a JaBot harness preset |
| [Buzz](https://github.com/block/buzz) | Team workspace (Nostr relay + Tauri desktop) with ACP supervisor and BYOH catalog | Named prior art for the harness seam, process supervisor, pairing |

Copy the *seams*, not the products. JaBot stays a personal messenger with Inbox;
it does not become a WhatsApp gateway, a 20-platform bot bridge, or a Nostr team
workspace.

Deep dives: [openclaw.md](openclaw.md), [hermes.md](hermes.md), [buzz.md](buzz.md).
Synthesis: [findings.md](findings.md).

## Questions to answer

1. **Host vs UI** — do all three force a daemon/client split, and should JaBot
   from day one?
2. **What is a bot?** — OpenClaw agent vs Hermes profile/Bot vs Buzz persona.
   Mapping onto Chief of Staff + crew templates.
3. **Sessions vs tasks vs UI fold** — which of those three states do they keep
   separate, and what does Inbox consume?
4. **Harness catalog** — commands, env, probes, custom JSON. Hermes and OpenClaw
   as JaBot presets, not just Buzz's table.
5. **Permissions** — interactive ACP prompts vs auto-approve. What *not* to copy
   (Buzz `bypass-permissions`, Buzz's Hermes `allow_once`).
6. **Memory, skills, tools** — which layers are portable vs harness-private.
7. **Pairing / remote** — QR + SAS without adopting Nostr or shipping a channel
   catalog.
8. **Persistence** — SQLite vs files vs relay. Secrets. Transcript ownership.

## What this blocks (future issues)

- Host daemon vs in-process UI (feeds [app-shell](../app-shell/brief.md) and
  [remote-and-mobile](../remote-and-mobile/brief.md))
- Crew template schema (feeds [bot-crew](../bot-crew/brief.md))
- Thread / run / Inbox state split (feeds
  [session-lifecycle](../session-lifecycle/brief.md))
- Harness catalog + Doctor (feeds
  [harness-integration](../harness-integration/brief.md) — extends the
  already-answered ACP decision)
- SQLite + keychain layout (feeds
  [data-and-persistence](../data-and-persistence/brief.md))
- Device pairing without master-secret copy (feeds remote-and-mobile)
