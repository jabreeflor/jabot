//! Built-in harness catalog and core crew. Idempotent: builtins upsert;
//! crew rows are inserted only on an empty `bots` table.

use rusqlite::{params, Connection};

use super::error::StoreError;
use super::now_utc;

struct BuiltinHarness {
    id: &'static str,
    label: &'static str,
    command: &'static str,
    args_json: &'static str,
    install_hint: &'static str,
}

struct SeedBot {
    id: &'static str,
    name: &'static str,
    color: &'static str,
    instructions: &'static str,
    tools_json: &'static str,
    is_chief: i64,
    sort_order: i64,
}

/// Compiled-in New Chat cards (Buzz tier 1). Custom JSON is user-added (#13).
const BUILTIN_HARNESSES: &[BuiltinHarness] = &[
    BuiltinHarness {
        id: "claude",
        label: "Claude Code",
        command: "claude-agent-acp",
        args_json: "[]",
        install_hint: "Install Claude Code, then `claude-agent-acp` on PATH.",
    },
    BuiltinHarness {
        id: "codex",
        label: "Codex",
        command: "codex-acp",
        args_json: "[]",
        install_hint: "Install Codex, then `codex-acp` on PATH.",
    },
    BuiltinHarness {
        id: "pi",
        label: "Pi",
        command: "pi-acp",
        args_json: "[]",
        install_hint: "Install Pi, then `pi-acp` on PATH.",
    },
];

/// Prototype CREW[]. Default harness is `claude` until the user picks otherwise (#6).
const SEED_BOTS: &[SeedBot] = &[
    SeedBot {
        id: "chief",
        name: "Chief",
        color: "b-teal",
        instructions: "Route work across the crew. Fold long tasks away, surface only what matters.",
        tools_json: r#"["handoff_to_bot","spawn_code_session","fold_thread","list_crew_status"]"#,
        is_chief: 1,
        sort_order: 0,
    },
    SeedBot {
        id: "code",
        name: "Code",
        color: "b-yellow",
        instructions: "Run coding sessions in my repos. Open PRs, never push to main.",
        tools_json: r#"["github","terminal"]"#,
        is_chief: 0,
        sort_order: 1,
    },
    SeedBot {
        id: "inboxm",
        name: "Inbox Mgr",
        color: "b-purple",
        instructions: "Keep Gmail at zero. Park drafts for anything that needs my voice.",
        tools_json: r#"["gmail"]"#,
        is_chief: 0,
        sort_order: 2,
    },
    SeedBot {
        id: "sched",
        name: "Scheduler",
        color: "b-violet",
        instructions: "Guard the calendar. Fix conflicts, protect deep-work mornings.",
        tools_json: r#"["calendar"]"#,
        is_chief: 0,
        sort_order: 3,
    },
    SeedBot {
        id: "rsrch",
        name: "Research",
        color: "b-blue",
        instructions: "Dig sources, pull context into GlobNet, brief me short.",
        tools_json: r#"["browser","notion"]"#,
        is_chief: 0,
        sort_order: 4,
    },
    SeedBot {
        id: "writer",
        name: "Writer",
        color: "b-orange",
        instructions: "Draft in my voice: plain, short, no filler.",
        tools_json: r#"["gmail","notion"]"#,
        is_chief: 0,
        sort_order: 5,
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
    for harness in BUILTIN_HARNESSES {
        conn.execute(
            "INSERT INTO harnesses (
                id, label, command, args_json, env_json, install_hint,
                is_builtin, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, '{}', ?5, 1, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                command = excluded.command,
                args_json = excluded.args_json,
                install_hint = excluded.install_hint,
                updated_at = excluded.updated_at
             WHERE harnesses.is_builtin = 1",
            params![
                harness.id,
                harness.label,
                harness.command,
                harness.args_json,
                harness.install_hint,
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
