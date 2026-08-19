//! Secrets vault: OS keychain bytes, SQLite `secret_refs` pointers only.
//!
//! MVP is macOS Keychain (bundle id service). Linux/Windows put() fails
//! closed until a real OS store is wired. Tests use the in-memory backend.
//! Never log secret bytes; never write them into SQLite.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::SecretRefRow;
use super::{map_secret_ref, now_utc, secret_account};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const KEYCHAIN_SERVICE: &str = "com.jabot.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsBackend {
    Keychain,
    Memory,
    Unavailable,
}

impl SecretsBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::Memory => "memory",
            Self::Unavailable => "unavailable",
        }
    }
}

/// In-process secret bytes. Production uses the OS store; this is for tests
/// and for hosts where Keychain is missing (put still fails closed there).
#[derive(Debug, Default)]
pub struct MemoryVault {
    items: std::collections::HashMap<String, String>,
}

impl MemoryVault {
    pub fn put(&mut self, account: &str, secret: &str) {
        self.items.insert(account.to_string(), secret.to_string());
    }

    pub fn get(&self, account: &str) -> Option<String> {
        self.items.get(account).cloned()
    }

    pub fn delete(&mut self, account: &str) {
        self.items.remove(account);
    }
}

#[derive(Debug)]
pub enum Secrets {
    Memory(MemoryVault),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Os,
    Unavailable,
}

impl Secrets {
    pub fn memory() -> Self {
        Self::Memory(MemoryVault::default())
    }

    pub fn platform() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Os
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::Unavailable
        }
    }

    pub fn backend(&self) -> SecretsBackend {
        match self {
            Self::Memory(_) => SecretsBackend::Memory,
            Self::Os => SecretsBackend::Keychain,
            Self::Unavailable => SecretsBackend::Unavailable,
        }
    }

    pub fn put(&mut self, account: &str, secret: &str) -> Result<(), StoreError> {
        match self {
            Self::Memory(vault) => {
                vault.put(account, secret);
                Ok(())
            }
            Self::Os => os_put(account, secret),
            Self::Unavailable => Err(StoreError::SecretsUnavailable),
        }
    }

    pub fn get(&self, account: &str) -> Result<String, StoreError> {
        match self {
            Self::Memory(vault) => vault
                .get(account)
                .ok_or_else(|| StoreError::SecretNotFound(account.into())),
            Self::Os => os_get(account),
            Self::Unavailable => Err(StoreError::SecretsUnavailable),
        }
    }

    pub fn delete(&mut self, account: &str) -> Result<(), StoreError> {
        match self {
            Self::Memory(vault) => {
                vault.delete(account);
                Ok(())
            }
            Self::Os => os_delete(account),
            Self::Unavailable => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
fn os_put(account: &str, secret: &str) -> Result<(), StoreError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|e| StoreError::invalid(e.to_string()))?;
    entry
        .set_password(secret)
        .map_err(|e| StoreError::invalid(e.to_string()))
}

#[cfg(target_os = "macos")]
fn os_get(account: &str) -> Result<String, StoreError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|e| StoreError::invalid(e.to_string()))?;
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        Err(keyring::Error::NoEntry) => Err(StoreError::SecretNotFound(account.into())),
        Err(err) => Err(StoreError::invalid(err.to_string())),
    }
}

#[cfg(target_os = "macos")]
fn os_delete(account: &str) -> Result<(), StoreError> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .map_err(|e| StoreError::invalid(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(StoreError::invalid(err.to_string())),
    }
}

#[cfg(not(target_os = "macos"))]
fn os_put(_account: &str, _secret: &str) -> Result<(), StoreError> {
    Err(StoreError::SecretsUnavailable)
}

#[cfg(not(target_os = "macos"))]
fn os_get(_account: &str) -> Result<String, StoreError> {
    Err(StoreError::SecretsUnavailable)
}

#[cfg(not(target_os = "macos"))]
fn os_delete(_account: &str) -> Result<(), StoreError> {
    Ok(())
}

pub fn insert_secret_ref(
    conn: &Connection,
    kind: &str,
    label: &str,
    bot_id: Option<&str>,
) -> Result<SecretRefRow, StoreError> {
    if kind.trim().is_empty() || label.trim().is_empty() {
        return Err(StoreError::invalid("secret kind and label are required"));
    }
    let id = Uuid::new_v4().to_string();
    let account = secret_account(&id);
    let now = now_utc();
    conn.execute(
        "INSERT INTO secret_refs (id, kind, label, account, bot_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, kind, label, account, bot_id, now],
    )?;
    get_secret_ref(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_secret_ref(conn: &Connection, id: &str) -> Result<Option<SecretRefRow>, StoreError> {
    conn.query_row(
        "SELECT id, kind, label, account, bot_id, created_at, updated_at
         FROM secret_refs WHERE id = ?1",
        [id],
        map_secret_ref,
    )
    .optional()
    .map_err(Into::into)
}

pub fn delete_secret_ref(conn: &Connection, id: &str) -> Result<Option<SecretRefRow>, StoreError> {
    let row = get_secret_ref(conn, id)?;
    if row.is_some() {
        conn.execute("DELETE FROM secret_refs WHERE id = ?1", [id])?;
    }
    Ok(row)
}

pub fn list_secret_refs(conn: &Connection) -> Result<Vec<SecretRefRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, label, account, bot_id, created_at, updated_at
         FROM secret_refs ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([], map_secret_ref)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
