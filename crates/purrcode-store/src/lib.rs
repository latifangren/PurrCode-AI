// SPDX-License-Identifier: GPL-3.0-or-later

//! SQLite store for PurrCode machine state (ADR-015).
//!
//! One embedded database file per `purrcode_dir` (`<dir>/purrcode.db`),
//! holding sessions, turns, tool calls, cron jobs, deliveries, audit
//! events, and Telegram chats. Memory stays Markdown (ADR-010);
//! credentials never enter this database.
//!
//! Concurrency model (spec §4): a single `rusqlite::Connection` per
//! handle behind a `Mutex`; all blocking calls go through
//! `tokio::task::spawn_blocking` at the call site. WAL + `busy_timeout`
//! cover the gateway / WebUI / TUI / CLI multi-process writers.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub mod error;
mod migrations;

pub use error::StoreError;
pub use rusqlite;

/// Database file name inside a `purrcode_dir`.
pub const DB_FILE_NAME: &str = "purrcode.db";

/// Convert a `rusqlite::Result` into a `Result<_, StoreError>` so
/// query sites can use `?` into their own error type (every crate
/// maps `StoreError` onward). The orphan rule forbids a blanket
/// `From<rusqlite::Error> for PurrError`, so direct query calls go
/// through this helper instead of bare `?`.
pub fn sql<T>(result: rusqlite::Result<T>) -> Result<T, StoreError> {
    result.map_err(StoreError::from)
}

/// The store handle. Owns one SQLite connection; see the module docs
/// for the concurrency model.
pub struct Store {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Store {
    /// Open (or create) the database at `<dir>/purrcode.db`, creating
    /// the directory if needed. Applies the §2.3 PRAGMAs and any pending
    /// migrations. A newly created file is `0600` (Unix). Failing to
    /// open or migrate is a hard error — never a silent fallback to
    /// flat files.
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        Self::open_file(&dir.join(DB_FILE_NAME))
    }

    /// Open (or create) the database at an explicit file path.
    /// Same guarantees as [`Store::open`], without the directory join.
    pub fn open_file(path: &Path) -> Result<Self, StoreError> {
        let existed = path.exists();
        let conn = Connection::open(path)?;
        Self::finish(conn, path.to_path_buf(), existed)
    }

    /// Open an in-memory database (tests). Same schema and migrations
    /// as a file database; WAL is a no-op here.
    pub fn open_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::finish(conn, PathBuf::from(":memory:"), true)
    }

    fn finish(mut conn: Connection, path: PathBuf, existed: bool) -> Result<Self, StoreError> {
        apply_pragmas(&conn)?;
        migrations::migrate(&mut conn)?;
        if !existed && path != Path::new(":memory:") {
            restrict_permissions(&path)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Path of the database file (`:memory:` for [`Store::open_memory`]).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lock the single connection. Every query goes through this guard;
    /// hold it across multi-statement transactions. A poisoned mutex
    /// (a previous holder panicked) is an error, never a silent proceed.
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.conn.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

/// ADR-015 §2.3: WAL for concurrent readers + one writer, `NORMAL`
/// synchronous (durable enough with WAL), foreign keys on, 5 s busy
/// timeout so multi-process writers (gateway/Web/TUI/CLI) wait instead
/// of failing at once.
fn apply_pragmas(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

/// New database files hold session transcripts: `0600`, like
/// `~/.purrcode/credentials`. Unix-only; other platforms keep the
/// default permissions.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("purr_store_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn table_names(store: &Store) -> Vec<String> {
        let guard = store.lock().unwrap();
        let mut stmt = guard
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn test_open_memory_creates_all_tables_and_version() {
        let store = Store::open_memory().unwrap();
        let tables = table_names(&store);
        for want in [
            "audit",
            "chats",
            "cron_jobs",
            "deliveries",
            "schema_meta",
            "sessions",
            "tool_calls",
            "turns",
        ] {
            assert!(
                tables.contains(&want.to_string()),
                "missing {want}: {tables:?}"
            );
        }
        let guard = store.lock().unwrap();
        let version: i64 = guard
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, migrations::CURRENT_VERSION);
        let recorded: String = guard
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recorded, migrations::CURRENT_VERSION.to_string());
    }

    #[test]
    fn test_open_file_persists_across_reopen() {
        let dir = temp_dir();
        let path = dir.join(DB_FILE_NAME);
        {
            let store = Store::open_file(&path).unwrap();
            let guard = store.lock().unwrap();
            guard
                .execute(
                    "INSERT INTO sessions (id, title, created_at, updated_at)
                     VALUES ('purr_web', 'hello', '2026-11-16T00:00:00+07:00', '2026-11-16T00:00:00+07:00')",
                    [],
                )
                .unwrap();
        }
        let store = Store::open_file(&path).unwrap();
        let guard = store.lock().unwrap();
        let title: String = guard
            .query_row(
                "SELECT title FROM sessions WHERE id = 'purr_web'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "hello");
        let version: i64 = guard
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, migrations::CURRENT_VERSION);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_open_file_sets_0600_and_pragmas() {
        let dir = temp_dir();
        let path = dir.join(DB_FILE_NAME);
        let store = Store::open_file(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "new db file must be 0600, got {mode:o}");
        }
        let guard = store.lock().unwrap();
        let journal: String = guard
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        let synchronous: i64 = guard
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1, "NORMAL");
        let fk: i64 = guard
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        let busy: i64 = guard
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 5000);
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_foreign_keys_and_unique_seq_enforced() {
        let store = Store::open_memory().unwrap();
        let guard = store.lock().unwrap();
        // Turn for a nonexistent session must fail (FK on).
        let err = guard
            .execute(
                "INSERT INTO turns (session_id, seq, role, content, created_at)
                 VALUES ('purr_nope', 1, 'user', 'hi', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(..)));
        guard
            .execute(
                "INSERT INTO sessions (id, title, created_at, updated_at)
                 VALUES ('purr_web', '', '2026-11-16T00:00:00+07:00', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap();
        guard
            .execute(
                "INSERT INTO turns (session_id, seq, role, content, created_at)
                 VALUES ('purr_web', 1, 'user', 'hi', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap();
        // Same (session_id, seq) twice must fail (UNIQUE).
        let err = guard
            .execute(
                "INSERT INTO turns (session_id, seq, role, content, created_at)
                 VALUES ('purr_web', 1, 'user', 'again', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(..)));
    }

    #[test]
    fn test_session_turn_tool_call_round_trip() {
        let store = Store::open_memory().unwrap();
        let guard = store.lock().unwrap();
        guard
            .execute(
                "INSERT INTO sessions (id, title, created_at, updated_at)
                 VALUES ('purr_web', 'hello', '2026-11-16T00:00:00+07:00', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap();
        guard
            .execute(
                "INSERT INTO turns (session_id, seq, role, content, tool_calls_json, created_at)
                 VALUES ('purr_web', 1, 'assistant', '', '[{\"id\":\"call-1\",\"name\":\"read\"}]', '2026-11-16T00:00:00+07:00')",
                [],
            )
            .unwrap();
        let turn_id: i64 = guard
            .query_row(
                "SELECT id FROM turns WHERE session_id = 'purr_web' AND seq = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        guard
            .execute(
                "INSERT INTO tool_calls (id, turn_id, session_id, name, args_json, status, created_at)
                 VALUES ('call-1', ?, 'purr_web', 'read', '{\"path\":\"x\"}', 'done', '2026-11-16T00:00:00+07:00')",
                [turn_id],
            )
            .unwrap();
        let (role, name): (String, String) = guard
            .query_row(
                "SELECT t.role, c.name FROM turns t
                 JOIN tool_calls c ON c.turn_id = t.id
                 WHERE t.session_id = 'purr_web'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(role, "assistant");
        assert_eq!(name, "read");
    }
}
