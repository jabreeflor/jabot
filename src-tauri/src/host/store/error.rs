//! Errors from the host-owned SQLite store and secrets vault.

use rusqlite::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("migration {version}: {message}")]
    Migration { version: i32, message: String },
    #[error("integrity check failed: {0}")]
    Integrity(String),
    #[error("bundled sqlite {found} is older than required 3.51.3")]
    SqliteTooOld { found: String },
    #[error("secrets backend unavailable")]
    SecretsUnavailable,
    #[error("secret not found: {0}")]
    SecretNotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        match err {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound("row".into()),
            rusqlite::Error::SqliteFailure(info, message)
                if info.code == ErrorCode::ConstraintViolation =>
            {
                Self::Invalid(message.unwrap_or_else(|| format!("{:?}", info.code)))
            }
            other => Self::Sqlite(other),
        }
    }
}

impl StoreError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}
