//! Numbered SQL migrations. PRAGMAs are applied in connect code, not here.

use rusqlite::Connection;

use super::error::StoreError;
use super::now_utc;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_lifecycle.sql")),
    (3, include_str!("migrations/0003_tool_connections.sql")),
    (4, include_str!("migrations/0004_folders.sql")),
    (5, include_str!("migrations/0005_permission_requests.sql")),
    (6, include_str!("migrations/0006_handoffs.sql")),
    (8, include_str!("migrations/0008_pairing.sql")),
    (9, include_str!("migrations/0009_schedules.sql")),
    (10, include_str!("migrations/0010_pull_requests.sql")),
];

/// The schema version a freshly opened store lands on.
///
/// Derived from the list rather than written down twice: an assertion about
/// what `host/hello` reports is then an assertion about the migrations that
/// exist, and does not have to be edited by whoever lands the next one.
pub fn head() -> i32 {
    MIGRATIONS.last().map(|&(version, _)| version).unwrap_or(0)
}

/// Numbers must ascend, but they do not have to be contiguous.
///
/// Migration numbers are allocated per issue, and branches land out of order:
/// a tree can legitimately hold `0006` and `0008` while `0007` is still in
/// review, and refusing that would mean renumbering a migration that has
/// already run on someone's machine. What still has to hold is that every file
/// in the list is applied exactly once and in order — so a repeat or a step
/// backwards is a mis-numbered entry and is refused here, before anything runs.
fn check_order(migrations: &[(i32, &str)]) -> Result<(), StoreError> {
    for pair in migrations.windows(2) {
        if pair[1].0 <= pair[0].0 {
            return Err(StoreError::Migration {
                version: pair[1].0,
                message: format!(
                    "migration {} is listed after {}; the list must ascend",
                    pair[1].0, pair[0].0
                ),
            });
        }
    }
    Ok(())
}

pub fn migrate(conn: &mut Connection) -> Result<i32, StoreError> {
    check_order(MIGRATIONS)?;
    let mut current = schema_version(conn)?;
    for &(version, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        if let Err(err) = tx.execute_batch(sql) {
            return Err(StoreError::Migration {
                version,
                message: err.to_string(),
            });
        }
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![version, now_utc()],
        )?;
        tx.commit()?;
        current = version;
    }
    Ok(current)
}

pub fn schema_version(conn: &Connection) -> Result<i32, StoreError> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }
    let version: Option<i32> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), head());
        assert_eq!(migrate(&mut conn).unwrap(), head());
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }

    /// The head of the list is what `host/hello` reports as `schemaVersion`,
    /// and a gap in the numbers must not change that or stop the run.
    #[test]
    fn a_gap_in_the_numbers_still_applies_every_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), head());
        let applied: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(applied as usize, MIGRATIONS.len());
    }

    /// The invariant the contiguity check used to enforce, kept: a list that
    /// repeats or goes backwards is a mistake, and it fails before any SQL runs.
    #[test]
    fn a_list_that_does_not_ascend_is_refused() {
        assert!(check_order(&[(1, ""), (2, "")]).is_ok());
        assert!(check_order(&[(1, ""), (1, "")]).is_err());
        assert!(check_order(&[(2, ""), (1, "")]).is_err());
    }

    // ---- upgrading an install that already exists -------------------------
    //
    // Everything above this line runs migrations on an empty in-memory
    // database, which is the one case that cannot go wrong: every statement
    // sees a schema built by the statement before it. What ships is the other
    // case — a `jabot.sqlite` created by an older build, with rows in it, that
    // has to arrive at exactly the schema a fresh install gets.
    //
    // D-020 is the shape of the defect: `0010` rebuilds `inbox_events` to widen
    // a CHECK constraint, and rebuilding a table in SQLite silently drops every
    // index on it — including `inbox_events_unread`, which `0002` created and
    // the nav badge reads on every projection. A fresh database and an upgraded
    // one both *worked*; only one of them had the index. Nothing failed, and
    // nothing could have: no test compared the two schemas.

    /// Apply the migrations up to and including `upto`, and stop — an install
    /// that last ran a build from that point in the history.
    ///
    /// Deliberately re-implemented from `MIGRATIONS` rather than exposing a
    /// production `migrate_to`: no shipping caller wants to stop halfway, and a
    /// knob that only tests turn is a knob that can be turned by mistake.
    fn install_at(upto: i32) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for &(version, sql) in MIGRATIONS {
            if version > upto {
                break;
            }
            conn.execute_batch(sql).unwrap_or_else(|err| {
                panic!("migration {version} does not apply to a database at {upto}: {err}")
            });
            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![version, now_utc()],
            )
            .unwrap();
        }
        conn
    }

    /// Every object SQLite believes the database has, with the SQL that made
    /// it. Indexes are in here on purpose: they are the half of a schema that
    /// can go missing without anything failing.
    ///
    /// `sqlite_autoindex_*` rows carry a NULL `sql` and are implied by the
    /// table definitions already compared, so they add nothing but noise.
    fn schema(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT type, name, tbl_name, COALESCE(sql, '')
                   FROM sqlite_master
                  WHERE name NOT LIKE 'sqlite_%'
                  ORDER BY type, name",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                let kind: String = row.get(0)?;
                let name: String = row.get(1)?;
                let table: String = row.get(2)?;
                let sql: String = row.get(3)?;
                // Whitespace differs between a CREATE run from a migration file
                // and the same CREATE replayed by a table rebuild; the object
                // is the same either way.
                let sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                Ok(format!("{kind} {name} on {table}: {sql}"))
            })
            .unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// Every index any migration ever created, minus the ones a migration
    /// deliberately dropped.
    ///
    /// Read out of the SQL text rather than out of a database, because that is
    /// the only place the *intent* survives: once `0010` has rebuilt
    /// `inbox_events`, no database anywhere remembers that `0002` wanted an
    /// index on it.
    fn indexes_the_migrations_ask_for() -> Vec<String> {
        let mut wanted: Vec<String> = Vec::new();
        for &(_, sql) in MIGRATIONS {
            for line in sql.lines() {
                let line = line.trim();
                let rest = line
                    .strip_prefix("CREATE UNIQUE INDEX ")
                    .or_else(|| line.strip_prefix("CREATE INDEX "));
                if let Some(rest) = rest {
                    let name = rest.split_whitespace().next().unwrap_or_default();
                    wanted.push(name.to_string());
                }
                if let Some(rest) = line.strip_prefix("DROP INDEX ") {
                    let name = rest.trim_end_matches(';').trim();
                    wanted.retain(|held| held != name);
                }
            }
        }
        wanted
    }

    /// D-020, stated so that repeating it fails here.
    ///
    /// `0010` widens a CHECK on `inbox_events`, which SQLite can only do by
    /// rebuilding the table — and a rebuilt table arrives with none of its
    /// indexes. `0002` created `inbox_events_unread` for the nav badge, which
    /// reads the table on every projection. Losing it fails nothing, migrates
    /// cleanly, and answers every query correctly, just by scanning.
    ///
    /// Note what this deliberately does *not* do: compare a fresh database
    /// against an upgraded one. Both run `0002` and then `0010`, so both would
    /// be missing the index and they would agree perfectly — a comparison that
    /// cannot fail on the defect it is named after. The migrations' own SQL is
    /// the only statement of what was meant to exist.
    #[test]
    fn no_rebuild_quietly_drops_an_index_an_earlier_migration_created() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate(&mut conn).unwrap();

        let present: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();

        let wanted = indexes_the_migrations_ask_for();
        assert!(
            wanted.contains(&"inbox_events_unread".to_string()),
            "the scan found no index names at all, so it proves nothing: {wanted:?}"
        );
        let lost: Vec<&String> = wanted.iter().filter(|w| !present.contains(w)).collect();
        assert!(
            lost.is_empty(),
            "a later migration dropped {lost:?} without a DROP INDEX saying so — \
             re-create them at the end of the migration that rebuilds their table (D-020)"
        );
    }

    /// A migration that works on an empty database and not on one with rows in
    /// it breaks every existing install and no fresh one. Seeding first is what
    /// separates the two.
    ///
    /// Runs from *every* version in the list, not just the newest: a migration
    /// added next year that assumes a v9 shape fails here the day it lands,
    /// rather than on the machines of everyone who installed before it.
    #[test]
    fn an_install_at_any_earlier_version_upgrades_with_the_rows_it_already_had() {
        let mut fresh = Connection::open_in_memory().unwrap();
        fresh.pragma_update(None, "foreign_keys", true).unwrap();
        migrate(&mut fresh).unwrap();
        let want = schema(&fresh);

        for &(version, _) in MIGRATIONS {
            let mut old = install_at(version);
            assert_eq!(schema_version(&old).unwrap(), version);
            seed(&old);
            assert_eq!(
                migrate(&mut old).unwrap(),
                head(),
                "an install at {version} did not reach head"
            );

            let kept: i64 = old
                .query_row("SELECT COUNT(*) FROM inbox_events", [], |row| row.get(0))
                .unwrap();
            assert_eq!(
                kept, 3,
                "an install at {version} lost Inbox rows on upgrade"
            );
            let prs: i64 = old
                .query_row("SELECT COUNT(*) FROM thread_prs", [], |row| row.get(0))
                .unwrap();
            assert_eq!(prs, 1, "an install at {version} lost its PR linkage");

            let got = schema(&old);
            let missing: Vec<_> = want.iter().filter(|o| !got.contains(o)).collect();
            let extra: Vec<_> = got.iter().filter(|o| !want.contains(o)).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "an install upgraded from {version} does not match a fresh one.\n\
                 missing: {missing:#?}\nunexpected: {extra:#?}"
            );
        }
    }

    /// One of everything the tables from `0001` can hold, which is every table
    /// an install at any version in the list already has.
    fn seed(conn: &Connection) {
        conn.execute_batch(SEED).expect("seed an existing install");
    }

    const SEED: &str = "
        INSERT INTO harnesses (id, label, command, created_at, updated_at)
             VALUES ('claude', 'Claude', 'claude-agent-acp', 't0', 't0');
        INSERT INTO folders (id, name, path, created_at, updated_at)
             VALUES ('f-1', 'jabot', '/Users/x/jabot', 't0', 't0');
        INSERT INTO bots (id, name, color, harness_id, created_at, updated_at)
             VALUES ('b-1', 'Chief', '#fff', 'claude', 't0', 't0');
        INSERT INTO threads (id, folder_id, bot_id, harness_id, cwd, runtime_json,
                             title, state, created_at, updated_at)
             VALUES ('t-1', 'f-1', 'b-1', 'claude', '/Users/x/jabot', '{}',
                     'Auth migration', 'folded', 't0', 't0');
        INSERT INTO runs (id, thread_id, seq, kind, state, created_at)
             VALUES ('r-1', 't-1', 1, 'prompt', 'needs_you', 't0');
        INSERT INTO transcript_events (thread_id, seq, acp_method, payload_json, created_at)
             VALUES ('t-1', 1, 'session/update', '{}', 't0');
        INSERT INTO inbox_events (id, thread_id, run_id, kind, title, summary,
                                  payload_json, created_at, read_at, dismissed_at)
             VALUES ('e-read',  't-1', 'r-1', 'needs_you', 'Run ls', 'about to run ls',
                     '{\"reviewable\":true}', 't1', 't2', NULL),
                    ('e-fresh', 't-1', NULL, 'done', 'Finished', '',
                     NULL, 't3', NULL, NULL),
                    ('e-gone',  't-1', NULL, 'stuck', 'Went quiet', 'no output',
                     NULL, 't4', NULL, 't5');
        INSERT INTO thread_prs (id, thread_id, repo, number, url, status,
                                created_at, updated_at)
             VALUES ('p-1', 't-1', 'jabreeflor/jabot', 42,
                     'https://github.com/jabreeflor/jabot/pull/42', 'open', 't0', 't0');
    ";

    /// Rows an install already had, written at the oldest schema that can hold
    /// them and read back at head.
    ///
    /// `0010` copies `inbox_events` into a new table and drops the original, so
    /// an install's whole Inbox history goes through a `SELECT` written by
    /// hand. A column left out of that list, or a row the copy misses, is not a
    /// crash — it is silence, and the user's Inbox is simply shorter than it
    /// was.
    #[test]
    fn an_upgrade_carries_the_rows_the_install_already_had() {
        let mut conn = install_at(1);
        seed(&conn);

        assert_eq!(migrate(&mut conn).unwrap(), head());

        let mut stmt = conn
            .prepare(
                "SELECT id, kind, title, summary, COALESCE(payload_json, '-'),
                        created_at, COALESCE(read_at, '-'), COALESCE(dismissed_at, '-'),
                        COALESCE(run_id, '-')
                   FROM inbox_events ORDER BY created_at",
            )
            .unwrap();
        let events: Vec<Vec<String>> = stmt
            .query_map([], |row| {
                (0..9)
                    .map(|i| row.get::<_, String>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            events,
            vec![
                vec![
                    "e-read".to_string(),
                    "needs_you".into(),
                    "Run ls".into(),
                    "about to run ls".into(),
                    "{\"reviewable\":true}".into(),
                    "t1".into(),
                    "t2".into(),
                    "-".into(),
                    "r-1".into(),
                ],
                vec![
                    "e-fresh".to_string(),
                    "done".into(),
                    "Finished".into(),
                    "".into(),
                    "-".into(),
                    "t3".into(),
                    "-".into(),
                    "-".into(),
                    "-".into(),
                ],
                vec![
                    "e-gone".to_string(),
                    "stuck".into(),
                    "Went quiet".into(),
                    "no output".into(),
                    "-".into(),
                    "t4".into(),
                    "-".into(),
                    "t5".into(),
                    "-".into(),
                ],
            ],
            "the inbox_events rebuild in 0010 did not carry every row and column across"
        );

        // The PR row predates every column `0010` adds; the defaults have to
        // land on it rather than the row failing the NOT NULLs.
        let (title, checks, additions): (String, String, i64) = conn
            .query_row(
                "SELECT title, checks_json, additions FROM thread_prs WHERE id = 'p-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((title.as_str(), checks.as_str(), additions), ("", "[]", 0));
        let url: String = conn
            .query_row("SELECT url FROM thread_prs WHERE id = 'p-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(url, "https://github.com/jabreeflor/jabot/pull/42");
    }

    /// `0010` drops `inbox_events` and renames a copy over it. A rebuild that
    /// lost the foreign key would not fail anything at migration time — it
    /// would leave every future thread deletion behind a pile of orphan cards
    /// pointing at a thread that no longer exists.
    #[test]
    fn the_rebuilt_inbox_table_still_cascades_from_its_thread() {
        for from in [1, head()] {
            let mut conn = install_at(from);
            migrate(&mut conn).unwrap();
            conn.execute_batch(
                "INSERT INTO harnesses (id, label, command, created_at, updated_at)
                      VALUES ('claude', 'Claude', 'c', 't0', 't0');
                 INSERT INTO threads (id, harness_id, cwd, runtime_json, title,
                                      created_at, updated_at)
                      VALUES ('t-1', 'claude', '/tmp', '{}', 'x', 't0', 't0');
                 INSERT INTO runs (id, thread_id, seq, kind, created_at)
                      VALUES ('r-1', 't-1', 1, 'prompt', 't0');
                 INSERT INTO inbox_events (id, thread_id, run_id, kind, title, created_at)
                      VALUES ('e-1', 't-1', 'r-1', 'done', 'Finished', 't1');",
            )
            .unwrap();

            // The kind 0010 exists to allow, on a table it rebuilt.
            conn.execute(
                "INSERT INTO inbox_events (id, thread_id, kind, title, created_at)
                      VALUES ('e-pr', 't-1', 'pr', 'CI went red', 't2')",
                [],
            )
            .unwrap_or_else(|err| panic!("upgraded from {from}: 'pr' was refused: {err}"));
            // And one it must still refuse, because the CHECK is the only thing
            // keeping the Inbox's kinds a closed list.
            conn.execute(
                "INSERT INTO inbox_events (id, thread_id, kind, title, created_at)
                      VALUES ('e-bad', 't-1', 'whatever', 'x', 't3')",
                [],
            )
            .unwrap_err();

            conn.execute("DELETE FROM threads WHERE id = 't-1'", [])
                .unwrap();
            let left: i64 = conn
                .query_row("SELECT COUNT(*) FROM inbox_events", [], |row| row.get(0))
                .unwrap();
            assert_eq!(left, 0, "upgraded from {from}: cards outlived their thread");
        }
    }
}
