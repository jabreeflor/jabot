-- Repositories and sources attached to a code conversation (#269).
--
-- A thread already carries one folder — the checkout it was opened in — and
-- that remains the primary repository. Extra folders a person attaches live
-- here rather than as more columns on `threads`, because a conversation can
-- grow a second (or third) repo without rewriting the spawn record, and
-- forgetting a folder must drop the attachment rather than leave a dangling
-- folder_id on the thread.
--
-- Sources are files the person attached to *this* conversation. A path, not
-- a copy: the file is still theirs, and a thumbnail that went stale because
-- they moved the original is an honest empty slot, not a second store of
-- their disk.

CREATE TABLE thread_repos (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    PRIMARY KEY (thread_id, folder_id)
);

CREATE TABLE thread_sources (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    mime TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX thread_sources_thread ON thread_sources(thread_id, created_at);
CREATE INDEX thread_repos_folder ON thread_repos(folder_id);
