// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use chrono::TimeZone;

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 11, 12, 7, 0, 0).unwrap()
}

#[test]
fn test_cron_expr_parse_and_next() {
    let expr = CronExpr::parse("0 9 * * *").expect("parses");
    assert_eq!(expr.to_expr(), "0 9 * * *");
    let next = expr.next_after(now()).expect("next run");
    assert_eq!(
        next.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 09:00"
    );
    // After 09:00 the next run is tomorrow.
    let late = Local.with_ymd_and_hms(2026, 11, 12, 10, 0, 0).unwrap();
    let next = expr.next_after(late).expect("next run");
    assert_eq!(
        next.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-13 09:00"
    );
    // Step + range + list.
    let expr = CronExpr::parse("*/15 9-17 * * 1-5").expect("parses");
    assert_eq!(expr.minutes, vec![0, 15, 30, 45]);
    assert_eq!(expr.hours, vec![9, 10, 11, 12, 13, 14, 15, 16, 17]);
    assert_eq!(expr.dow, vec![1, 2, 3, 4, 5]);
    assert_eq!(
        expr.to_expr(),
        "*/15 9,10,11,12,13,14,15,16,17 * * 1,2,3,4,5"
    );
    // Sunday as 7 maps to 0 (both spellings equivalent).
    let expr7 = CronExpr::parse("0 9 * * 7").expect("parses");
    let expr0 = CronExpr::parse("0 9 * * 0").expect("parses");
    assert_eq!(expr7, expr0);
    let expr_range = CronExpr::parse("0 9 * * 1-7").expect("parses");
    assert_eq!(expr_range.dow, vec![0, 1, 2, 3, 4, 5, 6]);
    // Invalid: wrong field count, out of range, step 0.
    assert!(CronExpr::parse("0 9 * *").is_err());
    assert!(CronExpr::parse("60 9 * * *").is_err());
    assert!(CronExpr::parse("*/0 9 * * *").is_err());
    assert!(CronExpr::parse("0 25 * * *").is_err());
}

#[test]
fn test_parse_raw_cron_passthrough() {
    let s = parse_natural("0 9 * * 1-5", now()).expect("parses");
    assert!(!s.once);
    assert!(s.expr.is_some());
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 09:00"
    );
}

#[test]
fn test_parse_intervals_id_and_en() {
    let s = parse_natural("every 30m", now()).expect("parses");
    assert_eq!(s.display, "every 30 minute(s)");
    assert_eq!(s.expr.as_deref(), Some("*/30 * * * *"));
    assert_eq!(s.next_run.format("%H:%M").to_string(), "07:30");
    let s = parse_natural("tiap 2 jam", now()).expect("parses");
    assert_eq!(s.display, "every 2 hour(s)");
    assert_eq!(s.expr.as_deref(), Some("0 */2 * * *"));
    let s = parse_natural("30 menit", now()).expect("parses");
    assert_eq!(s.display, "every 30 minute(s)");
    assert_eq!(s.expr.as_deref(), Some("*/30 * * * *"));
    let s = parse_natural("setiap 1 hari", now()).expect("parses");
    assert_eq!(s.display, "every 1 day(s)");
    assert_eq!(s.expr.as_deref(), Some("0 0 * * *"));
    // Under one minute is rejected (cron is minute-resolved).
    assert!(parse_natural("every 30 seconds", now()).is_err());
    assert!(parse_natural("tiap 45 detik", now()).is_err());
    // Seconds that are not an exact whole number of minutes: no silent
    // truncation to a shorter interval.
    assert!(parse_natural("tiap 90 detik", now()).is_err());
    assert!(parse_natural("every 90 seconds", now()).is_err());
    // Exact multiples of 60 seconds are fine ("tiap 120 detik" = 2 min).
    let s = parse_natural("tiap 120 detik", now()).expect("parses");
    assert_eq!(s.display, "every 2 minute(s)");
    assert_eq!(s.expr.as_deref(), Some("*/2 * * * *"));
    // Intervals that cannot be expressed as a 5-field cron schedule
    // must be rejected, never stored with a wrong period.
    assert!(parse_natural("every 45 minutes", now()).is_err());
    assert!(parse_natural("every 25 minutes", now()).is_err());
    assert!(parse_natural("every 90 minutes", now()).is_err());
    assert!(parse_natural("every 150 minutes", now()).is_err());
    assert!(parse_natural("every 5 hours", now()).is_err());
    assert!(parse_natural("every 7 days", now()).is_err());
    assert!(parse_natural("every 2 days", now()).is_err());
    // Divisors of 60 / 24 stay valid and store the exact period.
    let s = parse_natural("every 15 minutes", now()).expect("parses");
    assert_eq!(s.expr.as_deref(), Some("*/15 * * * *"));
    let s = parse_natural("every 6 hours", now()).expect("parses");
    assert_eq!(s.expr.as_deref(), Some("0 */6 * * *"));
}

#[test]
fn test_parse_invalid_time_is_an_error_not_a_default() {
    // A "jam"/"at" marker with an unparseable clock must not silently
    // fall back to the 09:00 default.
    assert!(parse_natural("every day at 25:00", now()).is_err());
    assert!(parse_natural("setiap hari jam 8:75", now()).is_err());
    assert!(parse_natural("setiap hari jam 99", now()).is_err());
    assert!(parse_natural("jam 99", now()).is_err());
    assert!(parse_natural("Senin jam 99", now()).is_err());
    // ...while "every day" with NO time marker still defaults to 09:00.
    let s = parse_natural("every day", now()).expect("parses");
    assert_eq!(s.display, "every day at 09:00");
    let s = parse_natural("setiap hari", now()).expect("parses");
    assert_eq!(s.display, "every day at 09:00");
}

#[test]
fn test_parse_daily_time_markers() {
    // "tiap pagi jam 8" — the PRD headline example.
    let s = parse_natural("tiap pagi jam 8", now()).expect("parses");
    assert_eq!(s.display, "every day at 08:00");
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 08:00"
    );
    let s = parse_natural("jam 8 malam", now()).expect("parses");
    assert_eq!(s.display, "every day at 20:00");
    let s = parse_natural("jam 2 siang", now()).expect("parses");
    assert_eq!(s.display, "every day at 14:00");
    let s = parse_natural("every day at 9:30pm", now()).expect("parses");
    assert_eq!(s.display, "every day at 21:30");
    let s = parse_natural("noon", now()).expect("parses");
    assert_eq!(s.display, "every day at 12:00");
}

#[test]
fn test_parse_weekdays_id_and_en() {
    let s = parse_natural("tiap Senin jam 9 pagi", now()).expect("parses");
    assert!(s.display.contains("Monday"));
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-16 09:00"
    );
    let s = parse_natural("every monday 9am", now()).expect("parses");
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-16 09:00"
    );
    let s = parse_natural("Senin, Rabu jam 9", now()).expect("parses");
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-16 09:00"
    );
    let s = parse_natural("weekday at 9am", now()).expect("parses");
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 09:00"
    );
    let s = parse_natural("hari kerja jam 8", now()).expect("parses");
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 08:00"
    );
}

#[test]
fn test_parse_relative_and_tomorrow_and_iso() {
    let s = parse_natural("in 10 minutes", now()).expect("parses");
    assert!(s.once);
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 07:10"
    );
    let s = parse_natural("dalam 2 jam", now()).expect("parses");
    assert!(s.once);
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-12 09:00"
    );
    let s = parse_natural("besok jam 8", now()).expect("parses");
    assert!(s.once);
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-13 08:00"
    );
    let s = parse_natural("2026-11-13T09:00", now()).expect("parses");
    assert!(s.once);
    assert_eq!(
        s.next_run.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-13 09:00"
    );
    assert!(parse_natural("2026-11-01T09:00", now()).is_err()); // past
}

#[test]
fn test_parse_ununderstood_is_an_error() {
    assert!(parse_natural("", now()).is_err());
    assert!(parse_natural("blah blah", now()).is_err());
    assert!(parse_natural("remind me maybe someday", now()).is_err());
}
