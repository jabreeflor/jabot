//! The permission broker's durable ledger (#20).
//!
//! One row per `session/request_permission`, written *before* the ask is
//! announced. The reason is the one decision #5 makes about the Inbox, applied
//! to a question instead of a result: a notification that never arrives loses
//! nothing if the thing it announced is already on disk, and the reverse order
//! loses the agent's question to a quit.
//!
//! Resolution is a guarded UPDATE rather than a read-then-write, so two clicks
//! on the same card cannot both claim the request — the second one changes no
//! rows and the broker answers it from what the first one wrote.

use rusqlite::{params, Connection, OptionalExtension};

use super::error::StoreError;
use super::models::{NewPermissionRequest, PermissionRequestRow};
use super::{map_permission_request, now_utc};

/// Outstanding: the human still owes an answer.
pub const PENDING: &str = "pending";
/// The human (or the fold policy) chose an option.
pub const ANSWERED: &str = "answered";
/// Nobody chose: the turn was cancelled, or the adapter died holding the ask.
pub const CANCELLED: &str = "cancelled";

const COLUMNS: &str = "id, thread_id, run_id, kind, title, subject_json, options_json, \
     state, decided_by, option_id, delivered, created_at, resolved_at";

/// The same list, qualified: the join below shares column names with `threads`.
const JOINED_COLUMNS: &str = "p.id, p.thread_id, p.run_id, p.kind, p.title, p.subject_json, \
     p.options_json, p.state, p.decided_by, p.option_id, p.delivered, p.created_at, p.resolved_at";

pub fn insert_permission_request(
    conn: &Connection,
    new: &NewPermissionRequest,
) -> Result<PermissionRequestRow, StoreError> {
    conn.execute(
        "INSERT INTO permission_requests (
            id, thread_id, run_id, kind, title, subject_json, options_json,
            state, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8)",
        params![
            new.id,
            new.thread_id,
            new.run_id,
            new.kind,
            new.title,
            new.subject_json,
            new.options_json,
            now_utc(),
        ],
    )?;
    get_permission_request(conn, &new.id)?.ok_or_else(|| StoreError::NotFound(new.id.clone()))
}

pub fn get_permission_request(
    conn: &Connection,
    id: &str,
) -> Result<Option<PermissionRequestRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM permission_requests WHERE id = ?1"),
        [id],
        map_permission_request,
    )
    .optional()
    .map_err(Into::into)
}

/// Asks nobody has answered, oldest first.
///
/// Joined against `threads` because a request is only answerable while its
/// conversation is: an archived or deleted thread has no adapter to reach and
/// no card to put the question on, and its rows would otherwise sit in every
/// pending list forever.
pub fn list_open_permission_requests(
    conn: &Connection,
    thread_id: Option<&str>,
) -> Result<Vec<PermissionRequestRow>, StoreError> {
    let sql = format!(
        "SELECT {JOINED_COLUMNS} FROM permission_requests p
         JOIN threads t ON t.id = p.thread_id
         WHERE p.state = 'pending'
           AND t.deleted_at IS NULL
           AND t.state <> 'archived'
           AND (?1 IS NULL OR p.thread_id = ?1)
         ORDER BY p.created_at, p.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![thread_id], map_permission_request)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every ask ever taken on a thread, oldest first — the away log of what the
/// host asked and what it decided, whichever way each one went.
pub fn list_permission_requests(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<PermissionRequestRow>, StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM permission_requests
         WHERE thread_id = ?1 ORDER BY created_at, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([thread_id], map_permission_request)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Claim an outstanding request and record what was decided.
///
/// `Ok(false)` means somebody else got there first — a second click, or the
/// adapter dying between the button and the reply. That is not an error; it is
/// the answer to "did this call decide it", and the broker reads the row back
/// to say what the decision was.
pub fn resolve_permission_request(
    conn: &Connection,
    id: &str,
    state: &str,
    decided_by: &str,
    option_id: Option<&str>,
    delivered: bool,
) -> Result<bool, StoreError> {
    match state {
        ANSWERED | CANCELLED => {}
        other => {
            return Err(StoreError::invalid(format!(
                "invalid permission resolution {other}"
            )))
        }
    }
    let changed = conn.execute(
        "UPDATE permission_requests
            SET state = ?2, decided_by = ?3, option_id = ?4, delivered = ?5,
                resolved_at = ?6
          WHERE id = ?1 AND state = 'pending'",
        params![id, state, decided_by, option_id, delivered, now_utc()],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::{NewThread, Store, ThreadRepo};

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path().join("jabot.sqlite")).expect("store");
        store
            .insert_thread(&NewThread {
                id: "t-1".into(),
                folder_id: None,
                bot_id: None,
                harness_id: "claude".into(),
                cwd: dir.path().to_string_lossy().into(),
                runtime_json: "{\"command\":\"true\"}".into(),
                title: "Auth migration".into(),
                fold_policy: "default".into(),
                worktree_path: None,
                repo: ThreadRepo::default(),
            })
            .expect("thread");
        (dir, store)
    }

    fn ask(store: &Store, id: &str) -> PermissionRequestRow {
        store
            .insert_permission_request(&NewPermissionRequest {
                id: id.into(),
                thread_id: "t-1".into(),
                run_id: None,
                kind: Some("execute".into()),
                title: "Run ls".into(),
                subject_json: "{}".into(),
                options_json: "[]".into(),
            })
            .expect("insert")
    }

    #[test]
    fn an_outstanding_ask_survives_the_process_that_took_it() {
        let (dir, store) = store();
        ask(&store, "req-1");
        drop(store);

        let reopened = Store::open(dir.path().join("jabot.sqlite")).expect("reopen");
        let open = reopened.list_open_permission_requests(None).expect("list");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].title, "Run ls");
        assert_eq!(open[0].kind.as_deref(), Some("execute"));
    }

    #[test]
    fn only_the_first_answer_claims_the_request() {
        let (_dir, store) = store();
        ask(&store, "req-1");

        assert!(store
            .resolve_permission_request("req-1", ANSWERED, "dev-1", Some("allow_once"), true)
            .expect("first"));
        // The second click, or the click that raced the adapter dying.
        assert!(!store
            .resolve_permission_request("req-1", CANCELLED, "dev-1", None, false)
            .expect("second"));

        let row = store
            .get_permission_request("req-1")
            .expect("read")
            .expect("row");
        assert_eq!(row.state, ANSWERED);
        assert_eq!(row.option_id.as_deref(), Some("allow_once"));
        assert!(row.delivered);
        assert!(store
            .list_open_permission_requests(None)
            .expect("list")
            .is_empty());
    }

    #[test]
    fn an_archived_thread_stops_asking() {
        let (_dir, store) = store();
        ask(&store, "req-1");
        store
            .transition_thread("t-1", "active", "archived", None)
            .expect("archive");
        assert!(store
            .list_open_permission_requests(None)
            .expect("list")
            .is_empty());
    }
}
