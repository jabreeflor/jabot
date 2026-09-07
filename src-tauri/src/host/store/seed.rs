//! Built-in harness catalog and core crew. Idempotent: builtins upsert;
//! crew rows are inserted only on an empty `bots` table.
//!
//! The harness rows are written from the runtime catalog in `host/harness`
//! rather than a second list here: `threads.harness_id` is a foreign key, so
//! every compiled-in card has to exist as a row before a thread can name it,
//! and two hand-maintained lists of the same cards would drift the first time
//! one of them gained a preset.

use rusqlite::{params, Connection};

use super::super::harness::catalog::compiled_in;
use super::error::StoreError;
use super::now_utc;

struct SeedBot {
    id: &'static str,
    name: &'static str,
    color: &'static str,
    instructions: &'static str,
    tools_json: &'static str,
    is_chief: i64,
    sort_order: i64,
}

/// The two seats every empty crew starts with. Default harness is `claude`
/// until the user picks otherwise (#6, #184).
const SEED_BOTS: &[SeedBot] = &[
    SeedBot {
        id: "chief",
        name: "Chief",
        color: "b-teal",
        instructions:
            "Route work across the crew. Fold long tasks away, surface only what matters.",
        tools_json: r#"["handoff_to_bot","spawn_code_session","fold_thread","list_crew_status"]"#,
        is_chief: 1,
        sort_order: 0,
    },
    SeedBot {
        id: "bot-recruiter",
        name: "Bot Recruiter",
        color: "b-purple",
        instructions: "Help me shape and add the right bots for the work I need.",
        tools_json: "[]",
        is_chief: 0,
        sort_order: 1,
    },
];

pub fn seed(conn: &Connection) -> Result<(), StoreError> {
    seed_harnesses(conn)?;
    seed_bots(conn)?;
    seed_app_meta(conn)?;
    Ok(())
}

fn seed_harnesses(conn: &Connection) -> Result<(), StoreError> {
    let now = now_utc();
    for descriptor in compiled_in() {
        let launch = descriptor.primary();
        let args_json = serde_json::to_string(&launch.args)?;
        let env_json = serde_json::to_string(&descriptor.env)?;
        conn.execute(
            "INSERT INTO harnesses (
                id, label, command, args_json, env_json, install_hint,
                is_builtin, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)
             ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                command = excluded.command,
                args_json = excluded.args_json,
                env_json = excluded.env_json,
                install_hint = excluded.install_hint,
                updated_at = excluded.updated_at
             WHERE harnesses.is_builtin = 1",
            params![
                descriptor.id,
                descriptor.label,
                launch.command,
                args_json,
                env_json,
                descriptor.install_hint,
                now
            ],
        )?;
    }
    Ok(())
}

fn seed_bots(conn: &Connection) -> Result<(), StoreError> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM bots", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let now = now_utc();
    for bot in SEED_BOTS {
        conn.execute(
            "INSERT INTO bots (
                id, name, color, instructions, tools_json, harness_id,
                is_chief, template_id, host_id, sort_order, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'claude', ?6, NULL, NULL, ?7, ?8, ?8)",
            params![
                bot.id,
                bot.name,
                bot.color,
                bot.instructions,
                bot.tools_json,
                bot.is_chief,
                bot.sort_order,
                now
            ],
        )?;
    }
    Ok(())
}

fn seed_app_meta(conn: &Connection) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO app_meta (key, value) VALUES ('purge_deleted_after_days', '30')",
        [],
    )?;
    Ok(())
}
