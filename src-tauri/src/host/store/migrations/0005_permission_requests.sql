-- The permission broker's ledger (#20).
--
-- `session/request_permission` arrives on a live ACP call, and the JSON-RPC id
-- it carries is worth nothing to the next process: the adapter that would have
-- read the answer is dead the moment the host stops. Everything a human needs
-- to be asked again — which thread, what the agent wanted, which options it
-- offered — therefore has to be on disk before the ask is announced, or a quit
-- while a request is outstanding loses the question itself. #21 already
-- resurfaces such a thread as `needs_you` off the run ledger; this is the row
-- that lets the card say what the agent actually asked for, and lets the human
-- answer it.
--
-- Every ask gets a row, including the ones Wait for Inbox answers on the
-- user's behalf (#5) — a record of what the host decided while the user was
-- away is worth exactly as much as a record of what it asked.
--
-- `delivered` is the honest half: an answer to a request whose adapter is gone
-- is recorded, and the agent never hears it. Nothing here re-plays a dead RPC.
--
-- `permission_decisions` (0001) is a different table for a different question —
-- remembered "always allow" scopes — and stays unused until something offers
-- that scope.

CREATE TABLE permission_requests (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id        TEXT REFERENCES runs(id) ON DELETE SET NULL,
  -- The ACP tool kind the ask is about (`read`, `execute`, `delete`, …), when
  -- the agent named one. It is what the fold policy reads, so it is stored
  -- rather than re-derived from the subject blob on every read.
  kind          TEXT,
  title         TEXT NOT NULL,
  subject_json  TEXT NOT NULL,
  options_json  TEXT NOT NULL DEFAULT '[]',
  state         TEXT NOT NULL DEFAULT 'pending'
                  CHECK (state IN ('pending', 'answered', 'cancelled')),
  -- Which device answered, or 'host' when the fold policy did.
  decided_by    TEXT,
  option_id     TEXT,
  delivered     INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
  created_at    TEXT NOT NULL,
  resolved_at   TEXT
);

CREATE INDEX permission_requests_open ON permission_requests(thread_id, state);
