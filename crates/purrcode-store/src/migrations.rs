// SPDX-License-Identifier: GPL-3.0-or-later

//! Schema migrations. Versioned via `PRAGMA user_version` and mirrored
//! in the `schema_meta` table (`key='version'`). Each migration is one
//! DDL batch applied inside a transaction; adding a migration means
//! appending to `MIGRATIONS`, bumping `CURRENT_VERSION`, and adding a
//! test that migrates a v(n-1) database and asserts the v(n) shape.

use crate::error::StoreError;
use rusqlite::Connection;

/// Current schema version. Bump when appending to `MIGRATIONS`.
pub const CURRENT_VERSION: i64 = 1;

/// Migrations in order; index `i` produces version `i + 1`.
const MIGRATIONS: &[&str] = &[MIGRATION_V1];

/// v1: the full ADR-015 schema (`docs/db-store-design.md` §3).
/// IDs keep their existing formats (`purr_web`, `purr_cron_<id>_<run>`,
/// `purr_hb_<hex>`, `purr_<id>`, `cron_<12 hex>`); times are RFC3339
/// TEXT, as in the JSONL/JSONC files this replaces.
const MIGRATION_V1: &str = "
CREATE TABLE sessions (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL DEFAULT '',
  parent_id   TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  preset_json TEXT,
  web_preset  TEXT,
  web_model   TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX idx_sessions_updated ON sessions(updated_at DESC);

CREATE TABLE turns (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  seq               INTEGER NOT NULL,
  role              TEXT NOT NULL,
  content           TEXT NOT NULL DEFAULT '',
  tool_call_id      TEXT,
  tool_calls_json   TEXT,
  prompt_tokens     INTEGER,
  completion_tokens INTEGER,
  created_at        TEXT NOT NULL,
  UNIQUE (session_id, seq)
);
CREATE INDEX idx_turns_session_seq ON turns(session_id, seq);

CREATE TABLE tool_calls (
  id          TEXT NOT NULL,
  turn_id     INTEGER NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
  session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,
  args_json   TEXT NOT NULL DEFAULT '{}',
  status      TEXT NOT NULL DEFAULT 'done',
  created_at  TEXT NOT NULL,
  PRIMARY KEY (session_id, id)
);
CREATE INDEX idx_tool_calls_session ON tool_calls(session_id, created_at);

CREATE TABLE cron_jobs (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  enabled          INTEGER NOT NULL DEFAULT 1,
  schedule_expr    TEXT,
  schedule_display TEXT NOT NULL,
  schedule_once    INTEGER NOT NULL DEFAULT 0,
  schedule_at      TEXT,
  prompt           TEXT NOT NULL,
  tools_json       TEXT NOT NULL DEFAULT '[]',
  preset_json      TEXT NOT NULL,
  delivery_json    TEXT NOT NULL,
  created_at       TEXT NOT NULL,
  last_run_at      TEXT,
  next_run_at      TEXT,
  run_count        INTEGER NOT NULL DEFAULT 0,
  failure_count    INTEGER NOT NULL DEFAULT 0,
  last_status      TEXT
);

CREATE TABLE deliveries (
  id           TEXT PRIMARY KEY,
  job_id       TEXT NOT NULL,
  job_name     TEXT NOT NULL,
  kind         TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'pending',
  summary      TEXT NOT NULL DEFAULT '',
  detail       TEXT NOT NULL DEFAULT '',
  chat_id      INTEGER,
  created_at   TEXT NOT NULL,
  claimed_at   TEXT,
  delivered_at TEXT
);
CREATE INDEX idx_deliveries_job ON deliveries(job_id, created_at DESC);
CREATE INDEX idx_deliveries_status ON deliveries(status, created_at);

CREATE TABLE audit (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ts         TEXT NOT NULL,
  session_id TEXT NOT NULL,
  agent      TEXT NOT NULL,
  event      TEXT NOT NULL,
  tool       TEXT,
  decision   TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_audit_session_event ON audit(session_id, event, id);
CREATE INDEX idx_audit_ts ON audit(ts);

CREATE TABLE chats (
  chat_id         INTEGER PRIMARY KEY, -- Telegram chat id
  kind            TEXT,                -- private | group | supergroup | channel (NULL = unknown, not delivery-eligible)
  last_active_at  INTEGER,             -- Unix seconds of the last accepted message (NULL = never)
  title           TEXT,                -- reserved for v1 (chat title from the Bot API; not populated yet)
  current_session TEXT,                -- session new messages go to (NULL = none)
  history_json    TEXT NOT NULL DEFAULT '[]' -- archived session ids, oldest first
);

CREATE TABLE schema_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

/// Apply every pending migration, in order. Idempotent: a database
/// already at `CURRENT_VERSION` is untouched.
pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for v in (version + 1)..=CURRENT_VERSION {
        let ddl = MIGRATIONS
            .get((v - 1) as usize)
            .ok_or(StoreError::MissingMigration(v))?;
        let tx = conn.transaction()?;
        tx.execute_batch(ddl)?;
        tx.execute_batch(&format!("PRAGMA user_version = {v}"))?;
        tx.execute(
            "INSERT INTO schema_meta (key, value) VALUES ('version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [v.to_string()],
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    #[test]
    fn test_migrate_twice_is_idempotent() {
        let store = Store::open_memory().unwrap();
        let mut guard = store.lock().unwrap();
        migrate(&mut guard).unwrap();
        migrate(&mut guard).unwrap();
        let version: i64 = guard
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
        let count: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM schema_meta WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
