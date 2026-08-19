//! Host-owned SQLite store (WAL, single writer) and secrets vault glue.
//!
//! The renderer never opens this file. The host process is the only writer
//! (`docs/research/data-and-persistence/store.md`).

mod catalog;
mod error;
mod migrate;
mod models;
mod overlay;
mod secrets;
mod seed;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Row};

pub use error::StoreError;
pub use models::*;
pub use secrets::{Secrets, SecretsBackend};

const MIN_SQLITE: (u32, u32, u32) = (3, 51, 3);
const UNCLEAN_SUFFIX: &str = ".unclean";

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(&path)?;
        apply_pragmas(&conn)?;
        verify_sqlite_version()?;
        let marker = unclean_marker(&path);
        if marker.exists() {
            integrity_check(&conn)?;
        }
        std::fs::write(&marker, b"open")?;
        migrate::migrate(&mut conn)?;
        seed::seed(&conn)?;
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i32, StoreError> {
        migrate::schema_version(&self.conn)
    }

    pub fn journal_mode(&self) -> Result<String, StoreError> {
        let mode: String = self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        Ok(mode.to_ascii_lowercase())
    }

    pub fn status(&self, secrets: &Secrets) -> Result<StoreStatus, StoreError> {
        let harness_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM harnesses", [], |row| row.get(0))?;
        let bot_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM bots", [], |row| row.get(0))?;
        Ok(StoreStatus {
            path: self.path.display().to_string(),
            schema_version: self.schema_version()?,
            sqlite_version: rusqlite::version().to_string(),
            journal_mode: self.journal_mode()?,
            secrets_backend: secrets.backend().as_str().to_string(),
            harness_count,
            bot_count,
        })
    }

    pub fn checkpoint(&self) -> Result<(), StoreError> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let marker = unclean_marker(&self.path);
        if marker.exists() {
            std::fs::remove_file(marker)?;
        }
        Ok(())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderRow>, StoreError> {
        catalog::list_folders(&self.conn)
    }

    pub fn insert_folder(
        &self,
        name: &str,
        path: &str,
        sort_order: i64,
    ) -> Result<FolderRow, StoreError> {
        catalog::insert_folder(&self.conn, name, path, sort_order)
    }

    pub fn list_harnesses(&self) -> Result<Vec<HarnessRow>, StoreError> {
        catalog::list_harnesses(&self.conn)
    }

    pub fn get_harness(&self, id: &str) -> Result<Option<HarnessRow>, StoreError> {
        catalog::get_harness(&self.conn, id)
    }

    pub fn get_folder(&self, id: &str) -> Result<Option<FolderRow>, StoreError> {
        catalog::get_folder(&self.conn, id)
    }

    pub fn list_bots(&self) -> Result<Vec<BotRow>, StoreError> {
        catalog::list_bots(&self.conn)
    }

    pub fn get_bot(&self, id: &str) -> Result<Option<BotRow>, StoreError> {
        catalog::get_bot(&self.conn, id)
    }

    pub fn insert_thread(&self, new: &NewThread) -> Result<ThreadRow, StoreError> {
        overlay::insert_thread(&self.conn, new)
    }

    pub fn get_thread(&self, id: &str) -> Result<Option<ThreadRow>, StoreError> {
        overlay::get_thread(&self.conn, id)
    }

    pub fn set_thread_acp_session(
        &self,
        id: &str,
        acp_session_id: &str,
    ) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_acp_session(&self.conn, id, acp_session_id)
    }

    pub fn list_threads_by_state(&self, state: &str) -> Result<Vec<ThreadRow>, StoreError> {
        overlay::list_threads_by_state(&self.conn, state)
    }

    pub fn set_thread_state(&self, id: &str, state: &str) -> Result<ThreadRow, StoreError> {
        overlay::set_thread_state(&self.conn, id, state)
    }

    pub fn insert_run(
        &self,
        thread_id: &str,
        kind: &str,
        trigger_json: Option<&str>,
    ) -> Result<RunRow, StoreError> {
        overlay::insert_run(&self.conn, thread_id, kind, trigger_json)
    }

    pub fn set_run_state(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<RunRow, StoreError> {
        overlay::set_run_state(&self.conn, id, state, error)
    }

    pub fn append_transcript(
        &self,
        thread_id: &str,
        acp_method: &str,
        payload_json: &str,
    ) -> Result<TranscriptEventRow, StoreError> {
        overlay::append_transcript(&self.conn, thread_id, acp_method, payload_json)
    }

    pub fn transcript_after(
        &self,
        thread_id: &str,
        seq: i64,
    ) -> Result<Vec<TranscriptEventRow>, StoreError> {
        overlay::transcript_after(&self.conn, thread_id, seq)
    }

    pub fn insert_inbox_event(
        &self,
        thread_id: &str,
        run_id: Option<&str>,
        kind: &str,
        title: &str,
        summary: &str,
        payload_json: Option<&str>,
    ) -> Result<InboxEventRow, StoreError> {
        overlay::insert_inbox_event(
            &self.conn,
            thread_id,
            run_id,
            kind,
            title,
            summary,
            payload_json,
        )
    }

    /// Store secret bytes in the vault; SQLite keeps only the pointer.
    pub fn put_secret(
        &self,
        secrets: &mut Secrets,
        kind: &str,
        label: &str,
        secret: &str,
        bot_id: Option<&str>,
    ) -> Result<SecretRefRow, StoreError> {
        if secret.is_empty() {
            return Err(StoreError::invalid("secret bytes must be non-empty"));
        }
        if matches!(secrets.backend(), SecretsBackend::Unavailable) {
            return Err(StoreError::SecretsUnavailable);
        }
        let row = secrets::insert_secret_ref(&self.conn, kind, label, bot_id)?;
        if let Err(err) = secrets.put(&row.account, secret) {
            let _ = secrets::delete_secret_ref(&self.conn, &row.id);
            return Err(err);
        }
        Ok(row)
    }

    pub fn get_secret(&self, secrets: &Secrets, id: &str) -> Result<String, StoreError> {
        let row = secrets::get_secret_ref(&self.conn, id)?
            .ok_or_else(|| StoreError::NotFound(id.into()))?;
        secrets.get(&row.account)
    }

    pub fn delete_secret(&self, secrets: &mut Secrets, id: &str) -> Result<(), StoreError> {
        let Some(row) = secrets::delete_secret_ref(&self.conn, id)? else {
            return Err(StoreError::NotFound(id.into()));
        };
        secrets.delete(&row.account)
    }

    pub fn list_secret_refs(&self) -> Result<Vec<SecretRefRow>, StoreError> {
        secrets::list_secret_refs(&self.conn)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(err) = self.checkpoint() {
            eprintln!(
                "failed to checkpoint sqlite store at {}: {err}",
                self.path.display()
            );
        }
    }
}

fn apply_pragmas(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", "5000")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}

fn verify_sqlite_version() -> Result<(), StoreError> {
    let found = rusqlite::version();
    let Some(parsed) = parse_sqlite_version(found) else {
        return Err(StoreError::SqliteTooOld {
            found: found.to_string(),
        });
    };
    if parsed < MIN_SQLITE {
        return Err(StoreError::SqliteTooOld {
            found: found.to_string(),
        });
    }
    Ok(())
}

fn parse_sqlite_version(raw: &str) -> Option<(u32, u32, u32)> {
    let mut parts = raw.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let patch: u32 = patch_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn integrity_check(conn: &Connection) -> Result<(), StoreError> {
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(StoreError::Integrity(result));
    }
    Ok(())
}

fn unclean_marker(db_path: &Path) -> PathBuf {
    let mut marker = db_path.as_os_str().to_os_string();
    marker.push(UNCLEAN_SUFFIX);
    PathBuf::from(marker)
}

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn secret_account(id: &str) -> String {
    format!("jabot.secret.{id}")
}

pub fn validate_runtime_json(raw: &str) -> Result<(), StoreError> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let obj = value
        .as_object()
        .ok_or_else(|| StoreError::invalid("runtime_json must be an object"))?;
    let command = obj
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim();
    if command.is_empty() {
        return Err(StoreError::invalid("runtime_json.command is required"));
    }
    if let Some(env) = obj.get("env") {
        let env = env
            .as_object()
            .ok_or_else(|| StoreError::invalid("runtime_json.env must be an object"))?;
        for key in env.keys() {
            if env_key_looks_secret(key) {
                return Err(StoreError::invalid(format!(
                    "runtime_json.env must not contain secret key {key}"
                )));
            }
        }
    }
    Ok(())
}

fn env_key_looks_secret(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("API_KEY")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("ACCESS_KEY")
        || upper.ends_with("_TOKEN")
        || upper == "TOKEN"
}

pub(crate) fn map_folder(row: &Row<'_>) -> rusqlite::Result<FolderRow> {
    Ok(FolderRow {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub(crate) fn map_harness(row: &Row<'_>) -> rusqlite::Result<HarnessRow> {
    Ok(HarnessRow {
        id: row.get(0)?,
        label: row.get(1)?,
        command: row.get(2)?,
        args_json: row.get(3)?,
        env_json: row.get(4)?,
        install_hint: row.get(5)?,
        is_builtin: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub(crate) fn map_bot(row: &Row<'_>) -> rusqlite::Result<BotRow> {
    Ok(BotRow {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        instructions: row.get(3)?,
        tools_json: row.get(4)?,
        harness_id: row.get(5)?,
        is_chief: row.get::<_, i64>(6)? != 0,
        template_id: row.get(7)?,
        host_id: row.get(8)?,
        sort_order: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub(crate) fn map_thread(row: &Row<'_>) -> rusqlite::Result<ThreadRow> {
    Ok(ThreadRow {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        bot_id: row.get(2)?,
        harness_id: row.get(3)?,
        acp_session_id: row.get(4)?,
        native_session_ref: row.get(5)?,
        cwd: row.get(6)?,
        runtime_json: row.get(7)?,
        title: row.get(8)?,
        state: row.get(9)?,
        fold_policy: row.get(10)?,
        last_stop_reason: row.get(11)?,
        last_error: row.get(12)?,
        preview: row.get(13)?,
        worktree_path: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        folded_at: row.get(17)?,
        resurfaced_at: row.get(18)?,
        archived_at: row.get(19)?,
        deleted_at: row.get(20)?,
    })
}

pub(crate) fn map_run(row: &Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        seq: row.get(2)?,
        kind: row.get(3)?,
        state: row.get(4)?,
        trigger_json: row.get(5)?,
        error: row.get(6)?,
        started_at: row.get(7)?,
        ended_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}

pub(crate) fn map_transcript(row: &Row<'_>) -> rusqlite::Result<TranscriptEventRow> {
    Ok(TranscriptEventRow {
        thread_id: row.get(0)?,
        seq: row.get(1)?,
        acp_method: row.get(2)?,
        payload_json: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub(crate) fn map_inbox_event(row: &Row<'_>) -> rusqlite::Result<InboxEventRow> {
    Ok(InboxEventRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        summary: row.get(5)?,
        payload_json: row.get(6)?,
        created_at: row.get(7)?,
        read_at: row.get(8)?,
        dismissed_at: row.get(9)?,
    })
}

pub(crate) fn map_secret_ref(row: &Row<'_>) -> rusqlite::Result<SecretRefRow> {
    Ok(SecretRefRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        label: row.get(2)?,
        account: row.get(3)?,
        bot_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("jabot.sqlite")).unwrap();
        (store, dir)
    }

    fn sample_runtime() -> String {
        json!({
            "command": "claude-agent-acp",
            "args": [],
            "env": { "ACP_DEBUG": "1" }
        })
        .to_string()
    }

    fn sample_thread(id: &str) -> NewThread {
        NewThread {
            id: id.into(),
            folder_id: None,
            bot_id: Some("code".into()),
            harness_id: "claude".into(),
            cwd: "/tmp/repo".into(),
            runtime_json: sample_runtime(),
            title: "Auth migration".into(),
            fold_policy: "default".into(),
        }
    }

    #[test]
    fn catalog_getters_roundtrip() {
        let (store, _dir) = open_store();
        assert!(store.path().ends_with("jabot.sqlite"));
        assert_eq!(
            store.get_harness("claude").unwrap().unwrap().command,
            "claude-agent-acp"
        );
        assert!(store.get_bot("chief").unwrap().unwrap().is_chief);
        let folder = store.insert_folder("App", "/repos/app", 0).unwrap();
        assert_eq!(
            store.get_folder(&folder.id).unwrap().unwrap().path,
            "/repos/app"
        );
        assert_eq!(store.list_folders().unwrap().len(), 1);
        store.insert_thread(&sample_thread("t-get")).unwrap();
        assert_eq!(
            store.get_thread("t-get").unwrap().unwrap().title,
            "Auth migration"
        );
    }

    #[test]
    fn bundled_sqlite_meets_wal_fix_version() {
        let parsed = parse_sqlite_version(rusqlite::version()).unwrap();
        assert!(parsed >= MIN_SQLITE, "sqlite {}", rusqlite::version());
    }

    #[test]
    fn open_uses_wal_and_seeds_catalog() {
        let (store, _dir) = open_store();
        assert_eq!(store.journal_mode().unwrap(), "wal");
        assert_eq!(store.schema_version().unwrap(), 1);
        let harnesses = store.list_harnesses().unwrap();
        assert_eq!(harnesses.len(), 3);
        assert!(harnesses.iter().all(|h| h.is_builtin));
        let bots = store.list_bots().unwrap();
        assert_eq!(bots.len(), 6);
        assert_eq!(bots[0].id, "chief");
        assert!(bots[0].is_chief);
        assert_eq!(bots[0].harness_id, "claude");
    }

    #[test]
    fn reopen_does_not_recreate_deleted_bot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jabot.sqlite");
        {
            let store = Store::open(&path).unwrap();
            store
                .conn
                .execute("DELETE FROM bots WHERE id = 'writer'", [])
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let ids: Vec<_> = store
            .list_bots()
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert!(!ids.contains(&"writer".to_string()));
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn foreign_keys_and_unique_constraints() {
        let (store, _dir) = open_store();
        let err = store
            .insert_thread(&NewThread {
                harness_id: "nope".into(),
                ..sample_thread("t1")
            })
            .unwrap_err();
        assert!(matches!(err, StoreError::Invalid(_)), "{err}");

        store
            .insert_folder("App", "/repos/app", 0)
            .unwrap();
        let dup = store.insert_folder("App 2", "/repos/app", 1).unwrap_err();
        assert!(matches!(dup, StoreError::Invalid(_)), "{dup}");

        store
            .conn
            .execute(
                "INSERT INTO bots (id, name, color, instructions, tools_json, harness_id, is_chief, sort_order, created_at, updated_at)
                 VALUES ('chief2', 'Other', 'b-teal', '', '[]', 'claude', 1, 9, 't', 't')",
                [],
            )
            .unwrap_err();
    }

    #[test]
    fn runtime_json_rejects_secret_env() {
        let (store, _dir) = open_store();
        let mut new = sample_thread("t-secret");
        new.runtime_json = json!({
            "command": "claude-agent-acp",
            "env": { "ANTHROPIC_API_KEY": "sk-ant-secret" }
        })
        .to_string();
        let err = store.insert_thread(&new).unwrap_err();
        assert!(err.to_string().contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn thread_run_transcript_inbox_overlay() {
        let (store, _dir) = open_store();
        store.insert_thread(&sample_thread("t1")).unwrap();
        store.set_thread_state("t1", "folded").unwrap();
        let sleeping = store.list_threads_by_state("folded").unwrap();
        assert_eq!(sleeping.len(), 1);
        assert!(sleeping[0].folded_at.is_some());

        let run = store.insert_run("t1", "prompt", None).unwrap();
        assert_eq!(run.seq, 1);
        store.set_run_state(&run.id, "running", None).unwrap();
        let done = store.set_run_state(&run.id, "succeeded", None).unwrap();
        assert!(done.ended_at.is_some());

        store
            .append_transcript(
                "t1",
                "session/update",
                &json!({ "sessionUpdate": "agent_message_chunk" }).to_string(),
            )
            .unwrap();
        store
            .append_transcript(
                "t1",
                "session/update",
                &json!({ "sessionUpdate": "agent_message_chunk", "content": "more" }).to_string(),
            )
            .unwrap();
        let replay = store.transcript_after("t1", 1).unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].seq, 2);

        let event = store
            .insert_inbox_event("t1", Some(&run.id), "done", "Auth migration", "PR ready", None)
            .unwrap();
        assert_eq!(event.kind, "done");
        assert!(event.read_at.is_none());
    }

    #[test]
    fn secrets_live_in_vault_not_sqlite() {
        let (store, _dir) = open_store();
        let mut secrets = Secrets::memory();
        let token = "ya29.gmail-refresh-token";
        let row = store
            .put_secret(&mut secrets, "gmail", "Gmail", token, Some("inboxm"))
            .unwrap();
        assert_eq!(row.account, secret_account(&row.id));
        assert_eq!(store.get_secret(&secrets, &row.id).unwrap(), token);

        let dump: String = store
            .conn
            .query_row("SELECT quote(id) || quote(kind) || quote(label) || quote(account) || COALESCE(quote(bot_id),'') FROM secret_refs WHERE id = ?1", [&row.id], |r| r.get(0))
            .unwrap();
        assert!(!dump.contains(token), "sqlite leaked secret: {dump}");

        let all_sql: String = {
            let mut stmt = store
                .conn
                .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(!all_sql.to_lowercase().contains("ciphertext"));

        store.delete_secret(&mut secrets, &row.id).unwrap();
        assert!(store.get_secret(&secrets, &row.id).is_err());
        assert!(store.list_secret_refs().unwrap().is_empty());
    }

    #[test]
    fn unavailable_secrets_fail_closed() {
        let (store, _dir) = open_store();
        let mut secrets = Secrets::Unavailable;
        let err = store
            .put_secret(&mut secrets, "gmail", "Gmail", "tok", None)
            .unwrap_err();
        assert!(matches!(err, StoreError::SecretsUnavailable));
        assert!(store.list_secret_refs().unwrap().is_empty());
    }

    #[test]
    fn checkpoint_clears_unclean_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jabot.sqlite");
        let store = Store::open(&path).unwrap();
        assert!(unclean_marker(&path).exists());
        store.checkpoint().unwrap();
        assert!(!unclean_marker(&path).exists());
    }
}
