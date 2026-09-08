-- Conversation branches (#266). One live child per (source thread, through
-- seq): a second click on the same message must reopen the existing branch,
-- not mint another conversation.
--
-- Numbered 0015 because #275 landed 0014_message_reactions on main first.
CREATE TABLE thread_branches (
  source_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  through_seq      INTEGER NOT NULL,
  branch_thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  created_at       TEXT NOT NULL,
  PRIMARY KEY (source_thread_id, through_seq)
);

CREATE UNIQUE INDEX thread_branches_child ON thread_branches(branch_thread_id);
