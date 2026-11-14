// SPDX-License-Identifier: GPL-3.0-or-later

//! Due/missed classification for cron jobs (split from `runner/mod.rs`,
//! v0.3 polish, ADR-011).
//!
//! Pure functions: one job, one moment, one verdict. The tick only acts
//! on the outcome — it never re-derives the rules.

use crate::cron::store::CronJob;
use chrono::{DateTime, Local};

/// How late a recurring firing may be and still run: twice the tick
/// interval, at least two minutes. A daemon ticking normally fires
/// within one interval of the schedule; anything older missed its tick
/// (the daemon was down) and is skipped, never fired late
/// (ZeroClaw `MissedTickBehavior::Skip`).
fn recurring_grace(tick_secs: u64) -> chrono::Duration {
    chrono::Duration::seconds((2 * tick_secs).max(120) as i64)
}

/// One-shot jobs get a fixed five-minute grace: the daemon may have
/// been starting up when the target time passed.
const ONE_SHOT_GRACE_SECS: i64 = 300;

/// A firing that started but whose daemon died leaves last_status =
/// "running". Older than this, the run is dead and the job may be
/// classified normally again.
const STALE_RUNNING_SECS: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueDecision {
    Due,
    NotDue,
    Missed(String),
}

/// Whether a job's "running" marker is fresh enough to block a new run.
/// A marker with no timestamp is malformed (only reachable by
/// hand-editing the store) and counts as stale: the next run overwrites
/// both fields, so this self-heals instead of deadlocking the job.
pub fn is_lease_fresh(job: &CronJob, now: DateTime<Local>) -> bool {
    job.last_status.as_deref() == Some("running")
        && job
            .last_run
            .is_some_and(|last| now.signed_duration_since(last).num_seconds() < STALE_RUNNING_SECS)
}

/// Pure due/missed classification for one job at one moment (unit-tested;
/// the tick only acts on the outcome).
pub fn classify_due(job: &CronJob, now: DateTime<Local>, tick_secs: u64) -> DueDecision {
    if !job.enabled {
        return DueDecision::NotDue;
    }
    let Some(target) = job.next_run else {
        return DueDecision::NotDue;
    };
    // A "running" marker from a run that started recently means the
    // daemon (or a restart of it) may still be working this job: do not
    // fire it twice. After an hour the run is dead and the job is
    // classified normally again.
    if is_lease_fresh(job, now) {
        return DueDecision::NotDue;
    }
    if target > now {
        return DueDecision::NotDue;
    }
    let late = now.signed_duration_since(target).num_seconds();
    let grace = if job.schedule.once {
        ONE_SHOT_GRACE_SECS
    } else {
        recurring_grace(tick_secs).num_seconds()
    };
    if late <= grace {
        DueDecision::Due
    } else {
        DueDecision::Missed(format!(
            "target {} was {late}s ago, past the {grace}s grace",
            target.format("%Y-%m-%d %H:%M")
        ))
    }
}
