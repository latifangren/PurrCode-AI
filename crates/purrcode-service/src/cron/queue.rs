// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery queue (v0.3 F19, ADR-014 item 2; SQLite since ADR-015).
//!
//! One row per delivery in the `deliveries` table of
//! `<purrcode_dir>/purrcode.db` (formerly append-only JSONL at
//! `~/.purrcode/cron/deliveries.jsonl`). At-most-once claiming, ala
//! Hermes: a delivery is `pending`, then `claimed` (updated BEFORE
//! the send is attempted), then `delivered` (updated after a
//! successful send). A `claimed` record without a later `delivered`
//! is treated as delivered: losing a delivery is safer than
//! duplicating a possibly-completed send.
//!
//! The queue opens the database on every operation (like the old
//! file code opened the file per append/read), so `new()` never
//! fails — open errors surface per operation, as before.

use chrono::{DateTime, Local};
use purrcode_api::error::{PurrError, Result};
use purrcode_store::rusqlite::params;
use purrcode_store::{sql, Store};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKind {
    Result,
    Failure,
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// Stable id, `dlv_<12 hex chars>`.
    pub id: String,
    pub job_id: String,
    pub job_name: String,
    pub created_at: DateTime<Local>,
    pub kind: DeliveryKind,
    /// One-line summary (what the user sees first).
    pub summary: String,
    /// Full result text (kept in the queue; the Telegram send is chunked).
    pub detail: String,
    /// Telegram chat to deliver to, if any. `None` = queue-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Claimed,
    Delivered,
}

fn kind_str(kind: &DeliveryKind) -> &'static str {
    match kind {
        DeliveryKind::Result => "result",
        DeliveryKind::Failure => "failure",
        DeliveryKind::Heartbeat => "heartbeat",
    }
}

fn parse_kind(value: &str) -> Option<DeliveryKind> {
    match value {
        "result" => Some(DeliveryKind::Result),
        "failure" => Some(DeliveryKind::Failure),
        "heartbeat" => Some(DeliveryKind::Heartbeat),
        _ => None,
    }
}

fn parse_status(value: &str) -> Option<DeliveryStatus> {
    match value {
        "pending" => Some(DeliveryStatus::Pending),
        "claimed" => Some(DeliveryStatus::Claimed),
        "delivered" => Some(DeliveryStatus::Delivered),
        _ => None,
    }
}

fn parse_dt(value: &str) -> Result<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Local))
        .map_err(|e| {
            PurrError::Session(format!(
                "corrupt delivery queue: invalid timestamp '{value}': {e}"
            ))
        })
}

/// The delivery queue. Opens the database on every operation; see
/// the module docs.
pub struct DeliveryQueue {
    dir: PathBuf,
}

// Runner API (batch 3b); exercised only by the unit tests until then.
#[allow(dead_code)]
pub fn new_delivery_id() -> String {
    format!(
        "dlv_{}",
        &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]
    )
}

// The write side of the queue (create/claim/mark_delivered) is the job
// runner's API — it lands in batch 3b. Until then it is exercised only by
// the unit tests below, so the dead-code lint is silenced with a reason.
#[allow(dead_code)]
impl DeliveryQueue {
    pub fn new(purrcode_dir: &Path) -> Self {
        Self {
            dir: purrcode_dir.to_path_buf(),
        }
    }

    fn open(&self) -> Result<Store> {
        Ok(Store::open(&self.dir)?)
    }

    /// Record a new delivery (status pending). A duplicate id replaces
    /// the row and resets it to pending — the old event fold did the
    /// same for a duplicate `Created` event.
    pub fn create(&self, delivery: &Delivery) -> Result<()> {
        let store = self.open()?;
        let guard = store.lock()?;
        sql(guard.execute(
            "INSERT INTO deliveries
               (id, job_id, job_name, kind, status, summary, detail, chat_id, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               job_id = excluded.job_id, job_name = excluded.job_name,
               kind = excluded.kind, status = 'pending',
               summary = excluded.summary, detail = excluded.detail,
               chat_id = excluded.chat_id, created_at = excluded.created_at",
            params![
                delivery.id,
                delivery.job_id,
                delivery.job_name,
                kind_str(&delivery.kind),
                delivery.summary,
                delivery.detail,
                delivery.chat_id,
                delivery.created_at.to_rfc3339(),
            ],
        ))?;
        Ok(())
    }

    /// Claim a delivery BEFORE attempting the send (Hermes rule: a crash
    /// between claim and send loses the delivery rather than duplicating
    /// a possibly-completed one). Only a pending delivery moves to
    /// claimed; claiming an already-claimed or delivered one is a no-op.
    pub fn claim(&self, delivery_id: &str) -> Result<()> {
        let store = self.open()?;
        let guard = store.lock()?;
        sql(guard.execute(
            "UPDATE deliveries SET status = 'claimed', claimed_at = ?2
             WHERE id = ?1 AND status = 'pending'",
            params![delivery_id, Local::now().to_rfc3339()],
        ))?;
        Ok(())
    }

    /// Record a successful send, from any status (the old event fold
    /// let a `Delivered` event overwrite `Claimed` or `Pending`).
    pub fn mark_delivered(&self, delivery_id: &str) -> Result<()> {
        let store = self.open()?;
        let guard = store.lock()?;
        sql(guard.execute(
            "UPDATE deliveries SET status = 'delivered', delivered_at = ?2 WHERE id = ?1",
            params![delivery_id, Local::now().to_rfc3339()],
        ))?;
        Ok(())
    }

    /// All (delivery, latest status) pairs, newest first. A row with
    /// an unparseable kind, status, or timestamp is skipped (with a
    /// warning) instead of killing the whole queue — the old JSONL
    /// reader skipped corrupt lines the same way (v0.3 F22 fix,
    /// re-checker F4). The delivery on the bad row itself is lost;
    /// the rest still loads.
    pub fn list_all(&self) -> Result<Vec<(Delivery, DeliveryStatus)>> {
        let store = self.open()?;
        let guard = store.lock()?;
        let mut stmt = sql(guard.prepare(
            "SELECT id, job_id, job_name, kind, status, summary, detail, chat_id, created_at
             FROM deliveries ORDER BY created_at DESC, rowid DESC",
        ))?;
        let rows = sql(stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, String>(8)?,
            ))
        }))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, job_id, job_name, kind_s, status_s, summary, detail, chat_id, created_at) =
                row.map_err(purrcode_store::StoreError::from)?;
            let (Some(kind), Some(status)) = (parse_kind(&kind_s), parse_status(&status_s)) else {
                tracing::warn!(
                    "delivery queue: skipping delivery {id} with unknown kind '{kind_s}' or status '{status_s}'"
                );
                continue;
            };
            let Ok(created_at) = parse_dt(&created_at) else {
                tracing::warn!("delivery queue: skipping delivery {id} with invalid created_at");
                continue;
            };
            out.push((
                Delivery {
                    id,
                    job_id,
                    job_name,
                    created_at,
                    kind,
                    summary,
                    detail,
                    chat_id,
                },
                status,
            ));
        }
        Ok(out)
    }

    /// Deliveries whose latest status is still pending.
    pub fn pending(&self) -> Result<Vec<Delivery>> {
        let all = self.list_all()?;
        Ok(all
            .into_iter()
            .filter(|(_, s)| *s == DeliveryStatus::Pending)
            .map(|(d, _)| d)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_in_tmp() -> (DeliveryQueue, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "purr-cron-queue-test-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (DeliveryQueue::new(&dir), dir)
    }

    fn sample_delivery() -> Delivery {
        Delivery {
            id: new_delivery_id(),
            job_id: "cron_abcdef123456".to_string(),
            job_name: "pagi-ringkas".to_string(),
            created_at: Local::now(),
            kind: DeliveryKind::Result,
            summary: "Ringkasan email pagi".to_string(),
            detail: "3 email baru, 1 perlu balasan.".to_string(),
            chat_id: Some(713720576),
        }
    }

    #[test]
    fn test_delivery_queue_claim_deliver_fold() {
        let (queue, dir) = queue_in_tmp();
        assert!(queue.pending().expect("empty queue reads").is_empty());

        let d1 = sample_delivery();
        let d2 = sample_delivery();
        queue.create(&d1).expect("create d1");
        queue.create(&d2).expect("create d2");
        assert_eq!(queue.pending().expect("pending").len(), 2);

        queue.claim(&d1.id).expect("claim d1");
        let pending = queue.pending().expect("pending after claim");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, d2.id);

        // Claimed without delivered is treated as delivered (Hermes rule:
        // losing beats duplicating). d2 claimed+delivered leaves zero.
        queue.claim(&d2.id).expect("claim d2");
        queue.mark_delivered(&d2.id).expect("deliver d2");
        assert!(queue.pending().expect("pending after all").is_empty());

        let all = queue.list_all().expect("list all");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].1, DeliveryStatus::Delivered); // d2, newest first
        assert_eq!(all[1].1, DeliveryStatus::Claimed); // d1 (claimed, lost)
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delivery_queue_survives_reload() {
        let (queue, dir) = queue_in_tmp();
        let d = sample_delivery();
        let id = d.id.clone();
        queue.create(&d).expect("create");
        let queue2 = DeliveryQueue::new(&dir);
        let pending = queue2.pending().expect("pending after reload");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupt_row_does_not_kill_the_queue() {
        // A row with an unparseable kind (direct DB tampering, the SQL
        // equivalent of a corrupt JSONL line): list_all skips it and
        // still returns the valid deliveries (v0.3 F22 fix, re-checker F4).
        let (queue, dir) = queue_in_tmp();
        let d1 = sample_delivery();
        let d2 = sample_delivery();
        queue.create(&d1).expect("create d1");
        queue.create(&d2).expect("create d2");
        {
            let store = Store::open(&dir).unwrap();
            let guard = store.lock().unwrap();
            guard
                .execute(
                    "INSERT INTO deliveries
                       (id, job_id, job_name, kind, status, summary, detail, created_at)
                     VALUES ('dlv_bad0000001', 'cron_abcdef123456', 'bad', 'bogus', 'pending', '', '', '2026-11-16T00:00:00+07:00')",
                    [],
                )
                .unwrap();
        }
        let pending = queue.pending().expect("pending with a corrupt row");
        assert_eq!(pending.len(), 2);
        let all = queue.list_all().expect("list all with a corrupt row");
        assert_eq!(all.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
