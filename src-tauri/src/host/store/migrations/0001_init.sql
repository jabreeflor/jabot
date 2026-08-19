-- JaBot host store v1. Source of truth for threads, crew, Inbox, PRs,
-- schedules, harness catalog, and secret *references* (bytes live in the OS
-- keychain). See docs/research/data-and-persistence/schema.md and #5.

CREATE TABLE schema_migrations (
  version    INTEGER PRIMARY KEY,
  applied_at TEXT    NOT NULL
);

CREATE TABLE folders (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  path        TEXT NOT NULL UNIQUE,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

CREATE TABLE harnesses (
  id             TEXT PRIMARY KEY,
  label          TEXT NOT NULL,
  command        TEXT NOT NULL,
  args_json      TEXT NOT NULL DEFAULT '[]',
  env_json       TEXT NOT NULL DEFAULT '{}',
  install_hint   TEXT,
  is_builtin     INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

CREATE TABLE bots (
  id              TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  color           TEXT NOT NULL,
  instructions    TEXT NOT NULL DEFAULT '',
  tools_json      TEXT NOT NULL DEFAULT '[]',
  harness_id      TEXT NOT NULL REFERENCES harnesses(id),
  is_chief        INTEGER NOT NULL DEFAULT 0 CHECK (is_chief IN (0, 1)),
  template_id     TEXT,
  host_id         TEXT,
  sort_order      INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

CREATE UNIQUE INDEX bots_one_chief ON bots(is_chief) WHERE is_chief = 1;

CREATE TABLE threads (
  id                   TEXT PRIMARY KEY,
  folder_id            TEXT REFERENCES folders(id) ON DELETE SET NULL,
  bot_id               TEXT REFERENCES bots(id) ON DELETE SET NULL,
  harness_id           TEXT NOT NULL REFERENCES harnesses(id),
  acp_session_id       TEXT,
  native_session_ref   TEXT,
  cwd                  TEXT NOT NULL,
  runtime_json         TEXT NOT NULL,
  title                TEXT NOT NULL,
  state                TEXT NOT NULL DEFAULT 'active'
                         CHECK (state IN ('active', 'folded', 'resurfaced', 'archived')),
  fold_policy          TEXT NOT NULL DEFAULT 'default'
                         CHECK (fold_policy IN ('default', 'wait_for_inbox')),
  last_stop_reason     TEXT,
  last_error           TEXT,
  preview              TEXT,
  worktree_path        TEXT,
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL,
  folded_at            TEXT,
  resurfaced_at        TEXT,
  archived_at          TEXT,
  deleted_at           TEXT
);

CREATE UNIQUE INDEX threads_acp_session ON threads(acp_session_id)
  WHERE acp_session_id IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX threads_folder_state ON threads(folder_id, state) WHERE deleted_at IS NULL;
CREATE INDEX threads_inbox ON threads(state, resurfaced_at) WHERE deleted_at IS NULL;

CREATE TABLE runs (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  seq           INTEGER NOT NULL,
  kind          TEXT NOT NULL
                  CHECK (kind IN ('prompt', 'schedule', 'handoff', 'resume')),
  state         TEXT NOT NULL DEFAULT 'queued'
                  CHECK (state IN (
                    'queued', 'running', 'succeeded', 'failed',
                    'cancelled', 'timed_out', 'lost', 'needs_you'
                  )),
  trigger_json  TEXT,
  error         TEXT,
  started_at    TEXT,
  ended_at      TEXT,
  created_at    TEXT NOT NULL,
  UNIQUE (thread_id, seq)
);

CREATE INDEX runs_thread_state ON runs(thread_id, state);
CREATE INDEX runs_state ON runs(state, ended_at);

CREATE TABLE transcript_events (
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  seq           INTEGER NOT NULL,
  acp_method    TEXT NOT NULL,
  payload_json  TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  PRIMARY KEY (thread_id, seq)
);

CREATE TABLE inbox_events (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  run_id        TEXT REFERENCES runs(id) ON DELETE SET NULL,
  kind          TEXT NOT NULL
                  CHECK (kind IN (
                    'folded', 'done', 'failed', 'needs_you',
                    'judgment_call', 'permission', 'lost', 'stuck'
                  )),
  title         TEXT NOT NULL,
  summary       TEXT NOT NULL DEFAULT '',
  payload_json  TEXT,
  created_at    TEXT NOT NULL,
  read_at       TEXT,
  dismissed_at  TEXT
);

CREATE INDEX inbox_events_thread ON inbox_events(thread_id, created_at);

CREATE TABLE thread_prs (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  provider      TEXT NOT NULL DEFAULT 'github',
  repo          TEXT NOT NULL,
  number        INTEGER NOT NULL,
  url           TEXT NOT NULL,
  status        TEXT NOT NULL DEFAULT 'open'
                  CHECK (status IN ('open', 'draft', 'merged', 'closed')),
  check_state   TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  UNIQUE (provider, repo, number)
);

CREATE INDEX thread_prs_thread ON thread_prs(thread_id);

CREATE TABLE schedules (
  id             TEXT PRIMARY KEY,
  bot_id         TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
  title          TEXT NOT NULL,
  cron           TEXT NOT NULL,
  prompt         TEXT NOT NULL,
  enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  last_run_at    TEXT,
  next_run_at    TEXT,
  last_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);

CREATE INDEX schedules_next ON schedules(enabled, next_run_at);

CREATE TABLE permission_decisions (
  id            TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  scope         TEXT NOT NULL CHECK (scope IN ('once', 'session', 'always')),
  kind          TEXT NOT NULL,
  subject_json  TEXT NOT NULL,
  created_at    TEXT NOT NULL
);

CREATE TABLE secret_refs (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL,
  label         TEXT NOT NULL,
  account       TEXT NOT NULL UNIQUE,
  bot_id        TEXT REFERENCES bots(id) ON DELETE SET NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE TABLE app_meta (
  key           TEXT PRIMARY KEY,
  value         TEXT NOT NULL
);
