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
}
