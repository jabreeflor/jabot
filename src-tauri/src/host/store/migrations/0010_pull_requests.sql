-- The PR surface: what GitHub says about a pull request a session opened (#28).
--
-- `thread_prs` has existed since 0001 with the linkage half — thread, repo,
-- number, url — and nothing else. That is enough to *find* a PR and not enough
-- to *draw* one: the prototype's row wants a title, a diffstat, a head and base
-- branch, a checks line and a review verdict, and re-fetching all of that from
-- GitHub on every repaint would be a network round trip per render on a surface
-- the user leaves open.
--
-- So the poll writes what it learned and the view reads the store. Every column
-- here is a cached copy of a GitHub fact, refreshed by `pr/refresh` and stale
-- in exactly the way a poll is stale — `polled_at` is what says how stale.
-- Nothing here is authoritative and nothing here is a credential: the token is
-- the user's `gh` login, read on demand and never persisted (#16).
--
-- The dedupe key stays `UNIQUE (provider, repo, number)` from 0001. It is not
-- widened to include `forge_host` even though the column is added below: a
-- UNIQUE constraint cannot be altered in SQLite without rebuilding the table,
-- and the one case the wider key would catch — the same `owner/name` on
-- github.com and on a GHES host, both linked to threads on this Mac — is rarer
-- than the cost of rewriting a table every existing row already lives in.
-- `forge_host` is here because `gh` is addressed per host (`--hostname`), not
-- because identity needs it.

ALTER TABLE thread_prs ADD COLUMN forge_host    TEXT;
ALTER TABLE thread_prs ADD COLUMN title         TEXT NOT NULL DEFAULT '';
ALTER TABLE thread_prs ADD COLUMN head_ref      TEXT;
ALTER TABLE thread_prs ADD COLUMN base_ref      TEXT;
ALTER TABLE thread_prs ADD COLUMN additions     INTEGER NOT NULL DEFAULT 0;
ALTER TABLE thread_prs ADD COLUMN deletions     INTEGER NOT NULL DEFAULT 0;
ALTER TABLE thread_prs ADD COLUMN changed_files INTEGER NOT NULL DEFAULT 0;
-- GitHub's `reviewDecision`: 'approved' | 'changes_requested' | 'review_required'.
-- NULL is a real answer and not a missing one — a repository with no review
-- rules returns null for a PR nobody has looked at.
ALTER TABLE thread_prs ADD COLUMN review_state  TEXT;
-- `statusCheckRollup.contexts` flattened to `[{label,state}]`, which is exactly
-- the checks line the row draws. Kept as JSON rather than a table: it is one
-- render's worth of data, replaced wholesale on every poll, and never queried.
ALTER TABLE thread_prs ADD COLUMN checks_json   TEXT NOT NULL DEFAULT '[]';
-- When GitHub last saw the PR change, which is not when we last wrote the row.
-- `updated_at` is ours; the row moves when a poll finds nothing new, and the
-- "38m ago" on the card must not.
ALTER TABLE thread_prs ADD COLUMN pr_updated_at TEXT;
-- How we came to believe this thread opened this PR: 'stdout' (a PR URL in an
-- execute tool call), 'gh-pr-view' (asked `gh` for the branch's PR), or
-- 'head-list' (matched an open PR by head branch). Kept because the three are
-- not equally trustworthy and a wrong link is worth being able to explain.
ALTER TABLE thread_prs ADD COLUMN detected_via  TEXT;
ALTER TABLE thread_prs ADD COLUMN detected_at   TEXT;
-- Last successful refresh from the API. NULL means "linked, never polled",
-- which is what every row looks like on a machine with no `gh` login.
ALTER TABLE thread_prs ADD COLUMN polled_at     TEXT;

-- The poll's own read: every linked PR, newest first, so one query per refresh
-- can be built from one scan.
CREATE INDEX thread_prs_polled ON thread_prs(polled_at);

-- `inbox_events.kind` gains 'pr'.
--
-- A card about a pull request is not a card about a run, and reusing one of the
-- run kinds would make it one. `failed` over a green run whose CI later went
-- red claims the *turn* failed; `needs_you` over a PR nobody is blocked on
-- claims an agent is waiting. Both are lies the Inbox would then have to draw.
-- The kind is a closed list enforced by a CHECK constraint, and SQLite cannot
-- alter one — so the table is rebuilt. Nothing references `inbox_events`, so
-- this is a copy, a drop and a rename with no foreign key to re-point.
CREATE TABLE inbox_events_new (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id        TEXT REFERENCES runs(id) ON DELETE SET NULL,
  kind          TEXT NOT NULL
                  CHECK (kind IN (
                    'folded', 'done', 'failed', 'needs_you',
                    'judgment_call', 'permission', 'lost', 'stuck', 'pr'
                  )),
  title         TEXT NOT NULL,
  summary       TEXT NOT NULL DEFAULT '',
  payload_json  TEXT,
  created_at    TEXT NOT NULL,
  read_at       TEXT,
  dismissed_at  TEXT
);

INSERT INTO inbox_events_new (
  id, thread_id, run_id, kind, title, summary, payload_json,
  created_at, read_at, dismissed_at
)
SELECT id, thread_id, run_id, kind, title, summary, payload_json,
       created_at, read_at, dismissed_at
  FROM inbox_events;

DROP TABLE inbox_events;
ALTER TABLE inbox_events_new RENAME TO inbox_events;

CREATE INDEX inbox_events_thread ON inbox_events(thread_id, created_at);
