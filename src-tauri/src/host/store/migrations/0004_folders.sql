-- Folder registration (#16), and the repo facts a thread is stamped with at
-- spawn.
--
-- A folder is ONE registered local directory, almost always the root of a git
-- checkout (docs/research/git-and-prs/folders-and-auth.md). `path` stays what
-- the user registered; `repo_root` is what `git rev-parse --show-toplevel`
-- answered for it, and is NULL for a directory that is not in a repo — those
-- folders still run threads, they just have no PR surface. The partial unique
-- index is the rule that one repo is one folder: registering a subdirectory of
-- a repo already on the list must find the existing row rather than open a
-- second sidebar entry pointed at the same checkout.
--
-- `setup_command` and `files_to_copy_json` are stored here and consumed by #23
-- when it mints a worktree: a fresh tree has no `node_modules` and no `.env`,
-- and the folder is the only place that knows what makes this repo runnable.
--
-- Nothing here is a credential. GitHub auth is the user's `gh` login, read on
-- demand by shelling out (#16); a token never reaches SQLite, in this table or
-- any other.

ALTER TABLE folders ADD COLUMN repo_root          TEXT;
ALTER TABLE folders ADD COLUMN origin_url         TEXT;
ALTER TABLE folders ADD COLUMN forge_host         TEXT;
ALTER TABLE folders ADD COLUMN repo_owner         TEXT;
ALTER TABLE folders ADD COLUMN repo_name          TEXT;
ALTER TABLE folders ADD COLUMN default_branch     TEXT;
ALTER TABLE folders ADD COLUMN setup_command      TEXT;
ALTER TABLE folders ADD COLUMN files_to_copy_json TEXT NOT NULL DEFAULT '[]';

CREATE UNIQUE INDEX folders_repo_root ON folders(repo_root)
  WHERE repo_root IS NOT NULL;

-- Setup-porting §19: record repo, worktree, branch, host and cwd at spawn;
-- never infer them later. `cwd` and `worktree_path` were already on the row —
-- these are the rest of it. A thread that has to re-derive its own repo after a
-- restart derives it from wherever the app happens to be running, and a folder
-- the user has since removed takes the answer with it; the columns survive both
-- because they were written once, when the thread was opened.
--
-- `repo` is `owner/name` as `gh` spells it, and `forge_host` is the host from
-- `origin` — github.com, a GHES hostname, or gitlab.com. Both NULL for a thread
-- with no remote, which is legal: the thread runs, the PR view skips it.
ALTER TABLE threads ADD COLUMN repo_root  TEXT;
ALTER TABLE threads ADD COLUMN repo       TEXT;
ALTER TABLE threads ADD COLUMN forge_host TEXT;
ALTER TABLE threads ADD COLUMN branch     TEXT;
-- Which machine opened it. One host in MVP1; the column is what stops a
-- second one from having to guess (remote-and-mobile).
ALTER TABLE threads ADD COLUMN host_id    TEXT;
