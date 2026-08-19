//! Thread overlay, runs, transcript log, and Inbox projection.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{InboxEventRow, NewThread, RunRow, ThreadRow, TranscriptEventRow};
use super::{
    map_inbox_event, map_run, map_thread, map_transcript, now_utc, validate_runtime_json,
};

pub fn insert_thread(conn: &Connection, new: &NewThread) -> Result<ThreadRow, StoreError> {
    if new.id.trim().is_empty() || new.title.trim().is_empty() || new.cwd.trim().is_empty() {
        return Err(StoreError::invalid("thread id, title, and cwd are required"));
    }
    validate_runtime_json(&new.runtime_json)?;
    let now = now_utc();
    conn.execute(
        "INSERT INTO threads (
            id, folder_id, bot_id, harness_id, cwd, runtime_json, title,
            state, fold_policy, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?9)",
        params![
            new.id,
            new.folder_id,
            new.bot_id,
            new.harness_id,
            new.cwd,
            new.runtime_json,
            new.title,
            new.fold_policy,
            now
        ],
    )?;
    get_thread(conn, &new.id)?.ok_or_else(|| StoreError::NotFound(new.id.clone()))
}

pub fn get_thread(conn: &Connection, id: &str) -> Result<Option<ThreadRow>, StoreError> {
    conn.query_row(
        "SELECT id, folder_id, bot_id, harness_id, acp_session_id, native_session_ref,
                cwd, runtime_json, title, state, fold_policy, last_stop_reason, last_error,
                preview, worktree_path, created_at, updated_at, folded_at, resurfaced_at,
                archived_at, deleted_at
         FROM threads WHERE id = ?1",
        [id],
        map_thread,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_threads_by_state(
    conn: &Connection,
    state: &str,
) -> Result<Vec<ThreadRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, folder_id, bot_id, harness_id, acp_session_id, native_session_ref,
                cwd, runtime_json, title, state, fold_policy, last_stop_reason, last_error,
                preview, worktree_path, created_at, updated_at, folded_at, resurfaced_at,
                archived_at, deleted_at
         FROM threads
         WHERE state = ?1 AND deleted_at IS NULL
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt
        .query_map([state], map_thread)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn set_thread_acp_session(
    conn: &Connection,
    id: &str,
    acp_session_id: &str,
) -> Result<ThreadRow, StoreError> {
    if acp_session_id.trim().is_empty() {
        return Err(StoreError::invalid("acp_session_id is required"));
    }
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE threads SET acp_session_id = ?2, updated_at = ?3
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, acp_session_id, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

pub fn set_thread_state(
    conn: &Connection,
    id: &str,
    state: &str,
) -> Result<ThreadRow, StoreError> {
    match state {
        "active" | "folded" | "resurfaced" | "archived" => {}
        _ => return Err(StoreError::invalid(format!("invalid thread state {state}"))),
    }
    let now = now_utc();
    let folded_at = (state == "folded").then_some(now.clone());
    let resurfaced_at = (state == "resurfaced").then_some(now.clone());
    let archived_at = (state == "archived").then_some(now.clone());
    let changed = conn.execute(
        "UPDATE threads SET
            state = ?2,
            updated_at = ?3,
            folded_at = CASE WHEN ?2 = 'folded' THEN COALESCE(folded_at, ?4) ELSE folded_at END,
            resurfaced_at = CASE WHEN ?2 = 'resurfaced' THEN ?5 ELSE resurfaced_at END,
            archived_at = CASE WHEN ?2 = 'archived' THEN COALESCE(archived_at, ?6) ELSE archived_at END
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, state, now, folded_at, resurfaced_at, archived_at],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

pub fn insert_run(
    conn: &Connection,
    thread_id: &str,
    kind: &str,
    trigger_json: Option<&str>,
) -> Result<RunRow, StoreError> {
    match kind {
        "prompt" | "schedule" | "handoff" | "resume" => {}
        _ => return Err(StoreError::invalid(format!("invalid run kind {kind}"))),
    }
    let next_seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM runs WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )?;
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO runs (id, thread_id, seq, kind, state, trigger_json, created_at)
         VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6)",
        params![id, thread_id, next_seq, kind, trigger_json, now],
    )?;
    get_run(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_run(conn: &Connection, id: &str) -> Result<Option<RunRow>, StoreError> {
    conn.query_row(
        "SELECT id, thread_id, seq, kind, state, trigger_json, error, started_at, ended_at, created_at
         FROM runs WHERE id = ?1",
        [id],
        map_run,
    )
    .optional()
    .map_err(Into::into)
}

pub fn set_run_state(
    conn: &Connection,
    id: &str,
    state: &str,
    error: Option<&str>,
) -> Result<RunRow, StoreError> {
    match state {
        "queued" | "running" | "succeeded" | "failed" | "cancelled" | "timed_out"
        | "lost" | "needs_you" => {}
        _ => return Err(StoreError::invalid(format!("invalid run state {state}"))),
    }
    let now = now_utc();
    let started_at = (state == "running").then_some(now.as_str());
    let ended_at = matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "timed_out" | "lost" | "needs_you"
    )
    .then_some(now.as_str());
    let changed = conn.execute(
        "UPDATE runs SET
            state = ?2,
            error = ?3,
            started_at = CASE WHEN ?2 = 'running' THEN COALESCE(started_at, ?4) ELSE started_at END,
            ended_at = CASE WHEN ?5 IS NOT NULL THEN ?5 ELSE ended_at END
         WHERE id = ?1",
        params![id, state, error, started_at, ended_at],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_run(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

pub fn append_transcript(
    conn: &Connection,
    thread_id: &str,
    acp_method: &str,
    payload_json: &str,
) -> Result<TranscriptEventRow, StoreError> {
    if acp_method.trim().is_empty() {
        return Err(StoreError::invalid("acp_method is required"));
    }
    serde_json::from_str::<serde_json::Value>(payload_json)?;
    let next_seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM transcript_events WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )?;
    let now = now_utc();
    conn.execute(
        "INSERT INTO transcript_events (thread_id, seq, acp_method, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![thread_id, next_seq, acp_method, payload_json, now],
    )?;
    Ok(TranscriptEventRow {
        thread_id: thread_id.into(),
        seq: next_seq,
        acp_method: acp_method.into(),
        payload_json: payload_json.into(),
        created_at: now,
    })
}

pub fn transcript_after(
    conn: &Connection,
    thread_id: &str,
    seq: i64,
) -> Result<Vec<TranscriptEventRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT thread_id, seq, acp_method, payload_json, created_at
         FROM transcript_events
         WHERE thread_id = ?1 AND seq > ?2
         ORDER BY seq",
    )?;
    let rows = stmt
        .query_map(params![thread_id, seq], map_transcript)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn insert_inbox_event(
    conn: &Connection,
    thread_id: &str,
    run_id: Option<&str>,
    kind: &str,
    title: &str,
    summary: &str,
    payload_json: Option<&str>,
) -> Result<InboxEventRow, StoreError> {
    match kind {
        "folded" | "done" | "failed" | "needs_you" | "judgment_call" | "permission"
        | "lost" | "stuck" => {}
        _ => return Err(StoreError::invalid(format!("invalid inbox kind {kind}"))),
    }
    if title.trim().is_empty() {
        return Err(StoreError::invalid("inbox title is required"));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO inbox_events (
            id, thread_id, run_id, kind, title, summary, payload_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, thread_id, run_id, kind, title, summary, payload_json, now],
    )?;
    conn.query_row(
        "SELECT id, thread_id, run_id, kind, title, summary, payload_json,
                created_at, read_at, dismissed_at
         FROM inbox_events WHERE id = ?1",
        [&id],
        map_inbox_event,
    )
    .map_err(Into::into)
}
