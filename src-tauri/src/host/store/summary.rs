//! Extra repositories and attached sources on a code conversation (#269).
//!
//! The thread's own folder is the primary repository and is not stored here.
//! These tables are the attachments a person adds afterwards: another
//! registered folder, or a file they want the conversation to keep in view.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::now_utc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRepoLink {
    pub thread_id: String,
    pub folder_id: String,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSourceRow {
    pub id: String,
    pub thread_id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub mime: Option<String>,
    pub created_at: String,
}

pub fn list_thread_repos(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<ThreadRepoLink>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT thread_id, folder_id, sort_order, created_at
         FROM thread_repos
         WHERE thread_id = ?1
         ORDER BY sort_order ASC, created_at ASC",
    )?;
    let rows = stmt.query_map([thread_id], |row| {
        Ok(ThreadRepoLink {
            thread_id: row.get(0)?,
            folder_id: row.get(1)?,
            sort_order: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn attach_thread_repo(
    conn: &Connection,
    thread_id: &str,
    folder_id: &str,
) -> Result<ThreadRepoLink, StoreError> {
    if thread_id.trim().is_empty() {
        return Err(StoreError::invalid("threadId is required"));
    }
    if folder_id.trim().is_empty() {
        return Err(StoreError::invalid("folderId is required"));
    }
    let sort_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM thread_repos WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )?;
    let created_at = now_utc();
    conn.execute(
        "INSERT OR IGNORE INTO thread_repos (thread_id, folder_id, sort_order, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![thread_id, folder_id, sort_order, created_at],
    )?;
    conn.query_row(
        "SELECT thread_id, folder_id, sort_order, created_at
         FROM thread_repos WHERE thread_id = ?1 AND folder_id = ?2",
        params![thread_id, folder_id],
        |row| {
            Ok(ThreadRepoLink {
                thread_id: row.get(0)?,
                folder_id: row.get(1)?,
                sort_order: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn detach_thread_repo(
    conn: &Connection,
    thread_id: &str,
    folder_id: &str,
) -> Result<bool, StoreError> {
    let n = conn.execute(
        "DELETE FROM thread_repos WHERE thread_id = ?1 AND folder_id = ?2",
        params![thread_id, folder_id],
    )?;
    Ok(n > 0)
}

pub fn list_thread_sources(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<ThreadSourceRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, name, kind, path, mime, created_at
         FROM thread_sources
         WHERE thread_id = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([thread_id], |row| {
        Ok(ThreadSourceRow {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
            path: row.get(4)?,
            mime: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn add_thread_source(
    conn: &Connection,
    thread_id: &str,
    name: &str,
    kind: &str,
    path: &str,
    mime: Option<&str>,
) -> Result<ThreadSourceRow, StoreError> {
    if thread_id.trim().is_empty() {
        return Err(StoreError::invalid("threadId is required"));
    }
    if path.trim().is_empty() {
        return Err(StoreError::invalid("path is required"));
    }
    let id = Uuid::new_v4().to_string();
    let created_at = now_utc();
    conn.execute(
        "INSERT INTO thread_sources (id, thread_id, name, kind, path, mime, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, thread_id, name.trim(), kind, path, mime, created_at,],
    )?;
    get_thread_source(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_thread_source(
    conn: &Connection,
    id: &str,
) -> Result<Option<ThreadSourceRow>, StoreError> {
    conn.query_row(
        "SELECT id, thread_id, name, kind, path, mime, created_at
         FROM thread_sources WHERE id = ?1",
        [id],
        |row| {
            Ok(ThreadSourceRow {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                path: row.get(4)?,
                mime: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn remove_thread_source(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM thread_sources WHERE id = ?1", [id])?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use crate::host::store::Store;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("jabot.sqlite")).unwrap();
        (dir, store)
    }

    #[test]
    fn extra_repos_and_sources_round_trip() {
        let (_dir, store) = store();
        store
            .insert_folder(&crate::host::store::NewFolder {
                name: "jabot".into(),
                path: "/tmp/jabot".into(),
                files_to_copy_json: "[]".into(),
                ..Default::default()
            })
            .unwrap();
        let extra = store
            .insert_folder(&crate::host::store::NewFolder {
                name: "jabot-frontend".into(),
                path: "/tmp/frontend".into(),
                sort_order: 1,
                files_to_copy_json: "[]".into(),
                ..Default::default()
            })
            .unwrap();
        store
            .insert_thread(&crate::host::store::NewThread {
                id: "t-1".into(),
                folder_id: None,
                bot_id: None,
                harness_id: "claude".into(),
                cwd: "/tmp/jabot".into(),
                runtime_json: r#"{"command":"claude-agent-acp","args":[],"env":{}}"#.into(),
                title: "Auth".into(),
                fold_policy: "default".into(),
                worktree_path: None,
                repo: crate::host::store::ThreadRepo::default(),
            })
            .unwrap();

        let link = store.attach_thread_repo("t-1", &extra.id).unwrap();
        assert_eq!(link.folder_id, extra.id);
        assert_eq!(store.list_thread_repos("t-1").unwrap().len(), 1);
        // Idempotent: attaching twice does not grow a second row.
        store.attach_thread_repo("t-1", &extra.id).unwrap();
        assert_eq!(store.list_thread_repos("t-1").unwrap().len(), 1);

        let source = store
            .add_thread_source("t-1", "notes.md", "file", "/tmp/notes.md", None)
            .unwrap();
        assert_eq!(store.list_thread_sources("t-1").unwrap().len(), 1);
        assert!(store.remove_thread_source(&source.id).unwrap());
        assert!(store.list_thread_sources("t-1").unwrap().is_empty());
        assert!(store.detach_thread_repo("t-1", &extra.id).unwrap());
        assert!(store.list_thread_repos("t-1").unwrap().is_empty());
    }
}
