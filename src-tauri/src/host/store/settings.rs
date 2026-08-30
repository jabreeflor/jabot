//! App-wide preferences: the home three records parked a knob on.
//!
//! D-006 left the stuck backstop's threshold as `JABOT_IDLE_TIMEOUT_MS`, an env
//! var on the host process — which a bundled Tauri app gives nobody. D-018
//! recorded the same for the fold default and said plainly that naming #26 for
//! it had been optimistic, because nothing in that issue's scope created a
//! place to put it. This is the place.
//!
//! `app_meta` rather than a new table. It already exists, it is already the
//! "one row per app-wide knob" shape, and it already has a seeded row —
//! `purge_deleted_after_days`, written since 0001 and read by nothing. A
//! migration for a second key/value table beside a key/value table would be a
//! schema change buying a name.
//!
//! **Validated on read, not only on write.** A stored value can come from an
//! older build, a hand-edited row, or a write this code has not learned to
//! refuse yet. Falling back to the shipped default is how a nonsense row costs
//! a preference instead of a launch.

use rusqlite::{params, Connection, OptionalExtension};

use super::error::StoreError;

/// The stuck backstop's silence threshold, in milliseconds.
pub const KEY_IDLE_TIMEOUT_MS: &str = "idle_timeout_ms";
/// What a thread's fold policy starts as: `default` or `wait_for_inbox`.
pub const KEY_DEFAULT_FOLD_POLICY: &str = "default_fold_policy";

/// The floor, and what an empty store answers. Matches the column default in
/// `0001_init.sql` so the two cannot disagree about a fresh install.
pub const DEFAULT_FOLD_POLICY: &str = "default";

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    conn.query_row("SELECT value FROM app_meta WHERE key = ?1", [key], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(Into::into)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// The stored idle timeout, or `None` for "not set" — which is not the same as
/// zero, and is what lets the caller keep the shipped default.
///
/// A value that will not parse, or a zero, is treated as absent. Zero would
/// mean a backstop that fires on every tick, which nobody can have meant.
pub fn idle_timeout_ms(conn: &Connection) -> Result<Option<u64>, StoreError> {
    Ok(get_setting(conn, KEY_IDLE_TIMEOUT_MS)?
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0))
}

/// The stored fold default, falling back to `default` for anything the fold
/// path would refuse. Same two arms as `overlay::set_thread_fold_policy`, so a
/// value that reached the row through some other door cannot make a thread
/// that the fold code will not accept.
pub fn default_fold_policy(conn: &Connection) -> Result<String, StoreError> {
    Ok(get_setting(conn, KEY_DEFAULT_FOLD_POLICY)?
        .filter(|policy| is_fold_policy(policy))
        .unwrap_or_else(|| DEFAULT_FOLD_POLICY.to_string()))
}

/// The vocabulary, in one place. `overlay::set_thread_fold_policy` refuses
/// anything else, and a default that it would refuse is a default that breaks
/// thread creation rather than one that is merely wrong.
pub fn is_fold_policy(policy: &str) -> bool {
    matches!(policy, "default" | "wait_for_inbox")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::Store;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("jabot.sqlite")).unwrap();
        (store, dir)
    }

    #[test]
    fn a_fresh_store_answers_the_shipped_defaults() {
        let (store, _dir) = store();
        assert_eq!(store.idle_timeout_ms().unwrap(), None);
        assert_eq!(store.default_fold_policy().unwrap(), "default");
    }

    #[test]
    fn a_setting_survives_being_written_twice() {
        let (store, _dir) = store();
        store.set_setting(KEY_IDLE_TIMEOUT_MS, "90000").unwrap();
        assert_eq!(store.idle_timeout_ms().unwrap(), Some(90_000));
        // The second write is an update, not a second row: `app_meta`'s key is
        // its primary key and an INSERT alone would fail.
        store.set_setting(KEY_IDLE_TIMEOUT_MS, "120000").unwrap();
        assert_eq!(store.idle_timeout_ms().unwrap(), Some(120_000));
    }

    /// The reason validation is on the read. A row can come from an older
    /// build or a hand edit, and a nonsense value should cost the preference
    /// rather than the launch.
    #[test]
    fn a_value_that_makes_no_sense_falls_back() {
        let (store, _dir) = store();
        for raw in ["", "soon", "-1", "0", "12.5"] {
            store.set_setting(KEY_IDLE_TIMEOUT_MS, raw).unwrap();
            assert_eq!(store.idle_timeout_ms().unwrap(), None, "{raw}");
        }
        for raw in ["", "whenever", "Default", "wait for inbox"] {
            store.set_setting(KEY_DEFAULT_FOLD_POLICY, raw).unwrap();
            assert_eq!(store.default_fold_policy().unwrap(), "default", "{raw}");
        }
    }

    #[test]
    fn the_fold_vocabulary_is_the_one_the_fold_path_accepts() {
        let (store, _dir) = store();
        store
            .set_setting(KEY_DEFAULT_FOLD_POLICY, "wait_for_inbox")
            .unwrap();
        assert_eq!(store.default_fold_policy().unwrap(), "wait_for_inbox");
        assert!(is_fold_policy("default"));
        assert!(is_fold_policy("wait_for_inbox"));
        assert!(!is_fold_policy("wait_for_nothing"));
    }

    /// `purge_deleted_after_days` has been seeded since 0001 and read by
    /// nothing. Reusing `app_meta` means an unrelated key is simply another
    /// row, not something a settings read trips over.
    #[test]
    fn the_seeded_row_is_left_alone() {
        let (store, _dir) = store();
        store.set_setting(KEY_IDLE_TIMEOUT_MS, "60000").unwrap();
        assert_eq!(
            store.get_setting("purge_deleted_after_days").unwrap(),
            Some("30".to_string())
        );
        assert_eq!(store.get_setting("no_such_key").unwrap(), None);
    }
}
