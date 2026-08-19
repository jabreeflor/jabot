# JaBot Research

Research needed before we open build issues. Each topic has its own folder with a
`brief.md` (questions to answer). Findings get added as more `.md` files in the
same folder. When a topic's questions are answered, we turn its "What this blocks"
list into GitHub issues with dependencies flagged.

## Topics, in dependency order

| # | Topic | Why it's first/later |
|---|-------|----------------------|
| 1 | [harness-integration](harness-integration/brief.md) | The core bet. Everything depends on how we wrap the TUIs. |
| 2 | [session-lifecycle](session-lifecycle/brief.md) | Disappearing Threads / Inbox. Depends on #1 (what a "session" is per harness). |
| 3 | [app-shell](app-shell/brief.md) | Electron vs Tauri vs web. Constrained by #1 (process management needs). |
| 4 | [bot-crew](bot-crew/brief.md) | Chief of Staff routing + bot templates + tools. Depends on #1. |
| 5 | [git-and-prs](git-and-prs/brief.md) | Worktrees, PR view. Mostly independent; light dependency on #2. |
| 6 | [data-and-persistence](data-and-persistence/brief.md) | Storage for threads, transcripts, crew config. Depends on #1–#2 shapes. |
| 7 | [remote-and-mobile](remote-and-mobile/brief.md) | Bots hosted on other machines + mobile pairing (MVP2 feature, but the client/host split is an MVP1 decision). Depends on #1–#2. |

## Product source of truth

`prototypes/jabot-classic.html` is the MVP prototype. Concepts it defines:
Chief of Staff bot, crew of template bots, harness selection at new-chat time
(Claude Code / Codex / Pi / Custom), folders of code threads, Disappearing
Threads (fold away → Inbox), Pull Requests view, right-click actions
(Wait for Inbox / Archive / Delete).
