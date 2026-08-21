//! The handoff trail: where a thread's work came from when a bot sent it (#24).
//!
//! Chief's `handoff_to_bot` and `spawn_code_session` are host actions, not
//! nested subagents (decision #6). That is the right seam — but it means the
//! receiving agent is handed a prompt with no author, and a human opening
//! Writer's thread a day later cannot tell whether they asked for this or
//! Chief did. One row per dispatch is the answer, and it is written before the
//! prompt is sent, for the same reason #5 writes the Inbox card before it
//! notifies: a dispatch that fails halfway must not be a dispatch nobody can
//! account for.
//!
//! Reads are newest-first and hang off the receiving thread, because a
//! standing thread collects handoffs for as long as the bot exists.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{HandoffRow, NewHandoff};
use super::{map_handoff, now_utc};

/// `handoff_to_bot`: a task on another crew member's standing thread.
pub const KIND_HANDOFF: &str = "handoff";
/// `spawn_code_session`: a task on a fresh coding thread in a folder.
pub const KIND_CODE_SESSION: &str = "code_session";

const COLUMNS: &str = "id, kind, to_thread_id, to_bot_id, from_thread_id, from_bot_id, \
     task, context, dispatched, detail, created_at";

pub fn insert_handoff(conn: &Connection, new: &NewHandoff) -> Result<HandoffRow, StoreError> {
    if new.task.trim().is_empty() {
        return Err(StoreError::invalid("a handoff needs a task"));
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO handoffs (
            id, kind, to_thread_id, to_bot_id, from_thread_id, from_bot_id,
            task, context, dispatched, detail, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            new.kind,
            new.to_thread_id,
            new.to_bot_id,
            new.from_thread_id,
            new.from_bot_id,
            new.task.trim(),
            new.context,
            i64::from(new.dispatched),
            new.detail,
            now_utc(),
        ],
    )?;
    get_handoff(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_handoff(conn: &Connection, id: &str) -> Result<Option<HandoffRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM handoffs WHERE id = ?1"),
        [id],
        map_handoff,
    )
    .optional()
    .map_err(Into::into)
}

/// Whether the prompt actually reached an agent. Written after the dispatch
/// attempt rather than with the row, because the row has to exist first — the
/// attempt can leave the host holding an error, and an unrecorded handoff is
/// exactly the thing this table is for.
pub fn set_handoff_dispatched(
    conn: &Connection,
    id: &str,
    dispatched: bool,
    detail: Option<&str>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE handoffs SET dispatched = ?2, detail = ?3 WHERE id = ?1",
        params![id, i64::from(dispatched), detail],
    )?;
    Ok(())
}

/// The most recent handoff onto a thread — what `thread/state` reports as
/// "where this work came from".
pub fn latest_handoff_to(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<HandoffRow>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM handoffs WHERE to_thread_id = ?1
             ORDER BY created_at DESC, rowid DESC LIMIT 1"
        ),
        [thread_id],
        map_handoff,
    )
    .optional()
    .map_err(Into::into)
}

/// Every handoff onto a thread, newest first.
pub fn list_handoffs_to(conn: &Connection, thread_id: &str) -> Result<Vec<HandoffRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM handoffs WHERE to_thread_id = ?1
         ORDER BY created_at DESC, rowid DESC"
    ))?;
    let rows = stmt
        .query_map([thread_id], map_handoff)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
