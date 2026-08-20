//! Folders, harnesses, and crew rows.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{BotRow, FolderPatch, FolderRow, HarnessRow, NewFolder};
use super::{env_key_looks_secret, map_bot, map_folder, map_harness, now_utc};

/// Column order for [`map_folder`], in one place so a new column lands in every
/// read at once (same reason `overlay.rs` keeps `THREAD_COLUMNS`).
const FOLDER_COLUMNS: &str = "id, name, path, sort_order, created_at, updated_at, repo_root, \
     origin_url, forge_host, repo_owner, repo_name, default_branch, setup_command, \
     files_to_copy_json";

pub fn insert_folder(conn: &Connection, new: &NewFolder) -> Result<FolderRow, StoreError> {
    if new.name.trim().is_empty() || new.path.trim().is_empty() {
        return Err(StoreError::invalid("folder name and path are required"));
    }
    serde_json::from_str::<Vec<String>>(&new.files_to_copy_json)?;
    let id = Uuid::new_v4().to_string();
    let now = now_utc();
    conn.execute(
        "INSERT INTO folders (
            id, name, path, sort_order, created_at, updated_at, repo_root, origin_url,
            forge_host, repo_owner, repo_name, default_branch, setup_command, files_to_copy_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            id,
            new.name,
            new.path,
            new.sort_order,
            now,
            new.repo_root,
            new.origin_url,
            new.forge_host,
            new.repo_owner,
            new.repo_name,
            new.default_branch,
            new.setup_command,
            new.files_to_copy_json,
        ],
    )?;
    get_folder(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_folder(conn: &Connection, id: &str) -> Result<Option<FolderRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE id = ?1"),
        [id],
        map_folder,
    )
    .optional()
    .map_err(Into::into)
}

/// The two ways a directory is already registered: it is the same path, or it
/// is a different path inside the same checkout. Both have to be found *before*
/// an insert, so the answer is "here is the folder you already have" rather
/// than a unique-constraint failure the user cannot act on.
pub fn find_folder_by_path(conn: &Connection, path: &str) -> Result<Option<FolderRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE path = ?1"),
        [path],
        map_folder,
    )
    .optional()
    .map_err(Into::into)
}

pub fn find_folder_by_repo_root(
    conn: &Connection,
    repo_root: &str,
) -> Result<Option<FolderRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {FOLDER_COLUMNS} FROM folders WHERE repo_root = ?1"),
        [repo_root],
        map_folder,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_folders(conn: &Connection) -> Result<Vec<FolderRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FOLDER_COLUMNS} FROM folders ORDER BY sort_order, name"
    ))?;
    let rows = stmt
        .query_map([], map_folder)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Where a new folder goes in the sidebar: after the ones already there.
pub fn next_folder_sort_order(conn: &Connection) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM folders",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Rename, edit the setup script, or re-record what git says today. Each
/// column moves only when the patch names it — a rename must not silently
/// clear the setup command the user wrote last week.
pub fn update_folder(
    conn: &Connection,
    id: &str,
    patch: &FolderPatch,
) -> Result<FolderRow, StoreError> {
    if let Some(name) = &patch.name {
        if name.trim().is_empty() {
            return Err(StoreError::invalid("folder name must be non-empty"));
        }
    }
    if let Some(files) = &patch.files_to_copy_json {
        serde_json::from_str::<Vec<String>>(files)?;
    }
    let now = now_utc();
    let repo = patch.repo.clone().unwrap_or_default();
    let changed = conn.execute(
        "UPDATE folders SET
            name = COALESCE(?2, name),
            setup_command = CASE WHEN ?3 THEN ?4 ELSE setup_command END,
            files_to_copy_json = COALESCE(?5, files_to_copy_json),
            repo_root = CASE WHEN ?6 THEN ?7 ELSE repo_root END,
            origin_url = CASE WHEN ?6 THEN ?8 ELSE origin_url END,
            forge_host = CASE WHEN ?6 THEN ?9 ELSE forge_host END,
            repo_owner = CASE WHEN ?6 THEN ?10 ELSE repo_owner END,
            repo_name = CASE WHEN ?6 THEN ?11 ELSE repo_name END,
            default_branch = CASE WHEN ?6 THEN ?12 ELSE default_branch END,
            updated_at = ?13
         WHERE id = ?1",
        params![
            id,
            patch.name,
            patch.setup_command.is_some(),
            patch.setup_command.clone().flatten(),
            patch.files_to_copy_json,
            patch.repo.is_some(),
            repo.repo_root,
            repo.origin_url,
            repo.forge_host,
            repo.repo_owner,
            repo.repo_name,
            repo.default_branch,
            now,
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    get_folder(conn, id)?.ok_or_else(|| StoreError::NotFound(id.into()))
}

/// Forget a folder without touching the directory it points at.
///
/// The threads survive: `threads.folder_id` is `ON DELETE SET NULL`, and each
/// row already carries its own `cwd`, `repo_root` and `repo` from spawn — so a
/// session started here keeps working in the checkout it was started in, and
/// keeps saying which repo that is, after the sidebar row is gone.
pub fn delete_folder(conn: &Connection, id: &str) -> Result<usize, StoreError> {
    let detached: i64 = conn.query_row(
        "SELECT COUNT(*) FROM threads WHERE folder_id = ?1 AND deleted_at IS NULL",
        [id],
        |row| row.get(0),
    )?;
    let changed = conn.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StoreError::NotFound(id.into()));
    }
    Ok(detached as usize)
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
