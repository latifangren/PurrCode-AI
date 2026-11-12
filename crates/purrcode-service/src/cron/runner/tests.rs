// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::cron::schedule::parse_natural;
use crate::cron::store::{JobSchedule, PresetSnapshot, DEFAULT_ALLOWLIST};
use chrono::TimeZone;

fn test_job(at: DateTime<Local>, once: bool, expr: Option<&str>) -> CronJob {
    CronJob {
        id: "cron_abcdef123456".to_string(),
        name: "test-job".to_string(),
        enabled: true,
        schedule: JobSchedule {
            expr: expr.map(|s| s.to_string()),
            display: "test".to_string(),
            once,
            at: if once { Some(at) } else { None },
        },
        prompt: "do the thing".to_string(),
        tools: DEFAULT_ALLOWLIST.iter().map(|s| s.to_string()).collect(),
        preset: PresetSnapshot {
            name: "tabby".to_string(),
            approval: "supervised".to_string(),
            guardrail: "default".to_string(),
            sandbox: "confinement".to_string(),
        },
        delivery: DeliveryTarget::QueueOnly,
        created_at: at,
        last_run: None,
        next_run: Some(at),
        run_count: 0,
        failure_count: 0,
        last_status: None,
    }
}

#[test]
fn test_classify_due_recurring_within_grace() {
    let now = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 30).unwrap();
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let job = test_job(target, false, Some("0 8 * * *"));
    assert_eq!(classify_due(&job, now, 60), DueDecision::Due);
}

#[test]
fn test_classify_due_recurring_missed_is_skipped_not_fired() {
    let now = Local.with_ymd_and_hms(2026, 11, 12, 12, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let job = test_job(target, false, Some("0 8 * * *"));
    assert!(matches!(
        classify_due(&job, now, 60),
        DueDecision::Missed(_)
    ));
}

#[test]
fn test_classify_due_one_shot_grace_and_missed() {
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let job = test_job(target, true, None);
    let inside = Local.with_ymd_and_hms(2026, 11, 12, 8, 4, 0).unwrap();
    assert_eq!(classify_due(&job, inside, 60), DueDecision::Due);
    let outside = Local.with_ymd_and_hms(2026, 11, 12, 8, 6, 0).unwrap();
    assert!(matches!(
        classify_due(&job, outside, 60),
        DueDecision::Missed(_)
    ));
}

#[test]
fn test_classify_due_not_due_future_and_disabled() {
    let now = Local.with_ymd_and_hms(2026, 11, 12, 7, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let job = test_job(target, false, Some("0 8 * * *"));
    assert_eq!(classify_due(&job, now, 60), DueDecision::NotDue);
    let mut disabled = test_job(now, false, Some("0 8 * * *"));
    disabled.enabled = false;
    assert_eq!(classify_due(&disabled, now, 60), DueDecision::NotDue);
}

#[test]
fn test_classify_due_fresh_lease_blocks_and_stale_marker_fires() {
    let now = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 30).unwrap();
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let mut job = test_job(target, false, Some("0 8 * * *"));
    job.last_status = Some("running".to_string());
    job.last_run = Some(now - chrono::Duration::seconds(10));
    assert_eq!(classify_due(&job, now, 60), DueDecision::NotDue);
    // Stale marker: classified normally (Due, within grace).
    job.last_run = Some(now - chrono::Duration::seconds(7200));
    assert_eq!(classify_due(&job, now, 60), DueDecision::Due);
    // Marker with no timestamp: malformed, treated as stale (Due).
    job.last_run = None;
    assert_eq!(classify_due(&job, now, 60), DueDecision::Due);
}

#[test]
fn test_is_lease_fresh_cases() {
    let now = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    // Fresh marker (started 10s ago) blocks.
    let mut job = test_job(target, false, Some("0 8 * * *"));
    job.last_status = Some("running".to_string());
    job.last_run = Some(now - chrono::Duration::seconds(10));
    assert!(is_lease_fresh(&job, now));
    // Stale marker (started 2h ago) does not block.
    job.last_run = Some(now - chrono::Duration::seconds(7200));
    assert!(!is_lease_fresh(&job, now));
    // Marker with no timestamp is malformed (hand-edit only): stale.
    job.last_run = None;
    assert!(!is_lease_fresh(&job, now));
    // Not running at all: no lease.
    job.last_status = Some("ok".to_string());
    job.last_run = Some(now - chrono::Duration::seconds(10));
    assert!(!is_lease_fresh(&job, now));
}

#[test]
fn test_classify_due_stale_running_skips_then_expires() {
    let target = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let mut job = test_job(target, false, Some("0 8 * * *"));
    job.last_status = Some("running".to_string());
    job.last_run = Some(target);
    // 30 minutes after the run started: still "running", skip.
    let soon = Local.with_ymd_and_hms(2026, 11, 12, 8, 30, 0).unwrap();
    assert_eq!(classify_due(&job, soon, 60), DueDecision::NotDue);
    // 2 hours later the marker is stale: the (long-missed) firing is
    // classified normally — here as Missed, never fired late.
    let later = Local.with_ymd_and_hms(2026, 11, 12, 10, 0, 0).unwrap();
    assert!(matches!(
        classify_due(&job, later, 60),
        DueDecision::Missed(_)
    ));
}

#[test]
fn test_parse_preset_enums_roundtrip() {
    let snapshot = PresetSnapshot {
        name: "tabby".to_string(),
        approval: "supervised".to_string(),
        guardrail: "default".to_string(),
        sandbox: "confinement".to_string(),
    };
    let (a, g, s) = parse_preset_enums(&snapshot).expect("parses");
    assert_eq!(a, ApprovalMode::Supervised);
    assert_eq!(g, GuardrailMode::Default);
    assert_eq!(s, SandboxMode::Confinement);
    let bad = PresetSnapshot {
        approval: "warp".to_string(),
        ..snapshot
    };
    assert!(parse_preset_enums(&bad).is_err());
}

#[test]
fn test_schedule_next_after_advances() {
    // The runner advances a recurring job with the stored expr.
    let now = Local.with_ymd_and_hms(2026, 11, 12, 8, 0, 0).unwrap();
    let parsed = parse_natural("tiap pagi jam 8", now).expect("parses");
    assert_eq!(parsed.expr.as_deref(), Some("0 8 * * *"));
    let expr = CronExpr::parse(parsed.expr.as_deref().unwrap()).expect("parses");
    let next = expr.next_after(now).expect("has next");
    assert_eq!(next, Local.with_ymd_and_hms(2026, 11, 13, 8, 0, 0).unwrap());
}
