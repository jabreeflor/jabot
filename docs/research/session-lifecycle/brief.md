# Session Lifecycle (Disappearing Threads)

The signature feature: a long-running thread "folds away" (disappears from the
sidebar), keeps working in the background, and resurfaces in the Inbox when it
finishes, fails, or needs a human call.

Depends on: [harness-integration](../harness-integration/brief.md) (what a
session even is per harness).

## Questions to answer

1. **Keeping sessions alive** — when a thread folds, does the harness process
   keep running (detached / background job), or do we checkpoint and resume?
   What do Claude Code / Codex support for resume (`--resume`, `codex resume`)?
2. **Resurface triggers** — how do we detect: done, failed, stuck, "needs a
   judgment call"? Structured completion events vs idle-detection vs the agent
   explicitly signaling.
3. **State machine** — define it: active → folded (sleeping) → resurfaced →
   reviewed/archived. Plus right-click states: Wait for Inbox, Archive, Delete.
   What transitions are legal, what data each state carries.
4. **Crash / laptop-sleep survival** — sessions must survive app restart and
   machine sleep. Supervisor process? launchd? Just resume-on-reopen?
5. **Notifications** — when something resurfaces while the app is closed or
   backgrounded, do we notify (macOS notifications)? What's the noise budget?
6. **Judgment calls while away** — prototype shows "1 judgment call made while
   you were away." How do we capture decisions the agent made autonomously so
   they're reviewable in the Inbox?

## What this blocks (future issues)

- Thread state machine + store
- Background session supervisor
- Inbox view (real data): resurfaced + still-sleeping sections
- Fold/Wait-for-Inbox actions wired to real sessions
- Native notifications
