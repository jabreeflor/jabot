//! Conversation branches (#266).
//!
//! A Code thread can be forked at a message: the child is a new thread whose
//! transcript is the source's log through that seq, and whose project context
//! (folder, harness, runtime) is the source's. The original is untouched.
//!
//! One live child per `(source, through_seq)`. The unique primary key is the
//! whole of "do not mint a second branch from the same click."

use rusqlite::{params, Connection, OptionalExtension};

use super::error::StoreError;
use super::models::ThreadBranchRow;
use super::now_utc;

const COLUMNS: &str = "source_thread_id, through_seq, branch_thread_id, created_at";

pub fn get_branch(
    conn: &Connection,
    source_thread_id: &str,
    through_seq: i64,
) -> Result<Option<ThreadBranchRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM thread_branches WHERE source_thread_id = ?1 AND through_seq = ?2"),
        params![source_thread_id, through_seq],
        map_branch,
    )
    .optional()
    .map_err(Into::into)
}

/// The source this thread was branched from, if it is a child.
pub fn branch_source_of(
    conn: &Connection,
    branch_thread_id: &str,
) -> Result<Option<ThreadBranchRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM thread_branches WHERE branch_thread_id = ?1"),
        [branch_thread_id],
        map_branch,
    )
    .optional()
    .map_err(Into::into)
}

/// Record a branch. `INSERT OR IGNORE` so a raced second write does not
/// overwrite the winner — the caller re-reads and discards its own child.
pub fn insert_branch(
    conn: &Connection,
    source_thread_id: &str,
    through_seq: i64,
    branch_thread_id: &str,
) -> Result<bool, StoreError> {
    if through_seq < 1 {
        return Err(StoreError::invalid("throughSeq must be at least 1"));
    }
    let changed = conn.execute(
        "INSERT OR IGNORE INTO thread_branches (
            source_thread_id, through_seq, branch_thread_id, created_at
         ) VALUES (?1, ?2, ?3, ?4)",
        params![source_thread_id, through_seq, branch_thread_id, now_utc()],
    )?;
    Ok(changed == 1)
}

/// Drop a stale mapping whose child was deleted, so a later click can write
/// a new one against the same `(source, through_seq)`.
pub fn delete_branch(
    conn: &Connection,
    source_thread_id: &str,
    through_seq: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM thread_branches WHERE source_thread_id = ?1 AND through_seq = ?2",
        params![source_thread_id, through_seq],
    )?;
    Ok(())
}

/// Copy the source log through `through_seq` onto the child, keeping seq
/// numbers so a message that was `e12-0` on the source is `e12-0` on the
/// branch. `created_at` is the original's: this is history, not a new turn.
pub fn copy_transcript_through(
    conn: &Connection,
    source_thread_id: &str,
    branch_thread_id: &str,
    through_seq: i64,
) -> Result<usize, StoreError> {
    let copied = conn.execute(
        "INSERT INTO transcript_events (thread_id, seq, acp_method, payload_json, created_at)
         SELECT ?1, seq, acp_method, payload_json, created_at
         FROM transcript_events
         WHERE thread_id = ?2 AND seq <= ?3
         ORDER BY seq",
        params![branch_thread_id, source_thread_id, through_seq],
    )?;
    Ok(copied)
}

fn map_branch(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadBranchRow> {
    Ok(ThreadBranchRow {
        source_thread_id: row.get(0)?,
        through_seq: row.get(1)?,
        branch_thread_id: row.get(2)?,
        created_at: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::host::store::{NewThread, Store, ThreadRepo};

    fn open() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("host.sqlite")).unwrap();
        (store, dir)
    }

    fn thread(id: &str) -> NewThread {
        NewThread {
            id: id.into(),
            folder_id: None,
            bot_id: None,
            harness_id: "claude".into(),
            cwd: "/tmp".into(),
            runtime_json: json!({ "command": "fake" }).to_string(),
            title: id.into(),
            fold_policy: "default".into(),
            worktree_path: None,
            repo: ThreadRepo::default(),
        }
    }

    fn event(store: &Store, thread_id: &str, text: &str) {
        store
            .append_transcript(
                thread_id,
                "session/update",
                &json!({
                    "sessionUpdate": "user_message_chunk",
                    "content": { "type": "text", "text": text }
                })
                .to_string(),
            )
            .unwrap();
    }

    #[test]
    fn one_mapping_per_source_and_seq() {
        let (store, _dir) = open();
        store.insert_thread(&thread("src")).unwrap();
        store.insert_thread(&thread("a")).unwrap();
        store.insert_thread(&thread("b")).unwrap();

        assert!(store.insert_branch("src", 2, "a").unwrap());
        assert!(!store.insert_branch("src", 2, "b").unwrap());
        let row = store.get_branch("src", 2).unwrap().unwrap();
        assert_eq!(row.branch_thread_id, "a");
    }

    #[test]
    fn a_deleted_mapping_can_be_replaced() {
        let (store, _dir) = open();
        store.insert_thread(&thread("src")).unwrap();
        store.insert_thread(&thread("old")).unwrap();
        store.insert_thread(&thread("new")).unwrap();
        store.insert_branch("src", 1, "old").unwrap();
        store.delete_branch("src", 1).unwrap();
        assert!(store.insert_branch("src", 1, "new").unwrap());
        assert_eq!(
            store
                .get_branch("src", 1)
                .unwrap()
                .unwrap()
                .branch_thread_id,
            "new"
        );
    }

    #[test]
    fn copy_keeps_seq_and_stops_at_the_cut() {
        let (store, _dir) = open();
        store.insert_thread(&thread("src")).unwrap();
        store.insert_thread(&thread("child")).unwrap();
        event(&store, "src", "one");
        event(&store, "src", "two");
        event(&store, "src", "three");

        assert_eq!(store.copy_transcript_through("src", "child", 2).unwrap(), 2);
        let copied = store.transcript_after("child", 0).unwrap();
        assert_eq!(
            copied.iter().map(|row| row.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(store.transcript_after("src", 0).unwrap().len(), 3);
    }

    #[test]
    fn through_seq_must_be_positive() {
        let (store, _dir) = open();
        store.insert_thread(&thread("src")).unwrap();
        store.insert_thread(&thread("child")).unwrap();
        assert!(store.insert_branch("src", 0, "child").is_err());
    }
}
