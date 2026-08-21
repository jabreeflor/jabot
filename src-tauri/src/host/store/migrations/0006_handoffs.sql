-- Where a job came from, when it did not come from the human (#24).
--
-- Chief routes work by *handing it off*: `handoff_to_bot` puts a task on
-- another crew member's standing thread, `spawn_code_session` opens a coding
-- thread in a registered folder. Decision #6 is explicit that routing is a
-- host action rather than a nested subagent — which means the receiving bot
-- gets a prompt with no natural way of saying who asked for it, and the human
-- reading that thread later has no way of finding out.
--
-- One row per dispatch answers both. It hangs off the *receiving* thread
-- because that is where the question gets asked ("why is Writer working on
-- this?"), and it keeps the sending thread and bot so the trail runs the other
-- way too.
--
-- Deliberately not a message and not a run: the task text here is what Chief
-- asked for, which is not the same thing as what was said to the agent (the
-- transcript owns that, #14) or how the turn went (`runs` owns that, #15). A
-- dispatch whose prompt could not be delivered still leaves this row, and
-- `dispatched` is what says so.

CREATE TABLE handoffs (
  id              TEXT PRIMARY KEY,
  -- What kind of dispatch this was: 'handoff' | 'code_session'. Two tools,
  -- one trail — the difference is whether the receiving thread was a standing
  -- thread or a fresh checkout.
  kind            TEXT NOT NULL CHECK (kind IN ('handoff', 'code_session')),
  -- The thread the work landed on. CASCADE because a handoff to a thread that
  -- no longer exists is a trail to nowhere; `thread/delete` is a tombstone, so
  -- in practice this only fires on a hard purge.
  to_thread_id    TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  -- SET NULL, not CASCADE: removing a bot from the crew must not erase the
  -- history of what it was asked to do (#17 already detaches its threads).
  to_bot_id       TEXT REFERENCES bots(id) ON DELETE SET NULL,
  from_thread_id  TEXT REFERENCES threads(id) ON DELETE SET NULL,
  from_bot_id     TEXT REFERENCES bots(id) ON DELETE SET NULL,
  -- What Chief asked for, and anything it wanted carried across. Kept as the
  -- caller wrote it: this is evidence, not a prompt template.
  task            TEXT NOT NULL,
  context         TEXT,
  -- Whether the task actually reached an agent. A handoff to a bot whose
  -- harness is not installed is still a real handoff — the row is the record
  -- that it was asked for, and this is the record that nobody heard it.
  dispatched      INTEGER NOT NULL DEFAULT 0 CHECK (dispatched IN (0, 1)),
  detail          TEXT,
  created_at      TEXT NOT NULL
);

-- The read that matters: "who sent this thread its work?" — newest first,
-- because a standing thread accumulates handoffs for as long as it lives.
CREATE INDEX handoffs_to_thread ON handoffs(to_thread_id, created_at DESC);
CREATE INDEX handoffs_from_thread ON handoffs(from_thread_id, created_at DESC);
