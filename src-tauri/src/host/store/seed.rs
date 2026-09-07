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
/// Exact pre-#237 defaults, so an upgrade can recognise an untouched shipped
/// seat. Customized instructions or tool lists are left alone.
const LEGACY_CHIEF_INSTRUCTIONS: &str =
    "Route work across the crew. Fold long tasks away, surface only what matters.";
const LEGACY_CHIEF_TOOLS: &str =
    r#"["handoff_to_bot","spawn_code_session","fold_thread","list_crew_status"]"#;
const LEGACY_RECRUITER_INSTRUCTIONS: &str =
    "Help me shape and add the right bots for the work I need.";
const LEGACY_RECRUITER_TOOLS: &str = "[]";

const CHIEF_INSTRUCTIONS: &str =
    "Route work across the crew. Fold long tasks away, surface only what matters. \
     When the user asks for a new crew member, propose one with draft_bot; they must Save it.";
const CHIEF_TOOLS: &str = r#"["handoff_to_bot","spawn_code_session","fold_thread","list_crew_status","draft_bot","get_bot_draft"]"#;
const RECRUITER_INSTRUCTIONS: &str =
    "Help me shape and add the right bots for the work I need. When they ask for a new crew member, \
     propose one with draft_bot. Submitting a draft does not create or launch the bot; the user must Save it.";
const RECRUITER_TOOLS: &str = r#"["draft_bot","get_bot_draft"]"#;

const SEED_BOTS: &[SeedBot] = &[
    SeedBot {
        id: "chief",
        name: "Chief",
        color: "b-teal",
        instructions: CHIEF_INSTRUCTIONS,
        tools_json: CHIEF_TOOLS,
        is_chief: 1,
        sort_order: 0,
    },
    SeedBot {
        id: "bot-recruiter",
        name: "Bot Recruiter",
        color: "b-purple",
        instructions: RECRUITER_INSTRUCTIONS,
        tools_json: RECRUITER_TOOLS,
        is_chief: 0,
        sort_order: 1,
    },
];

const DRAFT_TOOLS_META: &str = "crew_draft_tools_v1";

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

/// Grant `draft_bot` / `get_bot_draft` to untouched shipped Chief and Recruiter
/// rows on an existing install. Customized seats and a deleted Recruiter are
/// left alone. Receipts are rewritten so the additive grant does not look like
/// session drift.
pub fn upgrade_draft_tools(conn: &Connection) -> Result<(), StoreError> {
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM app_meta WHERE key = ?1",
        [DRAFT_TOOLS_META],
        |row| row.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }
    let now = now_utc();
    grant_untouched(
        conn,
        "chief",
        "Chief",
        LEGACY_CHIEF_INSTRUCTIONS,
        LEGACY_CHIEF_TOOLS,
        CHIEF_INSTRUCTIONS,
        CHIEF_TOOLS,
        &now,
    )?;
    grant_untouched(
        conn,
        "bot-recruiter",
        "Bot Recruiter",
        LEGACY_RECRUITER_INSTRUCTIONS,
        LEGACY_RECRUITER_TOOLS,
        RECRUITER_INSTRUCTIONS,
        RECRUITER_TOOLS,
        &now,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO app_meta (key, value) VALUES (?1, '1')",
        [DRAFT_TOOLS_META],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn grant_untouched(
    conn: &Connection,
    id: &str,
    name: &str,
    old_instructions: &str,
    old_tools: &str,
    new_instructions: &str,
    new_tools: &str,
    now: &str,
) -> Result<(), StoreError> {
    let changed = conn.execute(
        "UPDATE bots SET instructions = ?2, tools_json = ?3, updated_at = ?4
          WHERE id = ?1 AND name = ?5 AND instructions = ?6 AND tools_json = ?7",
        params![
            id,
            new_instructions,
            new_tools,
            now,
            name,
            old_instructions,
            old_tools
        ],
    )?;
    if changed > 0 {
        super::draft::align_receipts_tools(conn, id, new_tools)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::migrate;
    use rusqlite::Connection;

    fn open_conn() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(dir.path().join("jabot.sqlite")).unwrap();
        migrate::migrate(&mut conn).unwrap();
        seed(&conn).unwrap();
        upgrade_draft_tools(&conn).unwrap();
        (dir, conn)
    }

    fn tools(conn: &Connection, id: &str) -> String {
        conn.query_row(
            "SELECT tools_json FROM bots WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn instructions(conn: &Connection, id: &str) -> String {
        conn.query_row(
            "SELECT instructions FROM bots WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_fresh_crew_ships_draft_tools_on_chief_and_recruiter() {
        let (_dir, conn) = open_conn();
        assert!(tools(&conn, "chief").contains("draft_bot"));
        assert!(tools(&conn, "bot-recruiter").contains("draft_bot"));
        assert!(instructions(&conn, "chief").contains("draft_bot"));
    }

    #[test]
    fn upgrade_grants_untouched_defaults_and_skips_customized_or_deleted() {
        let (_dir, conn) = open_conn();
        conn.execute("DELETE FROM app_meta WHERE key = ?1", [DRAFT_TOOLS_META])
            .unwrap();
        conn.execute(
            "UPDATE bots SET instructions = ?1, tools_json = ?2 WHERE id = 'chief'",
            params![LEGACY_CHIEF_INSTRUCTIONS, LEGACY_CHIEF_TOOLS],
        )
        .unwrap();
        conn.execute(
            "UPDATE bots SET instructions = 'Custom recruiter.', tools_json = '[]' WHERE id = 'bot-recruiter'",
            [],
        )
        .unwrap();
        upgrade_draft_tools(&conn).unwrap();
        assert!(tools(&conn, "chief").contains("draft_bot"));
        assert_eq!(instructions(&conn, "bot-recruiter"), "Custom recruiter.");
        assert_eq!(tools(&conn, "bot-recruiter"), "[]");

        conn.execute("DELETE FROM app_meta WHERE key = ?1", [DRAFT_TOOLS_META])
            .unwrap();
        conn.execute("DELETE FROM bots WHERE id = 'bot-recruiter'", [])
            .unwrap();
        upgrade_draft_tools(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bots WHERE id = 'bot-recruiter'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn upgrade_is_idempotent() {
        let (_dir, conn) = open_conn();
        upgrade_draft_tools(&conn).unwrap();
        upgrade_draft_tools(&conn).unwrap();
        assert!(tools(&conn, "chief").contains("get_bot_draft"));
    }
}
