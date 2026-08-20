-- Chief's handoff ledger (#24).
--
-- Decision #6 puts routing in the host: "Routing is still a host action
-- (handoff), not a nested subagent." A host action that leaves no record is a
-- job the user cannot trace — they open the Writer's chat, find a task they
-- never typed, and have no way to learn who asked for it or why.
--
-- So every `handoff_to_bot` writes a row *before* the receiving bot is
-- prompted. The row is the durable half of provenance; the receiving thread
-- also gets a transcript line saying the same thing, because the chat is where
-- a human actually looks. The two are written together and neither is derived
-- from the other: the transcript is a log the reducer replays, this is a table
-- `list_crew_status` can join on to say "Writer — handed off by Chief".
--
-- `delivered` is the honest half, in the same spirit as `permission_requests`:
-- a handoff whose target harness is not installed still happened, and the row
-- says it was never delivered rather than quietly not existing.
--
-- `from_thread_id` / `from_bot_id` are nullable because a handoff can come
-- from the user driving `chief/invoke` directly, and because removing a bot
-- from the crew must not delete the history of the work it routed.

CREATE TABLE handoffs (
  id             TEXT PRIMARY KEY,
  from_thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
  from_bot_id    TEXT REFERENCES bots(id) ON DELETE SET NULL,
  to_thread_id   TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  to_bot_id      TEXT REFERENCES bots(id) ON DELETE SET NULL,
  task           TEXT NOT NULL,
  delivered      INTEGER NOT NULL DEFAULT 0 CHECK (delivered IN (0, 1)),
  -- Why it was not delivered, when it was not.
  detail         TEXT,
  created_at     TEXT NOT NULL
);

CREATE INDEX handoffs_to_thread ON handoffs(to_thread_id, created_at DESC);
