-- Lifecycle core (#15). Three gaps v1 left for the state machine to fill:
--
-- 1. `threads.resurfaced_reason` — v1 could say a thread came back but not why,
--    and the Inbox needs FAILED and STUCK to be distinguishable (they are two
--    different asks of the human: retry versus wait or cancel).
-- 2. `runs.acp_session_id` — a thread's many sequential runs share one ACP
--    session, until a resume mints a new one. Stamping the run says which.
-- 3. `session_receipts` — the compatibility fingerprint #21 compares against
--    when it resumes a session it did not spawn. Holding this in RAM is what
--    made Buzz lose sessions across a restart; it is a row here on purpose.

ALTER TABLE threads ADD COLUMN resurfaced_reason TEXT
  CHECK (resurfaced_reason IS NULL
         OR resurfaced_reason IN ('done', 'failed', 'stuck', 'needs_you'));

ALTER TABLE runs ADD COLUMN acp_session_id TEXT;

CREATE TABLE session_receipts (
  thread_id           TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  acp_session_id      TEXT NOT NULL,
  native_session_ref  TEXT,
  harness_id          TEXT NOT NULL,
  model               TEXT,
  cwd                 TEXT NOT NULL,
  tools_json          TEXT NOT NULL DEFAULT '[]',
  permission_mode     TEXT NOT NULL DEFAULT 'default',
  -- Digest of the five fields above. Cheap equality check on resume; the
  -- columns are kept alongside so a mismatch can name what drifted.
  fingerprint         TEXT NOT NULL,
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
);

-- The Inbox badge is a count of unread events, asked for on every render.
CREATE INDEX inbox_events_unread ON inbox_events(read_at, created_at);
