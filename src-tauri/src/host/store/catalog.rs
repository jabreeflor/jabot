//! Folders, harnesses, and crew rows.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{BotRow, FolderRow, HarnessRow};
use super::{env_key_looks_secret, map_bot, map_folder, map_harness, now_utc};

pub fn insert_folder(
    conn: &Connection,
    name: &str,
    path: &str,
    sort_order: i64,
) -> Result<FolderRow, StoreError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(StoreError::invalid("folder name and path are required"));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO folders (id, name, path, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, name, path, sort_order, now],
    )?;
    get_folder(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_folder(conn: &Connection, id: &str) -> Result<Option<FolderRow>, StoreError> {
    conn.query_row(
        "SELECT id, name, path, sort_order, created_at, updated_at FROM folders WHERE id = ?1",
        [id],
        map_folder,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_folders(conn: &Connection) -> Result<Vec<FolderRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, sort_order, created_at, updated_at
         FROM folders ORDER BY sort_order, name",
    )?;
    let rows = stmt
        .query_map([], map_folder)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_harnesses(conn: &Connection) -> Result<Vec<HarnessRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, label, command, args_json, env_json, install_hint, is_builtin,
                created_at, updated_at
         FROM harnesses ORDER BY is_builtin DESC, label",
    )?;
    let rows = stmt
        .query_map([], map_harness)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_harness(conn: &Connection, id: &str) -> Result<Option<HarnessRow>, StoreError> {
    conn.query_row(
        "SELECT id, label, command, args_json, env_json, install_hint, is_builtin,
                created_at, updated_at
         FROM harnesses WHERE id = ?1",
        [id],
        map_harness,
    )
    .optional()
    .map_err(Into::into)
}

/// Register a tier-3 harness so threads can point at it.
///
/// The row exists for the foreign key, not as the catalog: the JSON file the
/// user wrote stays the source of truth for label, args, and env, so this
/// overwrites its own row on every sync. `is_builtin` stays 0 — the seed's
/// upsert refuses to touch user rows, and this one refuses to touch builtins,
/// so neither tier can overwrite the other.
pub fn upsert_custom_harness(
    conn: &Connection,
    id: &str,
    label: &str,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    install_hint: Option<&str>,
) -> Result<HarnessRow, StoreError> {
    if id.trim().is_empty() || label.trim().is_empty() || command.trim().is_empty() {
        return Err(StoreError::invalid(
            "harness id, label, and command are required",
        ));
    }
    // Same rule as `runtime_json`: a credential in the catalog is a credential
    // in every bug report and backup that catalog ends up in.
    if let Some(key) = env.keys().find(|key| env_key_looks_secret(key)) {
        return Err(StoreError::invalid(format!(
            "harness env must not contain secret key {key}"
        )));
    }
    let args_json = serde_json::to_string(args)?;
    let env_json = serde_json::to_string(env)?;
    let now = now_utc();
    conn.execute(
        "INSERT INTO harnesses (
            id, label, command, args_json, env_json, install_hint,
            is_builtin, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET
            label = excluded.label,
            command = excluded.command,
            args_json = excluded.args_json,
            env_json = excluded.env_json,
            install_hint = excluded.install_hint,
            updated_at = excluded.updated_at
         WHERE harnesses.is_builtin = 0",
        params![id, label, command, args_json, env_json, install_hint, now],
    )?;
    get_harness(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))
}

pub fn list_bots(conn: &Connection) -> Result<Vec<BotRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, instructions, tools_json, harness_id, is_chief,
                template_id, host_id, sort_order, created_at, updated_at
         FROM bots ORDER BY is_chief DESC, sort_order, name",
    )?;
    let rows = stmt
        .query_map([], map_bot)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_bot(conn: &Connection, id: &str) -> Result<Option<BotRow>, StoreError> {
    conn.query_row(
        "SELECT id, name, color, instructions, tools_json, harness_id, is_chief,
                template_id, host_id, sort_order, created_at, updated_at
         FROM bots WHERE id = ?1",
        [id],
        map_bot,
    )
    .optional()
    .map_err(Into::into)
}
