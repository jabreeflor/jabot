-- Emoji reactions on transcript items (#265).
--
-- User-authored overlay, not an ACP event: a thumbs-up is not something the
-- harness said. The renderer item id is the key because that is what the
-- picker already has when the user clicks, and hydrate from the same log
-- produces the same ids.
--
-- Numbered 0014 because main already shipped 0013_thread_summary.

CREATE TABLE message_reactions (
  thread_id   TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  item_id     TEXT NOT NULL,
  emoji       TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  PRIMARY KEY (thread_id, item_id, emoji)
);

CREATE INDEX message_reactions_thread ON message_reactions(thread_id);
