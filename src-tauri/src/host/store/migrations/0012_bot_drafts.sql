-- Conversational bot drafts (#237). A draft is a proposal; Save is a separate
-- full-device action. Source ids are text, not foreign keys: deleting the
-- requesting bot must not delete the reviewable record, and name equality is
-- never the uniqueness key.
--
-- Tool grants for existing Chief / Recruiter installs are applied in
-- `seed::upgrade_draft_tools`, not here. Editing SEED_BOTS only affects an
-- empty crew; this table is the durable half of the new contract.

CREATE TABLE bot_drafts (
  id                TEXT PRIMARY KEY,
  request_key       TEXT NOT NULL,
  payload_hash      TEXT NOT NULL,
  source_bot_id     TEXT NOT NULL,
  source_thread_id  TEXT,
  source_run_id     TEXT,
  name              TEXT NOT NULL,
  instructions      TEXT NOT NULL,
  tools_json        TEXT NOT NULL DEFAULT '[]',
  harness_id        TEXT NOT NULL,
  color             TEXT NOT NULL,
  template_id       TEXT,
  status            TEXT NOT NULL DEFAULT 'pending_review'
                    CHECK (status IN ('pending_review', 'saved', 'dismissed', 'stale')),
  revision          INTEGER NOT NULL DEFAULT 1,
  bot_id            TEXT,
  deciding_device_id TEXT,
  stale_reason      TEXT,
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL
);

CREATE UNIQUE INDEX bot_drafts_source_request
  ON bot_drafts (source_bot_id, request_key);
