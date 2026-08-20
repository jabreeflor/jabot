-- Recurring jobs that fire into the Inbox (#25).
--
-- `schedules` is not new: #9 laid a placeholder down in `0001_init.sql` and
-- nothing has ever written a row into it. This migration finishes it rather
-- than putting a second table beside it, and adds the fire log it needs.
--
-- Decision #4 keeps the host inside the Tauri binary: there is no launchd job
-- and no daemon in MVP1, so a schedule is only ever evaluated while JaBot is
-- running. That single fact is what the columns are shaped around.
--
-- `next_fire_at` (was `next_run_at`) is the **armed slot**, in UTC, and it is
-- the whole of the missed-fire policy. A host that is running re-arms it
-- seconds after each fire, so it stays within one cron step of now. A host
-- that was closed for a week comes back to find it a week in the past — and
-- the distance between it and now is exactly "what we missed while we were
-- away". Nothing else has to be remembered, and no queue of pending fires can
-- build up on disk waiting to stampede at launch.
--
-- `catch_up` decides what that distance means. `1` (the default) fires **once**
-- for the oldest missed slot and drops the rest; `0` fires nothing and simply
-- re-arms. Neither replays. Firing a week of missed dailies on launch is the
-- failure this column exists to make unrepresentable.
--
-- The renames are the placeholder's vocabulary catching up with the ledger's.
-- A schedule produces a *fire*, and a fire produces a `runs` row — calling the
-- armed slot `next_run_at` while `schedule_fires.run_id` points at something
-- else would be two meanings of "run" in one join. Safe to rename because no
-- code path in any shipped issue has ever inserted here.

ALTER TABLE schedules RENAME COLUMN title TO name;
ALTER TABLE schedules RENAME COLUMN next_run_at TO next_fire_at;
ALTER TABLE schedules RENAME COLUMN last_run_at TO last_fired_at;
-- Superseded by `schedule_fires.thread_id`, which says the same thing per
-- fire instead of only for the most recent one.
ALTER TABLE schedules DROP COLUMN last_thread_id;
ALTER TABLE schedules ADD COLUMN catch_up INTEGER NOT NULL DEFAULT 1
  CHECK (catch_up IN (0, 1));

DROP INDEX schedules_next;
CREATE INDEX schedules_due ON schedules(enabled, next_fire_at);
CREATE INDEX schedules_bot ON schedules(bot_id, created_at);

-- The trace: one row per fire, written *before* the bot is prompted, in the
-- same spirit as `handoffs` (#24) and `permission_requests` (#20). A fire whose
-- harness will not start still happened, and the row says so with a reason
-- attached. It is also where "delivered to the Inbox exactly once" is durable
-- rather than a fact held in RAM: `delivered_at` survives the quit that a
-- half-finished fire would otherwise be lost to.
CREATE TABLE schedule_fires (
  id             TEXT PRIMARY KEY,
  schedule_id    TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
  -- Both nullable, and for the reason `handoffs.delivered` exists: the row is
  -- written before the prompt, so a fire that never reached an agent still has
  -- somewhere to record why.
  thread_id      TEXT REFERENCES threads(id) ON DELETE SET NULL,
  run_id         TEXT REFERENCES runs(id) ON DELETE SET NULL,
  -- The slot this fire is *for*, and when the host actually got to it. They
  -- differ by a second on a running host and by days on one that was closed.
  due_at         TEXT NOT NULL,
  fired_at       TEXT NOT NULL,
  caught_up      INTEGER NOT NULL DEFAULT 0 CHECK (caught_up IN (0, 1)),
  -- Occurrences between `due_at` and `fired_at` that were deliberately
  -- dropped. The number the card shows, so a catch-up cannot pretend it was
  -- the only thing that was due.
  skipped        INTEGER NOT NULL DEFAULT 0,
  -- `transcript_events.seq` at the moment of firing, so the card can quote
  -- what the agent said *this time* rather than the whole standing thread.
  transcript_seq INTEGER NOT NULL DEFAULT 0,
  outcome        TEXT CHECK (outcome IN (
                   'done', 'failed', 'lost', 'cancelled', 'not_started', 'skipped'
                 )),
  detail         TEXT,
  inbox_event_id TEXT REFERENCES inbox_events(id) ON DELETE SET NULL,
  delivered_at   TEXT
);

CREATE INDEX schedule_fires_undelivered ON schedule_fires(delivered_at, fired_at);
CREATE INDEX schedule_fires_schedule ON schedule_fires(schedule_id, fired_at DESC);
