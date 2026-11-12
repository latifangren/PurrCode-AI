// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery queue (v0.3 F19, ADR-014 item 2).
//!
//! Append-only JSONL at `~/.purrcode/cron/deliveries.jsonl` — one line per
//! event, mirroring the `audit.jsonl` pattern. At-most-once claiming, ala
//! Hermes: a delivery is `created`, then `claimed` (appended BEFORE the
//! send is attempted), then `delivered` (appended after a successful send).
//! A `claimed` record without a later `delivered` is treated as delivered:
//! losing a delivery is safer than duplicating a possibly-completed send.

use chrono::{DateTime, Local};
use purrcode_api::error::{PurrError, Result};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DeliveryEvent {
    Created(Delivery),
    Claimed { delivery_id: String },
    Delivered { delivery_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Claimed,
    Delivered,
}

pub fn deliveries_file_path(purrcode_dir: &Path) -> PathBuf {
    crate::cron::cron_dir(purrcode_dir).join("deliveries.jsonl")
}

/// The append-only delivery queue.
pub struct DeliveryQueue {
    path: PathBuf,
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
            path: deliveries_file_path(purrcode_dir),
        }
    }

    fn append_line(&self, line: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                PurrError::Session(format!(
                    "Failed to create cron dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                PurrError::Session(format!(
                    "Failed to open delivery queue {}: {e}",
                    self.path.display()
                ))
            })?;
        writeln!(file, "{line}").map_err(|e| {
            PurrError::Session(format!(
                "Failed to append to delivery queue {}: {e}",
                self.path.display()
            ))
        })?;
        Ok(())
    }

    /// Record a new delivery (status pending).
    pub fn create(&self, delivery: &Delivery) -> Result<()> {
        let event = DeliveryEvent::Created(delivery.clone());
        let line = serde_json::to_string(&event)
            .map_err(|e| PurrError::Session(format!("Failed to serialize delivery: {e}")))?;
        self.append_line(&line)
    }

    /// Claim a delivery BEFORE attempting the send (Hermes rule: a crash
    /// between claim and send loses the delivery rather than duplicating
    /// a possibly-completed one).
    pub fn claim(&self, delivery_id: &str) -> Result<()> {
        let event = DeliveryEvent::Claimed {
            delivery_id: delivery_id.to_string(),
        };
        let line = serde_json::to_string(&event)
            .map_err(|e| PurrError::Session(format!("Failed to serialize delivery claim: {e}")))?;
        self.append_line(&line)
    }

    /// Record a successful send.
    pub fn mark_delivered(&self, delivery_id: &str) -> Result<()> {
        let event = DeliveryEvent::Delivered {
            delivery_id: delivery_id.to_string(),
        };
        let line = serde_json::to_string(&event)
            .map_err(|e| PurrError::Session(format!("Failed to serialize delivery status: {e}")))?;
        self.append_line(&line)
    }

    fn read_events(&self) -> Result<Vec<DeliveryEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path).map_err(|e| {
            PurrError::Session(format!(
                "Failed to read delivery queue {}: {e}",
                self.path.display()
            ))
        })?;
        let mut events = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: DeliveryEvent = serde_json::from_str(line).map_err(|e| {
                PurrError::Session(format!(
                    "Invalid delivery queue line {} in {}: {e}",
                    i + 1,
                    self.path.display()
                ))
            })?;
            events.push(event);
        }
        Ok(events)
    }

    /// Fold the log into (delivery, latest status) pairs, newest first.
    pub fn list_all(&self) -> Result<Vec<(Delivery, DeliveryStatus)>> {
        let mut order: Vec<String> = Vec::new();
        let mut by_id: std::collections::HashMap<String, (Delivery, DeliveryStatus)> =
            std::collections::HashMap::new();
        for event in self.read_events()? {
            match event {
                DeliveryEvent::Created(d) => {
                    if !by_id.contains_key(&d.id) {
                        order.push(d.id.clone());
                    }
                    by_id.insert(d.id.clone(), (d, DeliveryStatus::Pending));
                }
                DeliveryEvent::Claimed { delivery_id } => {
                    if let Some((_, status)) = by_id.get_mut(&delivery_id) {
                        if *status == DeliveryStatus::Pending {
                            *status = DeliveryStatus::Claimed;
                        }
                    }
                }
                DeliveryEvent::Delivered { delivery_id } => {
                    if let Some((_, status)) = by_id.get_mut(&delivery_id) {
                        *status = DeliveryStatus::Delivered;
                    }
                }
            }
        }
        let mut out: Vec<(Delivery, DeliveryStatus)> = order
            .into_iter()
            .filter_map(|id| by_id.remove(&id))
            .collect();
        out.reverse();
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
}
