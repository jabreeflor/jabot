//! `thread_prs`: which session opened which pull request, and what GitHub last
//! said about it (#28).
//!
//! Three rules the callers depend on.
//!
//! **`(provider, repo, number)` is the identity.** It has been the table's
//! UNIQUE key since 0001 and it is the key the linkage path dedupes on: the
//! same PR detected twice — once from `gh pr create` stdout, once from the
//! post-turn `gh pr view` — has to be one row, or the PR view grows a duplicate
//! every turn. `id` is a surrogate the renderer keys React on; it is never the
//! thing two detections are compared by.
//!
//! **The first thread to claim a PR keeps it.** `thread_id` is NOT NULL and a
//! later detection from a *different* thread updates GitHub's half of the row
//! and leaves the link alone. Re-pointing would mean the PR view's "Reopen
//! thread" silently changing which conversation it opens, on evidence no
//! stronger than what wrote it in the first place. The one thing that does
//! remove a link is the thread going: the foreign key cascades.
//!
//! **Linkage and poll state are written by different calls.** [`link_pr`] takes
//! evidence, [`apply_snapshot`] takes GitHub. A machine with no `gh` login
//! links PRs it will never be able to poll, and that is a working state rather
//! than a broken one — the row says `polled_at IS NULL` and the view says so.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{NewThreadPr, PrSnapshot, ThreadPrRow};
use super::{map_thread_pr, now_utc};

/// `thread_prs.status`.
pub const STATUS_OPEN: &str = "open";
pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_MERGED: &str = "merged";
pub const STATUS_CLOSED: &str = "closed";

/// `thread_prs.check_state`. Deliberately three words and not a boolean: a PR
/// whose checks are still going is not asking to be reviewed yet, which is the
/// whole reason the prototype has a CHECKS RUNNING section.
pub const CHECKS_PASSING: &str = "passing";
pub const CHECKS_RUNNING: &str = "running";
pub const CHECKS_FAILING: &str = "failing";

/// `thread_prs.detected_via` — the confidence trail in `pr-linkage.md`.
pub const VIA_STDOUT: &str = "stdout";
pub const VIA_GH_PR_VIEW: &str = "gh-pr-view";
pub const VIA_HEAD_LIST: &str = "head-list";

const COLUMNS: &str = "id, thread_id, provider, forge_host, repo, number, url, title, status, \
     check_state, review_state, head_ref, base_ref, additions, deletions, changed_files, \
     checks_json, pr_updated_at, detected_via, detected_at, polled_at, created_at, updated_at";

pub fn is_status(raw: &str) -> bool {
    matches!(
        raw,
        STATUS_OPEN | STATUS_DRAFT | STATUS_MERGED | STATUS_CLOSED
    )
}

/// Record that this thread opened this pull request.
///
/// Idempotent on `(provider, repo, number)`: the second detection of the same
/// PR refreshes the URL (a detection from a branch listing has one; a detection
/// from stdout has a better one) and leaves the link and the first `detected_*`
/// stamp alone. Returns the row and whether this call is what created it — the
/// caller writes an Inbox card only for a genuinely new PR.
pub fn link_pr(conn: &Connection, new: &NewThreadPr) -> Result<(ThreadPrRow, bool), StoreError> {
    if new.repo.trim().is_empty() {
        return Err(StoreError::invalid("a linked PR needs a repo"));
    }
    if new.number <= 0 {
        return Err(StoreError::invalid("a linked PR needs a number"));
    }
    if let Some(existing) = get_pr(conn, &new.provider, &new.repo, new.number)? {
        // The URL is the one field a re-detection can legitimately improve:
        // `head-list` reconstructs it, `stdout` read it off `gh pr create`.
        if existing.url != new.url && !new.url.is_empty() {
            conn.execute(
                "UPDATE thread_prs SET url = ?2, updated_at = ?3 WHERE id = ?1",
                params![existing.id, new.url, now_utc()],
            )?;
            return Ok((get_pr_by_id(conn, &existing.id)?.unwrap_or(existing), false));
        }
        return Ok((existing, false));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO thread_prs (
            id, thread_id, provider, forge_host, repo, number, url, status,
            detected_via, detected_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            new.thread_id,
            new.provider,
            new.forge_host,
            new.repo,
            new.number,
            new.url,
            STATUS_OPEN,
            new.detected_via,
            now,
            now,
        ],
    )?;
    let row = get_pr_by_id(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))?;
    Ok((row, true))
}

/// Replace GitHub's half of the row with what the last poll saw.
///
/// Returns the row as it was *before* the write beside the row after it, so the
/// caller can decide what changed without re-reading. That comparison is the
/// only thing that can tell "checks are failing" from "checks have just started
/// failing", and only the second is worth an Inbox card.
pub fn apply_snapshot(
    conn: &Connection,
    id: &str,
    snapshot: &PrSnapshot,
) -> Result<(ThreadPrRow, ThreadPrRow), StoreError> {
    let before = get_pr_by_id(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    if !is_status(&snapshot.status) {
        return Err(StoreError::invalid(format!(
            "invalid pr status {}",
            snapshot.status
        )));
    }
    let now = now_utc();
    conn.execute(
        "UPDATE thread_prs SET
            title = ?2, status = ?3, check_state = ?4, review_state = ?5,
            head_ref = ?6, base_ref = ?7, additions = ?8, deletions = ?9,
            changed_files = ?10, checks_json = ?11, pr_updated_at = ?12,
            url = COALESCE(?13, url), polled_at = ?14, updated_at = ?14
         WHERE id = ?1",
        params![
            id,
            snapshot.title,
            snapshot.status,
            snapshot.check_state,
            snapshot.review_state,
            snapshot.head_ref,
            snapshot.base_ref,
            snapshot.additions,
            snapshot.deletions,
            snapshot.changed_files,
            snapshot.checks_json,
            snapshot.pr_updated_at,
            snapshot.url,
            now,
        ],
    )?;
    let after = get_pr_by_id(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    Ok((before, after))
}

pub fn get_pr(
    conn: &Connection,
    provider: &str,
    repo: &str,
    number: i64,
) -> Result<Option<ThreadPrRow>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM thread_prs
              WHERE provider = ?1 AND repo = ?2 AND number = ?3"
        ),
        params![provider, repo, number],
        map_thread_pr,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_pr_by_id(conn: &Connection, id: &str) -> Result<Option<ThreadPrRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM thread_prs WHERE id = ?1"),
        [id],
        map_thread_pr,
    )
    .optional()
    .map_err(Into::into)
}

/// Every linked PR whose thread still exists, newest activity first.
///
/// A deleted thread's PRs are already gone by cascade; the `deleted_at` filter
/// is for the tombstone `thread/delete` writes, which keeps the row.
pub fn list_prs(conn: &Connection) -> Result<Vec<ThreadPrRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM thread_prs
          WHERE thread_id IN (SELECT id FROM threads WHERE deleted_at IS NULL)
          ORDER BY COALESCE(pr_updated_at, updated_at) DESC, rowid DESC"
    ))?;
    let rows = stmt
        .query_map([], map_thread_pr)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_prs_for_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<ThreadPrRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM thread_prs WHERE thread_id = ?1
          ORDER BY number DESC"
    ))?;
    let rows = stmt
        .query_map([thread_id], map_thread_pr)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
