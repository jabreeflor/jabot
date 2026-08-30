# Schedules: in-process cron jobs delivered to Inbox

**Issue:** #25
**Status:** Implemented — `src-tauri/src/host/schedule/`, `src/components/ScheduleEditorModal.tsx`, `src/views/schedules.ts`

## What it is

Recurring, cron-defined jobs that fire a crew bot on a schedule (e.g.
"every weekday at 9am, ask Inbox Manager to triage") with results
delivered as new runs and Inbox events, without any external scheduler.

## Why

Some useful bot work is time-triggered, not chat-triggered. Running the
scheduler in-process (in the host) keeps this consistent with the
"resume, don't rely on a living background process" lifecycle policy
(see [desktop-host-lifecycle.md](desktop-host-lifecycle.md)) instead of
depending on launchd or an external cron.

## Requirements

1. A schedule is user-defined data: a cron expression, a target crew
   bot, and a prompt/task (`src-tauri/src/host/schedule/api.rs`), edited
   via `ScheduleEditorModal.tsx`.
2. `cron.rs` parses/evaluates the cron expression to determine next-fire
   times; `fire.rs` performs the actual firing — starting a new run on
   the target bot's thread (or a new thread if the schedule isn't bound
   to an existing one) per
   [thread-state-and-runs.md](thread-state-and-runs.md).
3. Because the host is not guaranteed to be running at every scheduled
   moment (per the Quit/hide-to-Dock policy), `catchup.rs` reconciles
   missed fires on next boot — a schedule that should have fired while
   the app was quit either fires once on resume or is explicitly marked
   skipped, never silently vanishes without record.
4. A fired schedule's resulting run flows into the same run ledger and
   Inbox projection as any manually-started run (see
   [inbox.md](inbox.md)) — schedules are not a separate notification
   path.
5. Schedules persist via the data layer (`store/schedule.rs`) and
   survive app restart with their next-fire time intact.
6. A user can enable/disable or delete a schedule; a disabled schedule
   does not fire (including on catch-up) until re-enabled.
7. Schedule editing/listing is covered by
   `src/__tests__/schedules.test.tsx` against the mock host.
