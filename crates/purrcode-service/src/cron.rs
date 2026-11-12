// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use purrcode_api::error::{PurrError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CronJob {
    pub id: String,
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub last_run: Option<DateTime<Utc>>,
    #[serde(default)]
    pub next_run: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl CronJob {
    /// PRD §4.3 F19: Cron allowlist fail-closed validation.
    /// Mutation tools (write, edit, bash, manage_skill) MUST be explicitly declared in allowed_tools.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if let Some(ref allowed) = self.allowed_tools {
            allowed.iter().any(|t| t == tool_name || t == "*")
        } else {
            // Fail-closed default: strictly read-only inspection tools
            matches!(
                tool_name,
                "read"
                    | "grep"
                    | "glob"
                    | "find"
                    | "todo"
                    | "ask"
                    | "websearch"
                    | "webfetch"
                    | "read_skill"
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronStore {
    #[serde(default)]
    pub jobs: Vec<CronJob>,
}

#[async_trait]
pub trait CronJobDispatcher: Send + Sync {
    async fn dispatch(&self, job: &CronJob) -> Result<String>;
}

/// Parses a natural language or cron expression and computes the next run timestamp
pub fn compute_next_run(schedule: &str, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let s = schedule.trim().to_lowercase();

    // 1. Natural language intervals: "every Xm", "every X minutes", "every Xh", "every X hours", "every Xs", "every X seconds"
    if let Some(rest) = s.strip_prefix("every ") {
        let rest = rest.trim();
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.is_empty() {
            return Err(PurrError::Config(format!(
                "Invalid schedule: '{}'",
                schedule
            )));
        }

        if parts.len() == 1 {
            let token = parts[0];
            if let Some(num) = token.strip_suffix('s') {
                let secs: i64 = num.parse().map_err(|_| {
                    PurrError::Config(format!("Invalid seconds in schedule: '{}'", schedule))
                })?;
                return Ok(from + ChronoDuration::seconds(secs));
            } else if let Some(num) = token.strip_suffix('m') {
                let mins: i64 = num.parse().map_err(|_| {
                    PurrError::Config(format!("Invalid minutes in schedule: '{}'", schedule))
                })?;
                return Ok(from + ChronoDuration::minutes(mins));
            } else if let Some(num) = token.strip_suffix('h') {
                let hrs: i64 = num.parse().map_err(|_| {
                    PurrError::Config(format!("Invalid hours in schedule: '{}'", schedule))
                })?;
                return Ok(from + ChronoDuration::hours(hrs));
            } else if let Some(num) = token.strip_suffix('d') {
                let days: i64 = num.parse().map_err(|_| {
                    PurrError::Config(format!("Invalid days in schedule: '{}'", schedule))
                })?;
                return Ok(from + ChronoDuration::days(days));
            }
        } else if parts.len() == 2 {
            let num: i64 = parts[0].parse().map_err(|_| {
                PurrError::Config(format!("Invalid number in schedule: '{}'", schedule))
            })?;
            let unit = parts[1];
            if unit.starts_with("sec") {
                return Ok(from + ChronoDuration::seconds(num));
            } else if unit.starts_with("min") {
                return Ok(from + ChronoDuration::minutes(num));
            } else if unit.starts_with("hour") || unit.starts_with("hr") {
                return Ok(from + ChronoDuration::hours(num));
            } else if unit.starts_with("day") {
                return Ok(from + ChronoDuration::days(num));
            }
        }
    }

    // 2. "daily at HH:MM" or "every day at HH:MM"
    let time_spec = s
        .strip_prefix("daily at ")
        .or_else(|| s.strip_prefix("every day at "))
        .map(|t| t.trim());

    if let Some(time_str) = time_spec {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() == 2 {
            let hour: u32 = parts[0].parse().map_err(|_| {
                PurrError::Config(format!("Invalid hour in schedule: '{}'", schedule))
            })?;
            let min: u32 = parts[1].parse().map_err(|_| {
                PurrError::Config(format!("Invalid minute in schedule: '{}'", schedule))
            })?;

            let mut target = from
                .with_hour(hour)
                .and_then(|dt| dt.with_minute(min))
                .and_then(|dt| dt.with_second(0))
                .and_then(|dt| dt.with_nanosecond(0))
                .ok_or_else(|| {
                    PurrError::Config(format!("Invalid time calculated from '{}'", schedule))
                })?;

            if target <= from {
                target += ChronoDuration::days(1);
            }
            return Ok(target);
        }
    }

    // 3. Simple cron syntax fallback: "minute hour dom month dow" (5 parts)
    let cron_parts: Vec<&str> = s.split_whitespace().collect();
    if cron_parts.len() == 5 {
        // Simple interval parser: if */X in minute, every X minutes
        if let Some(step) = cron_parts[0].strip_prefix("*/") {
            let mins: i64 = step.parse().map_err(|_| {
                PurrError::Config(format!("Invalid cron step in schedule: '{}'", schedule))
            })?;
            return Ok(from + ChronoDuration::minutes(mins));
        } else if cron_parts[0] != "*" && cron_parts[1] != "*" {
            // Specific min and hour
            let min: u32 = cron_parts[0].parse().map_err(|_| {
                PurrError::Config(format!("Invalid cron minute in schedule: '{}'", schedule))
            })?;
            let hour: u32 = cron_parts[1].parse().map_err(|_| {
                PurrError::Config(format!("Invalid cron hour in schedule: '{}'", schedule))
            })?;
            let mut target = from
                .with_hour(hour)
                .and_then(|dt| dt.with_minute(min))
                .and_then(|dt| dt.with_second(0))
                .and_then(|dt| dt.with_nanosecond(0))
                .ok_or_else(|| {
                    PurrError::Config(format!("Invalid cron time from '{}'", schedule))
                })?;
            if target <= from {
                target += ChronoDuration::days(1);
            }
            return Ok(target);
        }
    }

    // Fallback default: 1 hour from now
    Ok(from + ChronoDuration::hours(1))
}

pub struct CronScheduler {
    storage_path: PathBuf,
    store: RwLock<CronStore>,
}

impl CronScheduler {
    pub fn new(storage_path: impl Into<PathBuf>) -> Self {
        let path = storage_path.into();
        let store = Self::load_from_disk(&path).unwrap_or_default();
        Self {
            storage_path: path,
            store: RwLock::new(store),
        }
    }

    fn load_from_disk(path: &Path) -> Result<CronStore> {
        if !path.exists() {
            return Ok(CronStore::default());
        }
        let data = std::fs::read_to_string(path)?;
        let store: CronStore = serde_json::from_str(&data)?;
        Ok(store)
    }

    async fn save_to_disk(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let store = self.store.read().await;
        let data = serde_json::to_string_pretty(&*store)?;
        tokio::fs::write(&self.storage_path, data).await?;
        Ok(())
    }

    pub async fn add_job(&self, mut job: CronJob) -> Result<()> {
        let now = Utc::now();
        if job.next_run.is_none() {
            job.next_run = Some(compute_next_run(&job.schedule, now)?);
        }

        {
            let mut store = self.store.write().await;
            if let Some(pos) = store.jobs.iter().position(|j| j.id == job.id) {
                store.jobs[pos] = job;
            } else {
                store.jobs.push(job);
            }
        }
        self.save_to_disk().await?;
        Ok(())
    }

    pub async fn remove_job(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut store = self.store.write().await;
            let init_len = store.jobs.len();
            store.jobs.retain(|j| j.id != id);
            store.jobs.len() < init_len
        };
        if removed {
            self.save_to_disk().await?;
        }
        Ok(removed)
    }

    pub async fn list_jobs(&self) -> Vec<CronJob> {
        self.store.read().await.jobs.clone()
    }

    pub async fn tick(
        &self,
        dispatcher: &Arc<dyn CronJobDispatcher>,
    ) -> Result<Vec<(String, Result<String>)>> {
        let now = Utc::now();
        let mut due_jobs = Vec::new();

        {
            let store = self.store.read().await;
            for job in &store.jobs {
                if !job.enabled {
                    continue;
                }
                if let Some(next) = job.next_run {
                    if next <= now {
                        due_jobs.push(job.clone());
                    }
                }
            }
        }

        let mut results = Vec::new();

        for mut job in due_jobs {
            let res = dispatcher.dispatch(&job).await;
            results.push((job.id.clone(), res));

            // Update execution timestamps
            let next = compute_next_run(&job.schedule, now).ok();
            job.last_run = Some(now);
            job.next_run = next;

            {
                let mut store = self.store.write().await;
                if let Some(pos) = store.jobs.iter().position(|j| j.id == job.id) {
                    store.jobs[pos] = job;
                }
            }
        }

        if !results.is_empty() {
            let _ = self.save_to_disk().await;
        }

        Ok(results)
    }

    pub fn start_scheduler_loop(
        self: Arc<Self>,
        dispatcher: Arc<dyn CronJobDispatcher>,
        tick_interval: Duration,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_interval);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = self.tick(&dispatcher).await {
                            tracing::warn!("CronScheduler tick error: {}", e);
                        }
                    }
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tracing::info!("CronScheduler received shutdown signal");
                            break;
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_next_run_intervals() {
        let base = Utc::now();
        let next_30s = compute_next_run("every 30s", base).unwrap();
        assert_eq!((next_30s - base).num_seconds(), 30);

        let next_5m = compute_next_run("every 5m", base).unwrap();
        assert_eq!((next_5m - base).num_minutes(), 5);

        let next_2h = compute_next_run("every 2 hours", base).unwrap();
        assert_eq!((next_2h - base).num_hours(), 2);
    }

    #[test]
    fn test_compute_next_run_daily() {
        let base = Utc::now();
        let next_daily = compute_next_run("daily at 10:00", base).unwrap();
        assert!(next_daily > base);
        assert_eq!(next_daily.hour(), 10);
        assert_eq!(next_daily.minute(), 0);
    }

    struct MockDispatcher;

    #[async_trait]
    impl CronJobDispatcher for MockDispatcher {
        async fn dispatch(&self, job: &CronJob) -> Result<String> {
            Ok(format!("Dispatched job '{}'", job.id))
        }
    }

    #[tokio::test]
    async fn test_cron_scheduler_crud_and_tick() {
        let temp_dir =
            std::env::temp_dir().join(format!("purrcode_cron_test_{}", uuid::Uuid::new_v4()));
        let cron_file = temp_dir.join("cron.json");

        let scheduler = CronScheduler::new(&cron_file);
        let job = CronJob {
            id: "job-1".to_string(),
            schedule: "every 10s".to_string(),
            prompt: "Perform daily health check".to_string(),
            role: Some("tinker".to_string()),
            model: None,
            allowed_tools: None,
            last_run: None,
            next_run: Some(Utc::now() - ChronoDuration::seconds(5)), // Due immediately
            enabled: true,
        };

        assert!(job.is_tool_allowed("read"));
        assert!(job.is_tool_allowed("grep"));
        assert!(!job.is_tool_allowed("bash")); // Denied by fail-closed default
        assert!(!job.is_tool_allowed("write"));

        scheduler.add_job(job).await.unwrap();
        assert_eq!(scheduler.list_jobs().await.len(), 1);

        let dispatcher: Arc<dyn CronJobDispatcher> = Arc::new(MockDispatcher);
        let results = scheduler.tick(&dispatcher).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "job-1");
        assert!(results[0].1.is_ok());

        // Verify next_run was updated
        let jobs = scheduler.list_jobs().await;
        assert!(jobs[0].next_run.unwrap() > Utc::now());

        // Remove job
        assert!(scheduler.remove_job("job-1").await.unwrap());
        assert_eq!(scheduler.list_jobs().await.len(), 0);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
