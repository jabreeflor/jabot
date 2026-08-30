//! Thread overlay, runs, transcript log, and Inbox projection.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{
    InboxEventRow, NewThread, RunRow, SessionReceiptRow, ThreadRow, TranscriptEventRow,
};
use super::{
    map_inbox_event, map_receipt, map_run, map_thread, map_transcript, now_utc,
    validate_runtime_json,
};

/// Column order for [`map_thread`]. One list so a new column lands in every
/// read at once instead of in whichever query someone remembered.
const THREAD_COLUMNS: &str = "id, folder_id, bot_id, harness_id, acp_session_id, \
     native_session_ref, cwd, runtime_json, title, state, fold_policy, last_stop_reason, \
     last_error, preview, worktree_path, created_at, updated_at, folded_at, resurfaced_at, \
     archived_at, deleted_at, resurfaced_reason, repo_root, repo, forge_host, branch, host_id";

const RUN_COLUMNS: &str = "id, thread_id, seq, kind, state, trigger_json, error, \
     started_at, ended_at, created_at, acp_session_id";

const INBOX_COLUMNS: &str = "id, thread_id, run_id, kind, title, summary, payload_json, \
     created_at, read_at, dismissed_at";

const RECEIPT_COLUMNS: &str = "thread_id, acp_session_id, native_session_ref, harness_id, \
     model, cwd, tools_json, permission_mode, fingerprint, created_at, updated_at";

pub fn insert_thread(conn: &Connection, new: &NewThread) -> Result<ThreadRow, StoreError> {
    if new.id.trim().is_empty() || new.title.trim().is_empty() || new.cwd.trim().is_empty() {
        return Err(StoreError::invalid(
            "thread id, title, and cwd are required",
        ));
    }
    validate_runtime_json(&new.runtime_json)?;
    let now = now_utc();
    // The repo columns are written by the same INSERT as the thread, never by
    // a follow-up UPDATE: a thread that exists for even one crash-width moment
    // without knowing which checkout it belongs to is a thread that has to
    // guess afterwards (setup-porting §19).
    conn.execute(
        "INSERT INTO threads (
            id, folder_id, bot_id, harness_id, cwd, runtime_json, title,
            state, fold_policy, created_at, updated_at,
            repo_root, repo, forge_host, branch, host_id, worktree_path
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            new.id,
            new.folder_id,
            new.bot_id,
            new.harness_id,
            new.cwd,
            new.runtime_json,
            new.title,
            new.fold_policy,
            now,
            new.repo.repo_root,
            new.repo.repo,
            new.repo.forge_host,
            new.repo.branch,
            new.repo.host_id,
            new.worktree_path,
        ],
    )?;
    get_thread(conn, &new.id)?.ok_or_else(|| StoreError::NotFound(new.id.clone()))
}

pub fn get_thread(conn: &Connection, id: &str) -> Result<Option<ThreadRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {THREAD_COLUMNS} FROM threads WHERE id = ?1"),
        [id],
        map_thread,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_threads_by_state(conn: &Connection, state: &str) -> Result<Vec<ThreadRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THREAD_COLUMNS} FROM threads
         WHERE state = ?1 AND deleted_at IS NULL
         ORDER BY updated_at DESC"
    ))?;
    let rows = stmt
        .query_map([state], map_thread)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The rows the sidebar draws under a folder (#16).
///
/// `folded` and `archived` are absent on purpose: fold's promise is that the
/// row goes away and comes back through the Inbox, so a folder listing that
/// still showed it would break the one gesture the product is built around.
pub fn list_folder_threads(
    conn: &Connection,
    folder_id: &str,
) -> Result<Vec<ThreadRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THREAD_COLUMNS} FROM threads
         WHERE folder_id = ?1 AND deleted_at IS NULL
           AND state IN ('active', 'resurfaced')
         ORDER BY updated_at DESC"
    ))?;
    let rows = stmt
        .query_map([folder_id], map_thread)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every live thread one bot is working on (#24's `list_crew_status`).
///
/// **Folded rows are included here**, which is the opposite of
/// [`list_folder_threads`], and deliberately so: fold hides a thread from the
/// human's sidebar, not from the crew roster. "What is everyone working on"
/// that silently omitted the long job Chief folded away an hour ago would be
/// answering a different question than the one asked. Archived and deleted
/// rows are gone: nothing is working on those.
pub fn list_bot_threads(conn: &Connection, bot_id: &str) -> Result<Vec<ThreadRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THREAD_COLUMNS} FROM threads
         WHERE bot_id = ?1 AND deleted_at IS NULL
           AND state IN ('active', 'folded', 'resurfaced')
         ORDER BY updated_at DESC"
    ))?;
    let rows = stmt
        .query_map([bot_id], map_thread)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every thread that still claims a worktree.
///
/// Archived and deleted rows are excluded on purpose: their trees have been
/// released, or should have been, and the boot sweep (#23) uses exactly this
/// set to decide which directories under the worktree root nobody owns.
pub fn list_worktree_threads(conn: &Connection) -> Result<Vec<ThreadRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {THREAD_COLUMNS} FROM threads
         WHERE worktree_path IS NOT NULL AND deleted_at IS NULL AND state <> 'archived'"
    ))?;
    let rows = stmt
        .query_map([], map_thread)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Record that the thread's worktree has been removed, or put back.
///
/// The path itself is chosen once, by the INSERT that creates the thread; this
/// only ever says whether the tree named there currently exists. A row that
/// still claimed a directory git had collected would send every later reader —
/// and the next boot sweep — after a tree that is gone, and a row that stayed
/// `NULL` after a restore would have the sweep collect a live one.
pub fn set_thread_worktree(
    conn: &Connection,
    id: &str,
    path: Option<&str>,
) -> Result<ThreadRow, StoreError> {
    let changed = conn.execute(
        "UPDATE threads SET worktree_path = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, path, now_utc()],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.to_string()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))
}

/// Point a thread at the branch that actually holds its work.
///
/// The branch is chosen by the INSERT like the path is, and normally never
/// changes. The exception is the one archive discovers: the agent moved `HEAD`
/// inside the tree, so the save landed somewhere other than `jabot/<id>` and
/// the tree's own `HEAD` is about to be deleted with it (#23). A row still
/// naming the original branch would have a reopen restore a checkout without
/// the work in it, and nothing would say so.
pub fn set_thread_branch(
    conn: &Connection,
    id: &str,
    branch: &str,
) -> Result<ThreadRow, StoreError> {
    if branch.trim().is_empty() {
        return Err(StoreError::invalid("branch is required"));
    }
    let changed = conn.execute(
        "UPDATE threads SET branch = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, branch, now_utc()],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.to_string()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))
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

pub fn set_thread_state(conn: &Connection, id: &str, state: &str) -> Result<ThreadRow, StoreError> {
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
        &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
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
        "queued" | "running" | "succeeded" | "failed" | "cancelled" | "timed_out" | "lost"
        | "needs_you" => {}
        _ => return Err(StoreError::invalid(format!("invalid run state {state}"))),
    }
    let now = now_utc();
    let started_at = (state == "running").then_some(now.as_str());
    let ended_at = matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "timed_out" | "lost" | "needs_you"
    )
    .then_some(now.as_str());
    // `needs_you` stamps `ended_at` because the run has stopped producing, but
    // it is a pause, not an end: answering the permission puts the run back in
    // `running`, and the stale end time has to go with it.
    let changed = conn.execute(
        "UPDATE runs SET
            state = ?2,
            error = ?3,
            started_at = CASE WHEN ?2 = 'running' THEN COALESCE(started_at, ?4) ELSE started_at END,
            ended_at = CASE WHEN ?2 IN ('queued', 'running') THEN NULL ELSE COALESCE(?5, ended_at) END
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

/// The highest `seq` this thread has on disk, or 0 for a thread with no
/// transcript. Asked separately from [`transcript_after`] because a caller
/// that is already up to date reads no rows and still has to be told where the
/// log ends (#14).
pub fn transcript_head(conn: &Connection, thread_id: &str) -> Result<i64, StoreError> {
    let head = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM transcript_events WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )?;
    Ok(head)
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
        // `pr` (#28) is a card about a pull request rather than about a run.
        // It is its own kind because the run kinds would each say something
        // untrue: `failed` claims the turn failed, `needs_you` claims an agent
        // is blocked on the human. Neither is what "checks went red an hour
        // after the session finished" is.
        "folded" | "done" | "failed" | "needs_you" | "judgment_call" | "permission" | "lost"
        | "stuck" | "pr" => {}
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
        params![
            id,
            thread_id,
            run_id,
            kind,
            title,
            summary,
            payload_json,
            now
        ],
    )?;
    conn.query_row(
        &format!("SELECT {INBOX_COLUMNS} FROM inbox_events WHERE id = ?1"),
        [&id],
        map_inbox_event,
    )
    .map_err(Into::into)
}

/// Move a thread between overlay states, refusing to move one that is not
/// where the caller thought it was.
///
/// The `from` guard is in the WHERE clause rather than a read-then-write so a
/// stale caller loses the race instead of silently clobbering a state another
/// path already advanced. Legality of the pair itself is decided one layer up,
/// in `host::lifecycle::state` — this only enforces that the move applies to
/// the row it was computed from.
pub fn transition_thread(
    conn: &Connection,
    id: &str,
    from: &str,
    to: &str,
    reason: Option<&str>,
) -> Result<ThreadRow, StoreError> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE threads SET
            state = ?3,
            resurfaced_reason = CASE WHEN ?3 = 'resurfaced' THEN ?4 ELSE NULL END,
            updated_at = ?5,
            folded_at = CASE WHEN ?3 = 'folded' THEN ?5 ELSE folded_at END,
            resurfaced_at = CASE WHEN ?3 = 'resurfaced' THEN ?5 ELSE resurfaced_at END,
            archived_at = CASE WHEN ?3 = 'archived' THEN ?5 ELSE archived_at END
         WHERE id = ?1 AND state = ?2 AND deleted_at IS NULL",
        params![id, from, to, reason, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

/// Delete is a tombstone: the row stays so a late adapter event has something
/// to land on, and every read filters on `deleted_at IS NULL`.
pub fn tombstone_thread(conn: &Connection, id: &str) -> Result<ThreadRow, StoreError> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE threads SET deleted_at = ?2, updated_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

pub fn set_thread_fold_policy(
    conn: &Connection,
    id: &str,
    fold_policy: &str,
) -> Result<ThreadRow, StoreError> {
    match fold_policy {
        "default" | "wait_for_inbox" => {}
        _ => {
            return Err(StoreError::invalid(format!(
                "invalid fold policy {fold_policy}"
            )))
        }
    }
    let changed = conn.execute(
        "UPDATE threads SET fold_policy = ?2, updated_at = ?3
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, fold_policy, now_utc()],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_thread(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

/// Record how the last turn ended. `last_stop_reason` is what the resurface
/// classifier read; keeping the raw string means a custom `_reason` from an
/// adapter we do not know about is still visible to a human.
pub fn set_thread_stop(
    conn: &Connection,
    id: &str,
    stop_reason: Option<&str>,
    error: Option<&str>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE threads SET last_stop_reason = ?2, last_error = ?3, updated_at = ?4
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, stop_reason, error, now_utc()],
    )?;
    Ok(())
}

pub fn set_run_acp_session(
    conn: &Connection,
    id: &str,
    acp_session_id: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE runs SET acp_session_id = ?2 WHERE id = ?1",
        params![id, acp_session_id],
    )?;
    Ok(())
}

/// Runs newest first — the Inbox and the thread header both want the latest.
pub fn list_runs(conn: &Connection, thread_id: &str) -> Result<Vec<RunRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_COLUMNS} FROM runs WHERE thread_id = ?1 ORDER BY seq DESC"
    ))?;
    let rows = stmt
        .query_map([thread_id], map_run)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn latest_run(conn: &Connection, thread_id: &str) -> Result<Option<RunRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {RUN_COLUMNS} FROM runs WHERE thread_id = ?1 ORDER BY seq DESC LIMIT 1"),
        [thread_id],
        map_run,
    )
    .optional()
    .map_err(Into::into)
}

/// Every run a stopped host left open, newest run first within each thread.
///
/// The boot pass (#21) reads exactly this: nothing else can tell a fresh
/// process that work was in flight when the last one went away, because the
/// process axis is deliberately not persisted (#5). Tombstoned threads are
/// excluded — their runs went with them.
pub fn list_open_runs(conn: &Connection) -> Result<Vec<RunRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_COLUMNS} FROM runs
         WHERE state IN ('queued', 'running', 'needs_you')
           AND thread_id IN (SELECT id FROM threads WHERE deleted_at IS NULL)
         ORDER BY thread_id, seq DESC"
    ))?;
    let rows = stmt
        .query_map([], map_run)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Rewrite the summary of the newest undismissed card of `kind` on a thread,
/// and make it unread again. `false` when there is no such card.
///
/// This exists for one case and should not grow others: a thread that was
/// already resurfaced `needs_you` before the host stopped still has a card
/// describing a permission request whose process no longer exists. The
/// transition to `resurfaced` is a no-op the second time, so a boot pass that
/// could only insert would either say nothing or stack a duplicate card for
/// the same unanswered question. Restating the row is the only option that
/// leaves the Inbox with one true card.
pub fn restate_inbox_event(
    conn: &Connection,
    thread_id: &str,
    kind: &str,
    summary: &str,
) -> Result<bool, StoreError> {
    let changed = conn.execute(
        "UPDATE inbox_events
            SET summary = ?3, read_at = NULL
          WHERE id = (
            SELECT id FROM inbox_events
             WHERE thread_id = ?1 AND kind = ?2 AND dismissed_at IS NULL
             ORDER BY created_at DESC, rowid DESC
             LIMIT 1
          )",
        params![thread_id, kind, summary],
    )?;
    Ok(changed > 0)
}

/// The Inbox is a projection of these rows (#5), newest first. Deleted threads
/// take their cards with them.
pub fn list_inbox_events(
    conn: &Connection,
    limit: i64,
    include_dismissed: bool,
) -> Result<Vec<InboxEventRow>, StoreError> {
    let dismissed_filter = if include_dismissed {
        ""
    } else {
        "AND dismissed_at IS NULL"
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {INBOX_COLUMNS} FROM inbox_events
         WHERE thread_id IN (SELECT id FROM threads WHERE deleted_at IS NULL)
           {dismissed_filter}
         ORDER BY created_at DESC, rowid DESC
         LIMIT ?1"
    ))?;
    let rows = stmt
        .query_map([limit], map_inbox_event)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Badge count. `thread_id` narrows it to one row's dot; `None` is the Inbox
/// nav badge.
///
/// Only threads the Inbox still shows can badge it. `resurface.md` defines the
/// nav badge as resurfaced-and-unread, and `state-machine.md` hides `archived`
/// from both the sidebar and the Inbox — a badge for a thread the user has
/// archived points at a row that is not there any more, and nothing in the UI
/// could ever clear it.
///
/// A `pr` card (#28) is the one exception, and it is an exception because it is
/// not a claim about the thread at all: "checks went red" is about a pull
/// request that is still open on GitHub, and the session that opened it is
/// *expected* to be finished and archived by then. Gating it on the overlay
/// would mean the only cards this kind ever produces are cards nobody is told
/// about. Deleted threads still take theirs with them.
pub fn count_unread_inbox(conn: &Connection, thread_id: Option<&str>) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT COUNT(*) FROM inbox_events
         WHERE read_at IS NULL AND dismissed_at IS NULL
           AND (?1 IS NULL OR thread_id = ?1)
           AND thread_id IN (
                SELECT id FROM threads
                WHERE deleted_at IS NULL
                  AND (state IN ('folded', 'resurfaced') OR inbox_events.kind = 'pr')
           )",
        [thread_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// The same count, per bot: what the red dot on a crew blob means.
///
/// The predicate is [`count_unread_inbox`]'s, character for character, and
/// that is the point rather than an accident. A dot on Writer's face and the
/// number in the sidebar are two readings of one fact, so they have to agree
/// about which cards count — including the `pr` exception, whose whole reason
/// is that it is a claim about a pull request rather than about the thread.
///
/// Grouped on `threads.bot_id` and not on the derived `bot-<botId>` id.
/// `standing.rs` steps over tombstones with `MAX_GENERATIONS`, so one bot can
/// own several generations of standing thread and only the column is stable
/// across them. Bots with nothing waiting are simply absent from the map,
/// which is what the caller wants to write as a zero.
pub fn count_unread_inbox_by_bot(conn: &Connection) -> Result<HashMap<String, i64>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT threads.bot_id, COUNT(*) FROM inbox_events
         JOIN threads ON threads.id = inbox_events.thread_id
         WHERE inbox_events.read_at IS NULL AND inbox_events.dismissed_at IS NULL
           AND threads.bot_id IS NOT NULL
           AND threads.deleted_at IS NULL
           AND (threads.state IN ('folded', 'resurfaced') OR inbox_events.kind = 'pr')
         GROUP BY threads.bot_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut counts = HashMap::new();
    for row in rows {
        let (bot_id, count) = row?;
        counts.insert(bot_id, count);
    }
    Ok(counts)
}

/// An event the host answered on the user's behalf is recorded but never
/// badged — it is an away-log entry, not something the human still owes.
pub fn mark_inbox_event_read(conn: &Connection, id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE inbox_events SET read_at = ?2 WHERE id = ?1 AND read_at IS NULL",
        params![id, now_utc()],
    )?;
    Ok(())
}

/// Mark this thread's newest undismissed card of one kind read.
///
/// Narrower than [`mark_inbox_read`] on purpose. Opening a thread clears
/// everything it has waiting, because the user is now looking at all of it.
/// Answering one question clears exactly that question — anything else the
/// thread has surfaced is still owed, and sweeping it away because a
/// permission happened to be answered would lose cards nobody read.
///
/// Newest-first with the same tie-break as `restate_inbox_event`, so a thread
/// that asked twice clears the ask that is actually on screen.
pub fn mark_inbox_kind_read(
    conn: &Connection,
    thread_id: &str,
    kind: &str,
) -> Result<bool, StoreError> {
    let changed = conn.execute(
        "UPDATE inbox_events
            SET read_at = ?3
          WHERE id = (
            SELECT id FROM inbox_events
             WHERE thread_id = ?1 AND kind = ?2
               AND dismissed_at IS NULL AND read_at IS NULL
             ORDER BY created_at DESC, rowid DESC
             LIMIT 1
          )",
        params![thread_id, kind, now_utc()],
    )?;
    Ok(changed > 0)
}

/// Opening the thread is what clears its badge (`resurface.md`, Badge).
pub fn mark_inbox_read(conn: &Connection, thread_id: &str) -> Result<usize, StoreError> {
    let changed = conn.execute(
        "UPDATE inbox_events SET read_at = ?2 WHERE thread_id = ?1 AND read_at IS NULL",
        params![thread_id, now_utc()],
    )?;
    Ok(changed)
}

/// One receipt per thread; a re-spawn overwrites it because the live session is
/// the only one a resume can attach to.
#[allow(clippy::too_many_arguments)]
pub fn upsert_session_receipt(
    conn: &Connection,
    thread_id: &str,
    acp_session_id: &str,
    native_session_ref: Option<&str>,
    harness_id: &str,
    model: Option<&str>,
    cwd: &str,
    tools_json: &str,
    permission_mode: &str,
    fingerprint: &str,
) -> Result<SessionReceiptRow, StoreError> {
    if acp_session_id.trim().is_empty() {
        return Err(StoreError::invalid("receipt acp_session_id is required"));
    }
    serde_json::from_str::<serde_json::Value>(tools_json)?;
    let now = now_utc();
    conn.execute(
        "INSERT INTO session_receipts (
            thread_id, acp_session_id, native_session_ref, harness_id, model, cwd,
            tools_json, permission_mode, fingerprint, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT(thread_id) DO UPDATE SET
            acp_session_id = excluded.acp_session_id,
            native_session_ref = excluded.native_session_ref,
            harness_id = excluded.harness_id,
            model = excluded.model,
            cwd = excluded.cwd,
            tools_json = excluded.tools_json,
            permission_mode = excluded.permission_mode,
            fingerprint = excluded.fingerprint,
            updated_at = excluded.updated_at",
        params![
            thread_id,
            acp_session_id,
            native_session_ref,
            harness_id,
            model,
            cwd,
            tools_json,
            permission_mode,
            fingerprint,
            now
        ],
    )?;
    get_session_receipt(conn, thread_id)?.ok_or_else(|| StoreError::NotFound(thread_id.into()))
}

pub fn get_session_receipt(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<SessionReceiptRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {RECEIPT_COLUMNS} FROM session_receipts WHERE thread_id = ?1"),
        [thread_id],
        map_receipt,
    )
    .optional()
    .map_err(Into::into)
}

/// The resurface write: overlay state and Inbox card, or neither.
///
/// Both rows land in one transaction so a client can never be told a thread
/// came back without the card that says why still being on disk — and so a
/// failure part-way through does not leave a resurfaced thread with no Inbox
/// row to open. The notification is emitted by the caller, strictly after this
/// returns `Ok` (#15, persist-then-notify).
///
/// The card the user has not read yet is the Inbox twin of the one live
/// notification `resurface.md` allows per thread, so a resurface **replaces**
/// it rather than stacking beside it: a thread that went quiet and then
/// finished shows one Done row, not a stale "has gone quiet" next to its own
/// answer. Read cards are history from an earlier visit to the Inbox and stay.
#[allow(clippy::too_many_arguments)]
pub fn resurface_thread(
    conn: &Connection,
    id: &str,
    from: &str,
    reason: &str,
    kind: &str,
    title: &str,
    summary: &str,
    payload_json: Option<&str>,
    run_id: Option<&str>,
) -> Result<(ThreadRow, InboxEventRow), StoreError> {
    let tx = conn.unchecked_transaction()?;
    let now = now_utc();
    let changed = tx.execute(
        "UPDATE threads SET
            state = 'resurfaced',
            resurfaced_reason = ?3,
            resurfaced_at = ?4,
            updated_at = ?4
         WHERE id = ?1 AND state = ?2 AND deleted_at IS NULL",
        params![id, from, reason, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    tx.execute(
        "UPDATE inbox_events SET dismissed_at = ?2
         WHERE thread_id = ?1
           AND read_at IS NULL AND dismissed_at IS NULL
           AND kind IN ('done', 'failed', 'stuck', 'needs_you')",
        params![id, now],
    )?;
    let event = insert_inbox_event(&tx, id, run_id, kind, title, summary, payload_json)?;
    let row = get_thread(&tx, id)?.ok_or_else(|| StoreError::NotFound(id.into()))?;
    tx.commit()?;
    Ok((row, event))
}
