// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeartbeatLevel {
    Info,
    Warning,
    Alert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatNudge {
    pub timestamp: DateTime<Utc>,
    pub level: HeartbeatLevel,
    pub source: String,
    pub message: String,
}

pub struct HeartbeatMonitor {
    purrcode_dir: PathBuf,
    workspace_root: PathBuf,
}

impl HeartbeatMonitor {
    pub fn new(purrcode_dir: impl Into<PathBuf>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            purrcode_dir: purrcode_dir.into(),
            workspace_root: workspace_root.into(),
        }
    }

    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    /// Checks system signals and returns nudges if any actionable conditions exist
    pub async fn check_signals(&self) -> Vec<HeartbeatNudge> {
        let mut nudges = Vec::new();
        let now = Utc::now();

        // 1. Check workspace root health
        if !self.workspace_root.exists() {
            nudges.push(HeartbeatNudge {
                timestamp: now,
                level: HeartbeatLevel::Alert,
                source: "workspace".to_string(),
                message: format!(
                    "Workspace root '{}' is missing or inaccessible.",
                    self.workspace_root.display()
                ),
            });
        }

        // 2. Check for pending candidate skills
        let candidates_dir = self.purrcode_dir.join("skills").join("candidates");
        if candidates_dir.exists() {
            if let Ok(mut rd) = tokio::fs::read_dir(&candidates_dir).await {
                let mut count = 0;
                while let Ok(Some(entry)) = rd.next_entry().await {
                    if entry.path().is_dir() {
                        count += 1;
                    }
                }
                if count > 0 {
                    nudges.push(HeartbeatNudge {
                        timestamp: now,
                        level: HeartbeatLevel::Info,
                        source: "curator".to_string(),
                        message: format!(
                            "{} candidate skill(s) waiting for review. Run `purrcode-ai curator candidates` to inspect.",
                            count
                        ),
                    });
                }
            }
        }

        // 2. Check for cron jobs
        let cron_file = self.purrcode_dir.join("cron_jobs.json");
        if cron_file.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&cron_file).await {
                if let Ok(store) = serde_json::from_str::<crate::cron::CronStore>(&content) {
                    let total = store.jobs.len();
                    let enabled = store.jobs.iter().filter(|j| j.enabled).count();
                    if total > 0 {
                        nudges.push(HeartbeatNudge {
                            timestamp: now,
                            level: HeartbeatLevel::Info,
                            source: "cron".to_string(),
                            message: format!(
                                "{} active cron job(s) scheduled ({}/{} enabled).",
                                enabled, enabled, total
                            ),
                        });
                    }
                }
            }
        }

        nudges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_heartbeat_monitor_detects_candidates_and_cron() {
        let temp = std::env::temp_dir().join(format!("purr_hb_test_{}", uuid::Uuid::new_v4()));
        let cand = temp.join("skills").join("candidates").join("auto-test");
        tokio::fs::create_dir_all(&cand).await.unwrap();

        let monitor = HeartbeatMonitor::new(&temp, &temp);
        let nudges = monitor.check_signals().await;
        assert_eq!(nudges.len(), 1);
        assert_eq!(nudges[0].source, "curator");
        assert!(nudges[0].message.contains("candidate skill(s)"));
    }
}
