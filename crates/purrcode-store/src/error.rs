// SPDX-License-Identifier: GPL-3.0-or-later

//! Errors from the SQLite store.

use thiserror::Error;

/// Errors from [`crate::Store`]. A failed open or migration is a hard
/// error at startup — the store never falls back to flat files.
#[derive(Debug, Error)]
pub enum StoreError {
    /// SQLite failure (open, pragma, migration, query).
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Filesystem failure (create dir, set permissions).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization failure for a `*_json` column.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// The store mutex was poisoned by a previous holder panicking
    /// while holding the connection. The connection state is unknown,
    /// so callers must fail rather than proceed.
    #[error("store lock poisoned")]
    LockPoisoned,

    /// The migration list has no DDL for the requested schema version
    /// (a gap in `MIGRATIONS` — a bug, not a corrupt database).
    #[error("missing migration for schema version {0}")]
    MissingMigration(i64),
}

impl From<StoreError> for purrcode_api::error::PurrError {
    /// Store failures are infrastructure failures: `Internal`, with
    /// the store as the source. Domain conditions (unknown session,
    /// empty transcript) are checked by the callers and never surface
    /// through this conversion.
    fn from(e: StoreError) -> Self {
        purrcode_api::error::PurrError::Internal(format!("sqlite store: {e}"))
    }
}
