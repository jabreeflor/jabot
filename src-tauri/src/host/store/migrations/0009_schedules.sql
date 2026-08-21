-- Schedules: the fires, and the one column the sketch was missing (#25).
--
-- `0001_init.sql` already shipped a `schedules` table — the schema sketch put
-- it there long before anything could run one. This migration keeps that table
-- (a row in it is a user's job, and dropping it to build a prettier one would
-- be renaming a table nobody has data in *yet* and might tomorrow) and adds the
-- two things a working cron needs it to have.
--
-- **`catch_up`.** Decision #4 keeps the host inside the Tauri binary, so the
-- clock stops every time the user quits. `next_run_at` was therefore always
-- going to be in the past on some launch, and the sketch had nowhere to record
-- what the user wanted done about that. `once` runs the most recent missed
-- occurrence and drops the rest; `skip` runs none. There is deliberately no
-- value that replays a backlog.
--
-- **`schedule_fires`.** One row per occurrence. It is a separate table because
-- a schedule and a firing have different lifetimes: the fire outlives the tick
-- that produced it, because its result has to be delivered later — possibly by
-- a different host process, after a restart.

ALTER TABLE schedules ADD COLUMN catch_up TEXT NOT NULL DEFAULT 'once'
  CHECK (catch_up IN ('once', 'skip'));

CREATE TABLE schedule_fires (
  id            TEXT PRIMARY KEY,
  schedule_id   TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
  -- Both nullable, and for the same reason: a fire is recorded *before* it is
  -- dispatched (decision #5's persist-then-notify, applied to work instead of
  -- to cards), so a fire that never reached an agent is still a fire the user
  -- can see. `ON DELETE SET NULL` keeps that history when the thread goes.
  thread_id     TEXT REFERENCES threads(id) ON DELETE SET NULL,
  run_id        TEXT REFERENCES runs(id) ON DELETE SET NULL,
  -- The occurrence this fire is for, not the moment it ran. They differ by
  -- however long the Mac was shut.
  due_at        TEXT NOT NULL,
  fired_at      TEXT NOT NULL,
  state         TEXT NOT NULL DEFAULT 'dispatched'
                  CHECK (state IN ('dispatched', 'skipped', 'failed', 'delivered')),
  -- True when this occurrence was already in the past when the host ruled on
  -- it — the catch-up case, as opposed to a tick that landed on time.
  caught_up     INTEGER NOT NULL DEFAULT 0 CHECK (caught_up IN (0, 1)),
  -- How many earlier occurrences were dropped in favour of this one. This is
  -- the number that makes "JaBot was shut for a week" legible without writing
  -- seven rows and running seven jobs.
  skipped_count INTEGER NOT NULL DEFAULT 0,
  detail        TEXT,
  -- When the Inbox card for this fire was written. NULL means the run has not
  -- finished yet, which is exactly what the delivery pass looks for.
  delivered_at  TEXT,
  -- One fire per occurrence, ever. This is what makes catch-up idempotent: two
  -- ticks, a boot pass overlapping a live tick, or a Run now on a schedule that
  -- is already due cannot turn one 9am into two runs.
  UNIQUE (schedule_id, due_at)
);

CREATE INDEX schedule_fires_undelivered ON schedule_fires(state, delivered_at);
CREATE INDEX schedule_fires_schedule ON schedule_fires(schedule_id, fired_at);
