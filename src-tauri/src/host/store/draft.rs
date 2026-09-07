//! Durable bot drafts (#237). Submission is cheap; Save is atomic.

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::catalog::{get_bot, insert_bot, next_bot_sort_order};
use super::error::StoreError;
use super::models::{
    BotDraftPatch, BotDraftRow, BotRow, NewBot, NewBotDraft, DRAFT_DISMISSED, DRAFT_PENDING,
    DRAFT_SAVED, DRAFT_STALE,
};
use super::now_utc;

const COLUMNS: &str = "id, request_key, payload_hash, source_bot_id, source_thread_id, \
     source_run_id, name, instructions, tools_json, harness_id, color, template_id, \
     status, revision, bot_id, deciding_device_id, stale_reason, created_at, updated_at";

fn map_draft(row: &rusqlite::Row<'_>) -> rusqlite::Result<BotDraftRow> {
    Ok(BotDraftRow {
        id: row.get(0)?,
        request_key: row.get(1)?,
        payload_hash: row.get(2)?,
        source_bot_id: row.get(3)?,
        source_thread_id: row.get(4)?,
        source_run_id: row.get(5)?,
        name: row.get(6)?,
        instructions: row.get(7)?,
        tools_json: row.get(8)?,
        harness_id: row.get(9)?,
        color: row.get(10)?,
        template_id: row.get(11)?,
        status: row.get(12)?,
        revision: row.get(13)?,
        bot_id: row.get(14)?,
        deciding_device_id: row.get(15)?,
        stale_reason: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

pub fn get_draft(conn: &Connection, id: &str) -> Result<Option<BotDraftRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM bot_drafts WHERE id = ?1"),
        [id],
        map_draft,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_draft_by_request(
    conn: &Connection,
    source_bot_id: &str,
    request_key: &str,
) -> Result<Option<BotDraftRow>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM bot_drafts
              WHERE source_bot_id = ?1 AND request_key = ?2"
        ),
        params![source_bot_id, request_key],
        map_draft,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_reviewable_drafts(conn: &Connection) -> Result<Vec<BotDraftRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM bot_drafts
          WHERE status IN ('{DRAFT_PENDING}', '{DRAFT_STALE}')
          ORDER BY created_at"
    ))?;
    let rows = stmt
        .query_map([], map_draft)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_drafts_for_source(
    conn: &Connection,
    source_bot_id: &str,
) -> Result<Vec<BotDraftRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM bot_drafts
          WHERE source_bot_id = ?1
          ORDER BY created_at"
    ))?;
    let rows = stmt
        .query_map([source_bot_id], map_draft)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Insert, or return the existing row when the same source retries the same
/// key with the same payload. A changed payload on the same key is a conflict.
pub fn insert_draft(conn: &Connection, new: &NewBotDraft) -> Result<BotDraftRow, StoreError> {
    if let Some(existing) = get_draft_by_request(conn, &new.source_bot_id, &new.request_key)? {
        if existing.payload_hash == new.payload_hash {
            return Ok(existing);
        }
        return Err(StoreError::invalid(format!(
            "requestKey {} already has a different draft",
            new.request_key
        )));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO bot_drafts (
            id, request_key, payload_hash, source_bot_id, source_thread_id,
            source_run_id, name, instructions, tools_json, harness_id, color,
            template_id, status, revision, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14, ?14)",
        params![
            id,
            new.request_key,
            new.payload_hash,
            new.source_bot_id,
            new.source_thread_id,
            new.source_run_id,
            new.name,
            new.instructions,
            new.tools_json,
            new.harness_id,
            new.color,
            new.template_id,
            DRAFT_PENDING,
            now,
        ],
    )?;
    get_draft(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn mark_draft_stale(
    conn: &Connection,
    id: &str,
    reason: &str,
) -> Result<BotDraftRow, StoreError> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE bot_drafts SET status = ?2, stale_reason = ?3, updated_at = ?4
          WHERE id = ?1 AND status = ?5",
        params![id, DRAFT_STALE, reason, now, DRAFT_PENDING],
    )?;
    if changed == 0 {
        return get_draft(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()));
    }
    get_draft(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

pub fn dismiss_draft(
    conn: &Connection,
    id: &str,
    revision: i64,
) -> Result<BotDraftRow, StoreError> {
    let now = now_utc();
    let changed = conn.execute(
        "UPDATE bot_drafts SET status = ?2, revision = revision + 1, updated_at = ?3
          WHERE id = ?1 AND revision = ?4 AND status IN (?5, ?6)",
        params![id, DRAFT_DISMISSED, now, revision, DRAFT_PENDING, DRAFT_STALE],
    )?;
    if changed == 0 {
        let current = get_draft(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))?;
        if current.status == DRAFT_DISMISSED {
            return Ok(current);
        }
        return Err(StoreError::invalid(
            "draft revision conflict; reload the proposal",
        ));
    }
    get_draft(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

fn apply_patch(tx: &Connection, id: &str, revision: i64, patch: &BotDraftPatch) -> Result<(), StoreError> {
    if patch.name.is_none()
        && patch.instructions.is_none()
        && patch.tools_json.is_none()
        && patch.harness_id.is_none()
        && patch.color.is_none()
    {
        return Ok(());
    }
    let now = now_utc();
    let changed = tx.execute(
        "UPDATE bot_drafts SET
            name = COALESCE(?2, name),
            instructions = COALESCE(?3, instructions),
            tools_json = COALESCE(?4, tools_json),
            harness_id = COALESCE(?5, harness_id),
            color = COALESCE(?6, color),
            updated_at = ?7
          WHERE id = ?1 AND revision = ?8 AND status IN (?9, ?10)",
        params![
            id,
            patch.name.as_deref(),
            patch.instructions.as_deref(),
            patch.tools_json.as_deref(),
            patch.harness_id.as_deref(),
            patch.color.as_deref(),
            now,
            revision,
            DRAFT_PENDING,
            DRAFT_STALE,
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::invalid(
            "draft revision conflict; reload the proposal",
        ));
    }
    Ok(())
}

/// Create the bot and mark the draft saved in one transaction. A retry after
/// a lost response, or a second window, returns the same bot.
pub fn save_draft(
    conn: &Connection,
    id: &str,
    revision: i64,
    patch: &BotDraftPatch,
    deciding_device_id: Option<&str>,
    new_bot: Option<&NewBot>,
) -> Result<(BotDraftRow, BotRow), StoreError> {
    // Immediate so two windows cannot each insert a bot before either marks
    // the draft saved. A lost-response retry then sees `saved` and returns
    // the same row.
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let current = get_draft(&tx, id)?.ok_or_else(|| StoreError::NotFound(id.into()))?;
    if current.status == DRAFT_SAVED {
        let bot_id = current
            .bot_id
            .clone()
            .ok_or_else(|| StoreError::invalid("saved draft is missing its bot"))?;
        let bot = get_bot(&tx, &bot_id)?.ok_or_else(|| StoreError::NotFound(bot_id))?;
        return Ok((current, bot));
    }
    if current.status == DRAFT_DISMISSED {
        return Err(StoreError::invalid("a dismissed draft cannot be saved"));
    }
    if current.revision != revision {
        return Err(StoreError::invalid(
            "draft revision conflict; reload the proposal",
        ));
    }
    apply_patch(&tx, id, revision, patch)?;
    let prepared = get_draft(&tx, id)?.ok_or_else(|| StoreError::NotFound(id.into()))?;
    let bot = match new_bot {
        Some(new) => insert_bot(&tx, new)?,
        None => {
            let sort = next_bot_sort_order(&tx)?;
            insert_bot(
                &tx,
                &NewBot {
                    name: prepared.name.clone(),
                    color: prepared.color.clone(),
                    instructions: prepared.instructions.clone(),
                    tools_json: prepared.tools_json.clone(),
                    harness_id: prepared.harness_id.clone(),
                    template_id: prepared.template_id.clone(),
                    sort_order: sort,
                    image: None,
                },
            )?
        }
    };
    let now = now_utc();
    let changed = tx.execute(
        "UPDATE bot_drafts SET
            status = ?2,
            bot_id = ?3,
            deciding_device_id = ?4,
            revision = revision + 1,
            updated_at = ?5
          WHERE id = ?1 AND revision = ?6 AND status IN (?7, ?8)",
        params![
            id,
            DRAFT_SAVED,
            bot.id,
            deciding_device_id,
            now,
            prepared.revision,
            DRAFT_PENDING,
            DRAFT_STALE,
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::invalid(
            "draft revision conflict; reload the proposal",
        ));
    }
    let saved = get_draft(&tx, id)?.ok_or_else(|| StoreError::NotFound(id.into()))?;
    tx.commit()?;
    Ok((saved, bot))
}

/// Rewrite receipts for threads owned by `bot_id` so an additive host-tool
/// grant does not orphan a standing conversation (#237 §6).
pub fn align_receipts_tools(
    conn: &Connection,
    bot_id: &str,
    tools_json: &str,
) -> Result<usize, StoreError> {
    let changed = conn.execute(
        "UPDATE session_receipts SET tools_json = ?2
          WHERE thread_id IN (SELECT id FROM threads WHERE bot_id = ?1)",
        params![bot_id, tools_json],
    )?;
    Ok(changed)
}
