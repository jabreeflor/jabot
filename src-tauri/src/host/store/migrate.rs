//! Numbered SQL migrations. PRAGMAs are applied in connect code, not here.

use rusqlite::Connection;

use super::error::StoreError;
use super::now_utc;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_lifecycle.sql")),
    (3, include_str!("migrations/0003_tool_connections.sql")),
    (4, include_str!("migrations/0004_folders.sql")),
];

pub fn migrate(conn: &mut Connection) -> Result<i32, StoreError> {
    let mut current = schema_version(conn)?;
    for &(version, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        if version != current + 1 {
            return Err(StoreError::Migration {
                version,
                message: format!("expected next version {}, found {version}", current + 1),
            });
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
        let head = MIGRATIONS.last().expect("at least one migration").0;
        assert_eq!(migrate(&mut conn).unwrap(), head);
        assert_eq!(migrate(&mut conn).unwrap(), head);
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'threads'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }
}
