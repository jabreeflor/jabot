-- Structured questions and plan decisions on the permission ledger (#298).
--
-- Cursor's ACP bridge sends two requests that block its turn on a human and
-- are not permissions: `cursor/ask_question` (choose among the agent's
-- options) and `cursor/create_plan` (accept or reject a plan). They have the
-- same lifecycle as an ask — on disk before they are announced, answered
-- exactly once, honest about whether the agent heard — so they share the
-- ledger rather than getting a second broker. What changes is the row:
--
-- `ask` says which sort of question this is. `permission` is every row that
-- existed before this migration, and the only value the fold policy, the
-- Inbox's "answerable" merge, and `permission/pending` will ever read. The
-- other two are listed by `interaction/pending` and nowhere else, which is
-- what keeps a plan decision from ever being drawn as a permission grant.
--
-- `method` is the ACP method the ask arrived on, kept for diagnostics.
-- `answer_json` is what was sent back, in the host's own shape — the chosen
-- option ids for a question, accept/reject and a reason for a plan.
--
-- Two states join the three the broker had. `expired`: the turn ended with the
-- ask still open, so the agent stopped waiting and nothing can be delivered.
-- `unavailable`: the process that asked is gone (adapter died, host quit).
-- Both exist so a card can say why it cannot be answered instead of offering
-- buttons that reach nobody. A permission row never enters either: #20's
-- promise that a quit keeps the question `pending` for the next launch is
-- kept exactly as it was, because a permission decision is still worth
-- recording after the fact and a question's answer is not.
--
-- SQLite cannot widen a CHECK in place, so the table is rebuilt.

CREATE TABLE permission_requests_next (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id        TEXT REFERENCES runs(id) ON DELETE SET NULL,
  kind          TEXT,
  title         TEXT NOT NULL,
  subject_json  TEXT NOT NULL,
  options_json  TEXT NOT NULL DEFAULT '[]',
  state         TEXT NOT NULL DEFAULT 'pending'
                  CHECK (state IN ('pending', 'answered', 'cancelled', 'expired', 'unavailable')),
  decided_by    TEXT,
  option_id     TEXT,
  delivered     INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
  created_at    TEXT NOT NULL,
  resolved_at   TEXT,
  ask           TEXT NOT NULL DEFAULT 'permission'
                  CHECK (ask IN ('permission', 'question', 'plan')),
  method        TEXT,
  answer_json   TEXT
);

INSERT INTO permission_requests_next (
  id, thread_id, run_id, kind, title, subject_json, options_json, state,
  decided_by, option_id, delivered, created_at, resolved_at, ask, method
)
SELECT
  id, thread_id, run_id, kind, title, subject_json, options_json, state,
  decided_by, option_id, delivered, created_at, resolved_at, 'permission',
  'session/request_permission'
FROM permission_requests;

DROP TABLE permission_requests;
ALTER TABLE permission_requests_next RENAME TO permission_requests;

CREATE INDEX permission_requests_open ON permission_requests(thread_id, state);
