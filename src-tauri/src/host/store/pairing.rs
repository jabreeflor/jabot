//! Paired devices: the durable outcome of the handshake in `host/pairing/`.
//!
//! Everything here is written so that "revoked" and "may do this much" are
//! answers SQLite gives, not answers a client supplies or a cache remembers.
//!
//! [`upsert_paired_device`] is an upsert on purpose: a device that was revoked
//! — or one that lost its key material and re-scanned a QR — pairs again by
//! running the whole handshake, and the row it already has is the row that
//! should be re-issued rather than a second one under a new id. The upsert
//! clears `revoked_at` and resets `auth_counter`, because the new grant is
//! backed by a new token and nothing about the old one should carry over.
//!
//! [`bump_device_auth_counter`] is a guarded UPDATE for the same reason
//! `resolve_permission_request` is: the check and the write have to be one
//! statement, or two connections replaying the same proof could both pass the
//! read before either did the write.

use rusqlite::{params, Connection, OptionalExtension};

use super::error::StoreError;
use super::models::{NewPairedDevice, PairedDeviceRow};
use super::{map_paired_device, now_utc};

const COLUMNS: &str = "device_id, name, role, fingerprint, token_ref, auth_counter, \
     paired_via, sas, created_at, last_seen_at, revoked_at";

pub fn upsert_paired_device(
    conn: &Connection,
    new: &NewPairedDevice,
) -> Result<PairedDeviceRow, StoreError> {
    conn.execute(
        "INSERT INTO paired_devices (
            device_id, name, role, fingerprint, token_ref, auth_counter,
            paired_via, sas, created_at, last_seen_at, revoked_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, NULL, NULL)
         ON CONFLICT(device_id) DO UPDATE SET
            name = excluded.name,
            role = excluded.role,
            fingerprint = excluded.fingerprint,
            token_ref = excluded.token_ref,
            auth_counter = 0,
            paired_via = excluded.paired_via,
            sas = excluded.sas,
            last_seen_at = NULL,
            revoked_at = NULL",
        params![
            new.device_id,
            new.name,
            new.role,
            new.fingerprint,
            new.token_ref,
            new.paired_via,
            new.sas,
            now_utc(),
        ],
    )?;
    get_paired_device(conn, &new.device_id)?
        .ok_or_else(|| StoreError::NotFound(new.device_id.clone()))
}

/// The row, revoked or not. Callers that care ask the row.
pub fn get_paired_device(
    conn: &Connection,
    device_id: &str,
) -> Result<Option<PairedDeviceRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM paired_devices WHERE device_id = ?1"),
        [device_id],
        map_paired_device,
    )
    .optional()
    .map_err(Into::into)
}

/// Every device this host has ever admitted, newest first, tombstones included
/// — the revoke list is a list, and a device you cut off yesterday is part of
/// what the screen should be able to show you.
pub fn list_paired_devices(conn: &Connection) -> Result<Vec<PairedDeviceRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM paired_devices ORDER BY created_at DESC, device_id"
    ))?;
    let rows = stmt
        .query_map([], map_paired_device)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Cut a device off. `Ok(false)` means it was already revoked, or was never
/// paired — both of which leave the host in the state the caller wanted.
pub fn revoke_paired_device(conn: &Connection, device_id: &str) -> Result<bool, StoreError> {
    let changed = conn.execute(
        "UPDATE paired_devices SET revoked_at = ?2
          WHERE device_id = ?1 AND revoked_at IS NULL",
        params![device_id, now_utc()],
    )?;
    Ok(changed > 0)
}

/// Accept an authentication counter strictly greater than the last one.
///
/// `Ok(false)` is a replay — or a device whose grant was revoked between the
/// proof being checked and this write — and the caller must refuse the
/// connection on it.
pub fn bump_device_auth_counter(
    conn: &Connection,
    device_id: &str,
    counter: i64,
) -> Result<bool, StoreError> {
    let changed = conn.execute(
        "UPDATE paired_devices SET auth_counter = ?2, last_seen_at = ?3
          WHERE device_id = ?1 AND revoked_at IS NULL AND auth_counter < ?2",
        params![device_id, counter, now_utc()],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path().join("jabot.sqlite")).expect("store");
        (dir, store)
    }

    fn device(id: &str, role: &str) -> NewPairedDevice {
        NewPairedDevice {
            device_id: id.into(),
            name: "Jabree's iPhone".into(),
            role: role.into(),
            fingerprint: "fp-device".into(),
            token_ref: format!("jabot.secret.device-token.{id}"),
            paired_via: "qr".into(),
            sas: "1234-5678".into(),
        }
    }

    #[test]
    fn a_pairing_outlives_the_process_that_made_it() {
        let (dir, store) = store();
        store
            .upsert_paired_device(&device("d-1", "approver"))
            .expect("pair");
        drop(store);

        let reopened = Store::open(dir.path().join("jabot.sqlite")).expect("reopen");
        let row = reopened
            .get_paired_device("d-1")
            .expect("read")
            .expect("row");
        assert_eq!(row.role, "approver");
        assert!(row.revoked_at.is_none());
        assert_eq!(row.auth_counter, 0);
    }

    #[test]
    fn revocation_is_durable_and_happens_once() {
        let (dir, store) = store();
        store
            .upsert_paired_device(&device("d-1", "approver"))
            .expect("pair");
        assert!(store.revoke_paired_device("d-1").expect("revoke"));
        // A second revoke is not an error and does not move the timestamp.
        assert!(!store.revoke_paired_device("d-1").expect("revoke again"));
        let stamped = store.get_paired_device("d-1").expect("read").expect("row");
        drop(store);

        let reopened = Store::open(dir.path().join("jabot.sqlite")).expect("reopen");
        let row = reopened
            .get_paired_device("d-1")
            .expect("read")
            .expect("row");
        assert_eq!(row.revoked_at, stamped.revoked_at);
        assert!(
            row.revoked_at.is_some(),
            "a restart must not forget a revoke"
        );
    }

    #[test]
    fn a_revoked_device_cannot_advance_its_counter() {
        let (_dir, store) = store();
        store
            .upsert_paired_device(&device("d-1", "full"))
            .expect("pair");
        assert!(store.bump_device_auth_counter("d-1", 1).expect("first"));
        // Replay of the same proof, and of an older one.
        assert!(!store.bump_device_auth_counter("d-1", 1).expect("replay"));
        assert!(!store.bump_device_auth_counter("d-1", 0).expect("rollback"));
        assert!(store.bump_device_auth_counter("d-1", 2).expect("next"));

        store.revoke_paired_device("d-1").expect("revoke");
        assert!(
            !store
                .bump_device_auth_counter("d-1", 3)
                .expect("after revoke"),
            "a revoked device must not be able to authenticate"
        );
    }

    #[test]
    fn re_pairing_reissues_the_row_and_clears_the_tombstone() {
        let (_dir, store) = store();
        store
            .upsert_paired_device(&device("d-1", "approver"))
            .expect("pair");
        store.bump_device_auth_counter("d-1", 7).expect("use");
        store.revoke_paired_device("d-1").expect("revoke");

        let again = store
            .upsert_paired_device(&NewPairedDevice {
                role: "full".into(),
                token_ref: "jabot.secret.device-token.rotated".into(),
                ..device("d-1", "full")
            })
            .expect("re-pair");
        assert!(again.revoked_at.is_none());
        assert_eq!(again.role, "full");
        // The old token's counter must not carry over: the grant is new.
        assert_eq!(again.auth_counter, 0);
        assert_eq!(store.list_paired_devices().expect("list").len(), 1);
    }
}
