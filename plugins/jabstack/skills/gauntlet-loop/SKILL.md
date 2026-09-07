---
name: gauntlet-loop
description: Turn a short topic into a full "gauntlet" build — benchmarked against a real shipped product, subagent fan-out over every quality domain, a per-domain loop with an independent harsh critic, and blind side-by-side judging until every critic picks our version. Use when the user types /gauntlet-loop followed by a topic, or asks to "gauntlet" / "run the gauntlet on" something.
license: MIT
metadata:
  argument-hint: <topic to build at reference-grade quality>
  author: jabreeflor
  version: "1.0"
---
# Gauntlet Loop

Requirements: Requires a client with subagents, web access, and build tools, such as ChatGPT Work, Codex, Claude Code, or Cursor.

Topic: `$ARGUMENTS`

On clients that do not substitute `$ARGUMENTS`, use the topic from the user's request.
Use the client's available subagent tools. If the named `gauntlet-critic` agent is
unavailable, give a generic critic subagent the judging instructions from
`../../agents/gauntlet-critic.md`; its Claude-specific frontmatter does not apply.
If independent subagents or reference access are unavailable, explain the missing
capability and report any review as a self-review, never as an independent blind test.

Expand the topic into this prompt, then execute it:

> Build `<artifact>` at the level of `<the most recent version of a real shipped product>`.
> It should be utterly perfect, with every single thing done at the highest quality — from
> textures to physics to anything you could think of.
>
> Fan out subagents and have each tackle one domain individually. Loop on each domain and
> have a **separate** subagent check it and confirm it hits that bar. That separate subagent
> is a really harsh critic — if it doesn't hit the bar, keep going.
>
> Don't stop until each critic is utterly wowed compared to the real thing. It should
> literally compare them side by side **blind** and say which one is better. Loop until
> it's utterly perfect.

If `$ARGUMENTS` is empty, ask what to gauntlet. Otherwise the invocation is the permission —
start. Ask at most one clarifying question, only if the benchmark is genuinely ambiguous.

## Report

Short. Which one the blind critics picked, the final score, the per-domain scoreboard, the
tells that remain, a link to the artifact. Don't recount the rounds.
