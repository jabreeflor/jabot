//! Secrets vault: OS credential-store bytes, SQLite `secret_refs` pointers only.
//!
//! Production backends:
//! - **macOS:** Keychain generic password, service [`KEYCHAIN_SERVICE`]
//!   (`com.jabot.app`). Visible in Keychain Access under that service.
//! - **Windows:** Credential Manager generic credential via `keyring`
//!   `windows-native`. Target name is `{account}.{service}` (for example
//!   `jabot.secret.<id>.com.jabot.app`). Control Panel → Credential Manager
//!   → Windows Credentials.
//! - **Linux / other:** no OS store yet. [`Secrets::put`] fails closed
//!   ([`StoreError::SecretsUnavailable`]) unless `JABOT_SECRETS_BACKEND=memory`.
//!
//! Same `put` / `get` / `delete` host APIs on every target. Tests use the
//! in-memory backend. Never log secret bytes; never write them into SQLite.
//!
//! This is also where the *pointers* for tool credentials live (#18). An OAuth
//! grant is two halves: the tokens, which are vault bytes like any other
//! secret, and `tool_connections`, which records only what a chip needs to
//! draw itself. Keeping both halves in one module is deliberate — the
//! invariant that one never leaks into the other is easier to hold when the
//! writes are next to each other.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{SecretRefRow, ToolConnectionRow};
use super::{map_secret_ref, map_tool_connection, now_utc, secret_account};

/// Opt into a process-local vault where the OS store is absent.
///
/// Set to `memory`, this makes [`Secrets::platform`] return the in-RAM vault
/// instead of failing closed. It is not a persistence path and cannot become
/// one: the bytes live in this process and die with it. It exists so the OAuth
/// flow can be exercised on Linux CI, where there is no OS credential store
/// and every `put` would otherwise fail — never as a way to keep tokens on disk.
const BACKEND_ENV: &str = "JABOT_SECRETS_BACKEND";

pub const KEYCHAIN_SERVICE: &str = "com.jabot.app";

/// Override the Keychain service name. Used by packaged-app acceptance (#235)
/// so a test run never reads or writes the user's production items.
///
/// Empty / unset keeps [`KEYCHAIN_SERVICE`]. Acceptance requires a name under
/// `com.jabot.app.acceptance.` — see `crate::acceptance`.
const SERVICE_ENV: &str = "JABOT_KEYCHAIN_SERVICE";

/// The OS credential-store service this process will read and write.
///
/// Production is [`KEYCHAIN_SERVICE`]. A non-empty `JABOT_KEYCHAIN_SERVICE`
/// wins so an isolated acceptance run can use a throwaway service (and a
/// throwaway keychain / credential target) instead of the user's
/// `com.jabot.app` items.
pub fn keychain_service() -> String {
    match std::env::var(SERVICE_ENV) {
        Ok(value) if !value.is_empty() => value,
        _ => KEYCHAIN_SERVICE.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsBackend {
    Keychain,
    CredentialManager,
    Memory,
    Unavailable,
}

impl SecretsBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::CredentialManager => "credential-manager",
            Self::Memory => "memory",
            Self::Unavailable => "unavailable",
        }
    }

    /// The compiled-in OS store, or [`Self::Unavailable`] where none is wired.
    pub fn os_native() -> Self {
        if cfg!(target_os = "macos") {
            Self::Keychain
        } else if cfg!(target_os = "windows") {
            Self::CredentialManager
        } else {
            Self::Unavailable
        }
    }
}

/// True when this compile target persists secrets in a real OS store.
pub(crate) fn os_store_supported() -> bool {
    cfg!(target_os = "macos") || cfg!(target_os = "windows")
}

/// In-process secret bytes. Production uses the OS store; this is for tests
/// and for hosts where the OS store is missing (put still fails closed there).
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
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    Os,
    Unavailable,
}

impl Secrets {
    pub fn memory() -> Self {
        Self::Memory(MemoryVault::default())
    }

    pub fn platform() -> Self {
        if std::env::var(BACKEND_ENV).is_ok_and(|value| value == "memory") {
            return Self::memory();
        }
        if os_store_supported() {
            Self::Os
        } else {
            Self::Unavailable
        }
    }

    pub fn backend(&self) -> SecretsBackend {
        match self {
            Self::Memory(_) => SecretsBackend::Memory,
            Self::Os => SecretsBackend::os_native(),
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

/// Classified OS-store failure. Compiled on macOS/Windows for the live
/// `keyring` path, and on every target under `cfg(test)` so Linux CI can
/// assert the mapping without compiling `keyring`.
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OsSecretFailure {
    NotFound,
    Denied(String),
    Other(String),
}

#[cfg(any(test, target_os = "macos", target_os = "windows"))]
pub(crate) fn store_error_from_os(account: &str, failure: OsSecretFailure) -> StoreError {
    match failure {
        OsSecretFailure::NotFound => StoreError::SecretNotFound(account.into()),
        OsSecretFailure::Denied(detail) => {
            eprintln!("secrets: credential store denied access ({account}): {detail}");
            StoreError::SecretsDenied(detail)
        }
        OsSecretFailure::Other(detail) => {
            StoreError::invalid(format!("credential store: {detail}"))
        }
    }
}

#[cfg(any(test, target_os = "macos", target_os = "windows"))]
pub(crate) fn looks_like_access_denied(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("access denied")
        || lower.contains("access is denied")
        || lower.contains("permission denied")
        || lower.contains("not authorized")
        || lower.contains("errsecauthfailed")
        || lower.contains("error_access_denied")
        || lower.contains("error_no_such_logon_session")
        || lower.contains("0x80070005")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn classify_keyring_error(err: keyring::Error) -> OsSecretFailure {
    match err {
        keyring::Error::NoEntry => OsSecretFailure::NotFound,
        keyring::Error::NoStorageAccess(inner) => OsSecretFailure::Denied(inner.to_string()),
        other => {
            let text = other.to_string();
            if looks_like_access_denied(&text) {
                OsSecretFailure::Denied(text)
            } else {
                OsSecretFailure::Other(text)
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn os_entry(account: &str) -> Result<keyring::Entry, StoreError> {
    keyring::Entry::new(&keychain_service(), account)
        .map_err(|err| store_error_from_os(account, classify_keyring_error(err)))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn os_put(account: &str, secret: &str) -> Result<(), StoreError> {
    let entry = os_entry(account)?;
    entry
        .set_password(secret)
        .map_err(|err| store_error_from_os(account, classify_keyring_error(err)))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn os_get(account: &str) -> Result<String, StoreError> {
    let entry = os_entry(account)?;
    entry
        .get_password()
        .map_err(|err| store_error_from_os(account, classify_keyring_error(err)))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn os_delete(account: &str) -> Result<(), StoreError> {
    let entry = os_entry(account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(store_error_from_os(account, classify_keyring_error(err))),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn os_put(_account: &str, _secret: &str) -> Result<(), StoreError> {
    Err(StoreError::SecretsUnavailable)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn os_get(_account: &str) -> Result<String, StoreError> {
    Err(StoreError::SecretsUnavailable)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// Write (or replace) the non-secret half of a provider grant.
///
/// `secret_ref_id` is a pointer, never bytes: the caller has already put the
/// token bundle in the vault. Upsert rather than insert because a re-consent
/// is the same grant with new tokens, and a provider must not accumulate rows.
#[allow(clippy::too_many_arguments)]
pub fn upsert_tool_connection(
    conn: &Connection,
    provider: &str,
    status: &str,
    account: Option<&str>,
    scopes_json: &str,
    secret_ref_id: Option<&str>,
    client_id: Option<&str>,
    expires_at: Option<&str>,
    last_error: Option<&str>,
) -> Result<ToolConnectionRow, StoreError> {
    if provider.trim().is_empty() {
        return Err(StoreError::invalid("tool connection provider is required"));
    }
    let now = now_utc();
    conn.execute(
        "INSERT INTO tool_connections (
            provider, status, account, scopes_json, secret_ref_id, client_id,
            expires_at, last_error, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(provider) DO UPDATE SET
            status = excluded.status,
            account = excluded.account,
            scopes_json = excluded.scopes_json,
            secret_ref_id = excluded.secret_ref_id,
            client_id = excluded.client_id,
            expires_at = excluded.expires_at,
            last_error = excluded.last_error,
            updated_at = excluded.updated_at",
        params![
            provider,
            status,
            account,
            scopes_json,
            secret_ref_id,
            client_id,
            expires_at,
            last_error,
            now
        ],
    )?;
    get_tool_connection(conn, provider)?.ok_or_else(|| StoreError::NotFound(provider.into()))
}

pub fn get_tool_connection(
    conn: &Connection,
    provider: &str,
) -> Result<Option<ToolConnectionRow>, StoreError> {
    conn.query_row(
        "SELECT provider, status, account, scopes_json, secret_ref_id, client_id,
                expires_at, last_error, created_at, updated_at
         FROM tool_connections WHERE provider = ?1",
        [provider],
        map_tool_connection,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_tool_connections(conn: &Connection) -> Result<Vec<ToolConnectionRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT provider, status, account, scopes_json, secret_ref_id, client_id,
                expires_at, last_error, created_at, updated_at
         FROM tool_connections ORDER BY provider",
    )?;
    let rows = stmt
        .query_map([], map_tool_connection)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_tool_connection(
    conn: &Connection,
    provider: &str,
) -> Result<Option<ToolConnectionRow>, StoreError> {
    let row = get_tool_connection(conn, provider)?;
    if row.is_some() {
        conn.execute(
            "DELETE FROM tool_connections WHERE provider = ?1",
            [provider],
        )?;
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_service_defaults_to_the_bundle_id() {
        // The function reads the process environment. Tests share a process, so
        // this only asserts the documented default when the override is unset.
        // The override itself is what packaged acceptance sets; a missing
        // default here would write production items under a blank service.
        if std::env::var(SERVICE_ENV).is_err() {
            assert_eq!(keychain_service(), KEYCHAIN_SERVICE);
        }
    }

    #[test]
    fn keychain_service_override_is_not_the_production_name() {
        // Guard the constant, not the env: if someone "simplifies" the
        // production service into something acceptance could collide with,
        // isolation stops being isolation.
        assert_eq!(KEYCHAIN_SERVICE, "com.jabot.app");
        assert!(
            !KEYCHAIN_SERVICE.contains("acceptance"),
            "production Keychain service must stay distinct from the acceptance prefix"
        );
    }

    #[test]
    fn os_native_backend_name_matches_the_compile_target() {
        let backend = SecretsBackend::os_native();
        if cfg!(target_os = "macos") {
            assert_eq!(backend.as_str(), "keychain");
        } else if cfg!(target_os = "windows") {
            assert_eq!(backend.as_str(), "credential-manager");
        } else {
            assert_eq!(backend.as_str(), "unavailable");
        }
        assert_eq!(os_store_supported(), backend != SecretsBackend::Unavailable);
    }

    #[test]
    fn platform_selects_the_os_store_where_one_is_wired() {
        if std::env::var(BACKEND_ENV).is_ok() {
            return;
        }
        let secrets = Secrets::platform();
        if os_store_supported() {
            assert_eq!(secrets.backend(), SecretsBackend::os_native());
        } else {
            assert_eq!(secrets.backend(), SecretsBackend::Unavailable);
        }
    }

    #[test]
    fn linux_os_helpers_fail_closed_when_no_store_is_wired() {
        if os_store_supported() {
            return;
        }
        let err = os_put("jabot.secret.test", "tok").unwrap_err();
        assert!(matches!(err, StoreError::SecretsUnavailable), "{err}");
        assert!(matches!(
            os_get("jabot.secret.test"),
            Err(StoreError::SecretsUnavailable)
        ));
        os_delete("jabot.secret.test").expect("delete is a no-op without a store");
    }

    #[test]
    fn denied_access_is_explicit_and_never_embeds_secret_bytes() {
        let secret = "sk-ant-not-a-real-token";
        let err = store_error_from_os(
            "jabot.secret.test",
            OsSecretFailure::Denied("Windows ERROR_ACCESS_DENIED".into()),
        );
        match err {
            StoreError::SecretsDenied(detail) => {
                assert!(detail.contains("ACCESS_DENIED"), "{detail}");
                assert!(!detail.contains(secret), "{detail}");
            }
            other => panic!("expected SecretsDenied, got {other}"),
        }
        let rendered = StoreError::SecretsDenied("user cancelled the prompt".into()).to_string();
        assert!(
            rendered.contains("denied access"),
            "RPC/UI must see an explicit denial, not a generic invalid: {rendered}"
        );
        assert!(!rendered.contains(secret), "{rendered}");
    }

    #[test]
    fn access_denied_phrases_classify_as_denied() {
        for phrase in [
            "Access is denied.",
            "permission denied",
            "errSecAuthFailed",
            "ERROR_NO_SUCH_LOGON_SESSION",
            "0x80070005",
        ] {
            assert!(
                looks_like_access_denied(phrase),
                "{phrase} should classify as denied"
            );
        }
        assert!(!looks_like_access_denied("no such entry"));
        assert!(!looks_like_access_denied("credential too long"));
    }

    #[test]
    fn os_not_found_and_other_failures_stay_distinct_from_denied() {
        assert!(matches!(
            store_error_from_os("acct", OsSecretFailure::NotFound),
            StoreError::SecretNotFound(_)
        ));
        let other = store_error_from_os("acct", OsSecretFailure::Other("target too long".into()));
        assert!(matches!(other, StoreError::Invalid(_)), "{other}");
        assert!(!other.to_string().contains("denied access"), "{other}");
    }

    /// Live Keychain / Credential Manager put → get → delete. Linux compiles
    /// this out; `scripts/windows-secrets-check.sh` is the named entry on a
    /// Windows runner (#283 / #286).
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn os_secret_round_trip_put_get_delete() {
        let account = format!(
            "jabot.secret.test.os-round-trip.{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let secret = "jabot-test-not-a-user-credential";
        let mut vault = Secrets::Os;
        vault
            .put(&account, secret)
            .expect("OS credential store must accept a test put");
        let got = vault.get(&account).expect("OS credential store must load");
        let deleted = vault.delete(&account);
        assert_eq!(got, secret, "round-trip must not change bytes");
        deleted.expect("OS credential store must delete the test item");
        assert!(
            matches!(vault.get(&account), Err(StoreError::SecretNotFound(_))),
            "delete must remove the item"
        );
    }
}
