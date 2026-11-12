// SPDX-License-Identifier: GPL-3.0-or-later

//! Natural-language schedule parsing + cron expression evaluation (v0.3 F19).
//!
//! Deterministic, no model involved (ala Hermes `parse_schedule`): weekday
//! and time tables, durations, raw cron passthrough, ISO one-shots. The
//! parser understands Indonesian and English, because the PRD example is
//! Indonesian ("tiap pagi jam 8") and the user writes in Indonesian.
//! Everything here is pure: `now` is a parameter so tests are hermetic.
//! Schedules are evaluated in local time.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use purrcode_api::error::{PurrError, Result};

/// A parsed schedule: a recurring 5-field cron expression in local time,
/// or a one-shot time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSchedule {
    /// 5-field cron expression (minute hour dom month dow), local time.
    /// `None` for one-shot schedules.
    pub expr: Option<String>,
    /// Human-readable echo of what was understood (shown at creation).
    pub display: String,
    /// Next firing time (local). For one-shot, the target time.
    pub next_run: DateTime<Local>,
    /// True for one-shot (fires once), false for recurring.
    pub once: bool,
}

/// A 5-field cron expression (minute hour dom month dow), local time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    pub minutes: Vec<u32>,
    pub hours: Vec<u32>,
    pub dom: Vec<u32>,
    pub months: Vec<u32>,
    pub dow: Vec<u32>, // 0 = Sunday .. 6 = Saturday
}

fn parse_field(input: &str, lo: u32, hi: u32, names: &[(&str, u32)]) -> Result<Vec<u32>> {
    parse_field_mapped(input, lo, hi, names, false)
}

/// Like `parse_field`, but a value of 7 is treated as Sunday (0) — cron
/// allows both 0 and 7 for Sunday. The effective upper bound becomes 7.
fn parse_field_mapped(
    input: &str,
    lo: u32,
    hi: u32,
    names: &[(&str, u32)],
    seven_is_sunday: bool,
) -> Result<Vec<u32>> {
    let hi_eff = if seven_is_sunday { 7 } else { hi };
    let map = |v: u32| if seven_is_sunday && v == 7 { 0 } else { v };
    let mut out: Vec<u32> = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(PurrError::Config(format!(
                "Invalid cron field '{input}': empty list element"
            )));
        }
        let (range, step): (&str, u32) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = parse_named(s, names)
                    .ok_or_else(|| PurrError::Config(format!("Invalid cron step '{s}'")))?;
                if step == 0 {
                    return Err(PurrError::Config(
                        "Invalid cron step 0 (step must be >= 1)".to_string(),
                    ));
                }
                (r, step)
            }
            None => (part, 1),
        };
        let (start, end) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let a = parse_named(a, names).ok_or_else(|| {
                PurrError::Config(format!("Invalid cron value '{a}' in '{input}'"))
            })?;
            let b = parse_named(b, names).ok_or_else(|| {
                PurrError::Config(format!("Invalid cron value '{b}' in '{input}'"))
            })?;
            (a, b)
        } else {
            let v = parse_named(range, names).ok_or_else(|| {
                PurrError::Config(format!("Invalid cron value '{range}' in '{input}'"))
            })?;
            (v, v)
        };
        if start > end || start < lo || end > hi_eff {
            return Err(PurrError::Config(format!(
                "Cron range {start}-{end} out of bounds {lo}-{hi_eff} in '{input}'"
            )));
        }
        let mut v = start;
        while v <= end {
            let mapped = map(v);
            if !out.contains(&mapped) {
                out.push(mapped);
            }
            v += step;
        }
    }
    out.sort_unstable();
    Ok(out)
}

fn parse_named(token: &str, names: &[(&str, u32)]) -> Option<u32> {
    let lower = token.to_ascii_lowercase();
    for (name, value) in names {
        if lower == *name {
            return Some(*value);
        }
    }
    token.parse::<u32>().ok()
}

const MONTH_NAMES: &[(&str, u32)] = &[
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];
const DOW_NAMES: &[(&str, u32)] = &[
    ("sun", 0),
    ("mon", 1),
    ("tue", 2),
    ("wed", 3),
    ("thu", 4),
    ("fri", 5),
    ("sat", 6),
];

impl CronExpr {
    /// Parse a 5-field cron expression (minute hour dom month dow).
    pub fn parse(expr: &str) -> Result<Self> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(PurrError::Config(format!(
                "Invalid cron expression '{expr}': expected 5 fields (minute hour day month weekday)"
            )));
        }
        Ok(Self {
            minutes: parse_field(fields[0], 0, 59, &[])?,
            hours: parse_field(fields[1], 0, 23, &[])?,
            dom: parse_field(fields[2], 1, 31, &[])?,
            months: parse_field(fields[3], 1, 12, MONTH_NAMES)?,
            dow: parse_field_mapped(fields[4], 0, 6, DOW_NAMES, true)?,
        })
    }

    /// Canonical compact 5-field form (for storage and display): a full
    /// range renders as `*`, a full-range step as `*/n`, else a list.
    pub fn to_expr(&self) -> String {
        fn fmt(values: &[u32], lo: u32, hi: u32) -> String {
            let full: Vec<u32> = (lo..=hi).collect();
            if values == full.as_slice() {
                return "*".to_string();
            }
            // Two values are enough to pin a step: [0, 30] is `*/30`,
            // not a list.
            if values.len() >= 2 {
                let step = values[1] - values[0];
                if step > 0
                    && values[0] == lo
                    && values
                        .iter()
                        .enumerate()
                        .all(|(i, v)| *v == lo + (i as u32) * step && *v <= hi)
                    && values.last() == Some(&(lo + ((hi - lo) / step) * step))
                {
                    return format!("*/{step}");
                }
            }
            values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        }
        format!(
            "{} {} {} {} {}",
            fmt(&self.minutes, 0, 59),
            fmt(&self.hours, 0, 23),
            fmt(&self.dom, 1, 31),
            fmt(&self.months, 1, 12),
            fmt(&self.dow, 0, 6)
        )
    }

    pub fn matches(&self, dt: DateTime<Local>) -> bool {
        self.minutes.contains(&dt.minute())
            && self.hours.contains(&dt.hour())
            && self.dom.contains(&dt.day())
            && self.months.contains(&dt.month())
            // chrono: Monday = 1 .. Sunday = 7; cron: Sunday = 0.
            && self.dow.contains(&dt.weekday().num_days_from_sunday())
    }

    /// First firing strictly after `from` (minute resolution). Searches up
    /// to 366 days ahead; returns `None` only for an expression that can
    /// never fire (e.g. Feb 31).
    pub fn next_after(&self, from: DateTime<Local>) -> Option<DateTime<Local>> {
        let start = from
            .with_second(0)
            .and_then(|d| d.with_nanosecond(0))
            .unwrap_or(from)
            + chrono::Duration::minutes(1);
        let mut dt = start;
        for _ in 0..(366 * 24 * 60) {
            if self.matches(dt) {
                return Some(dt);
            }
            dt += chrono::Duration::minutes(1);
        }
        None
    }
}

const WEEKDAYS: &[(&str, u32)] = &[
    // Indonesian (cron dow: Sunday = 0).
    ("senin", 1),
    ("selasa", 2),
    ("rabu", 3),
    ("kamis", 4),
    ("jumat", 5),
    ("jum'at", 5),
    ("sabtu", 6),
    ("minggu", 0),
    // English.
    ("monday", 1),
    ("tuesday", 2),
    ("wednesday", 3),
    ("thursday", 4),
    ("friday", 5),
    ("saturday", 6),
    ("sunday", 0),
    ("mon", 1),
    ("tue", 2),
    ("tues", 2),
    ("wed", 3),
    ("thu", 4),
    ("thur", 4),
    ("thurs", 4),
    ("fri", 5),
    ("sat", 6),
    ("sun", 0),
];

fn weekday_name(dow: u32) -> &'static str {
    match dow {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        _ => "Saturday",
    }
}

/// Parse a clock token: "9", "9am", "9:30", "9:30pm", "14:00".
/// Returns (hour 0-23, minute). The caller applies a following
/// pagi/siang/sore/malam/am/pm marker if present.
fn parse_clock(token: &str) -> Option<(u32, u32)> {
    let lower = token.to_ascii_lowercase();
    let (num, suffix) = if let Some(s) = lower.strip_suffix("am") {
        (s, "am")
    } else if let Some(s) = lower.strip_suffix("pm") {
        (s, "pm")
    } else {
        (lower.as_str(), "")
    };
    let (h_str, m_str) = match num.split_once(':') {
        Some((h, m)) => (h, Some(m)),
        None => (num, None),
    };
    let mut hour: u32 = h_str.parse().ok()?;
    let minute: u32 = match m_str {
        Some(m) => m.parse().ok()?,
        None => 0,
    };
    if minute > 59 {
        return None;
    }
    match suffix {
        "am" => {
            if hour == 12 {
                hour = 0;
            } else if hour > 12 {
                return None;
            }
        }
        "pm" => {
            if hour == 12 {
                hour = 12;
            } else if hour < 12 {
                hour += 12;
            } else {
                return None;
            }
        }
        _ => {
            if hour > 23 {
                return None;
            }
        }
    }
    Some((hour, minute))
}

/// Apply an Indonesian time-of-day marker to a 12-hour clock value.
/// "jam 8 pagi" = 8, "jam 2 siang" = 14, "jam 4 sore" = 16,
/// "jam 8 malam" = 20, "jam 12 malam" = 0, "jam 12 siang" = 12.
fn apply_marker(hour: u32, minute: u32, marker: &str) -> Option<(u32, u32)> {
    if hour > 12 || minute > 59 {
        return None;
    }
    let h = match marker {
        "pagi" => {
            if hour == 12 {
                0
            } else {
                hour
            }
        }
        "siang" => {
            if hour == 12 {
                12
            } else {
                hour + 12
            }
        }
        "sore" | "malam" => {
            if hour == 12 {
                0
            } else {
                hour + 12
            }
        }
        _ => return None,
    };
    Some((h, minute))
}

/// Try to read a clock from the token stream at `i`, consuming the clock
/// token and an optional following marker ("am"/"pm"/"pagi"/"siang"/"sore"/
/// "malam"). Returns ((hour, minute), tokens consumed).
fn read_clock(tokens: &[String], i: usize) -> Option<((u32, u32), usize)> {
    let token = tokens.get(i)?;
    // "noon" / "midnight" / "tengah hari" / "tengah malam".
    match token.as_str() {
        "noon" => return Some(((12, 0), 1)),
        "midnight" => return Some(((0, 0), 1)),
        "tengah" => match tokens.get(i + 1).map(|t| t.as_str()) {
            Some("hari") => return Some(((12, 0), 2)),
            Some("malam") => return Some(((0, 0), 2)),
            _ => return None,
        },
        _ => {}
    }
    // "9am" / "9:30pm" carry their own marker.
    if let Some((h, m)) = parse_clock(token) {
        let lower = token.to_ascii_lowercase();
        if lower.ends_with("am") || lower.ends_with("pm") {
            return Some(((h, m), 1));
        }
        // A following marker word applies to a bare clock.
        if let Some(marker) = tokens.get(i + 1) {
            let marker = marker.as_str();
            if matches!(marker, "pagi" | "siang" | "sore" | "malam") {
                return apply_marker(h, m, marker).map(|c| (c, 2));
            }
        }
        return Some(((h, m), 1));
    }
    None
}

/// Duration in minutes for "30m" / "30 menit" / "2 jam" / "1 hari".
/// Returns None when the text is not a duration. Durations under one
/// minute are rejected by the caller (cron is minute-resolved).
fn parse_duration(tokens: &[String], i: usize) -> Option<(u64, usize)> {
    let token = tokens.get(i)?;
    let lower = token.to_ascii_lowercase();
    // Compact form: "30m", "2h", "1d", "45s".
    if let Some((num, unit)) = split_number_suffix(&lower) {
        // Seconds must be an exact whole number of minutes: "90s" is not
        // "every 1 minute" — it is unrepresentable in minute resolution,
        // so it is rejected (None) instead of silently truncated.
        let mins = match unit {
            "m" => Some(num),
            "h" => Some(num * 60),
            "d" => Some(num * 60 * 24),
            "s" => (num % 60 == 0).then_some(num / 60),
            _ => None,
        };
        return mins.map(|m| (m, 1));
    }
    // Two-token form: "30 menit" / "2 jam" / "1 hari" / "45 detik".
    if let Ok(n) = token.parse::<u64>() {
        if let Some(unit) = tokens.get(i + 1) {
            let mins = match unit.as_str() {
                "menit" | "minute" | "minutes" | "min" | "mins" => Some(n),
                "jam" | "hour" | "hours" | "hr" | "hrs" => Some(n * 60),
                "hari" | "day" | "days" => Some(n * 60 * 24),
                // Same rule as the compact form: only exact whole minutes.
                "detik" | "second" | "seconds" | "sec" | "secs" => (n % 60 == 0).then_some(n / 60),
                _ => None,
            };
            if let Some(m) = mins {
                return Some((m, 2));
            }
        }
    }
    None
}

fn split_number_suffix(s: &str) -> Option<(u64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit())?;
    if end == 0 {
        return None;
    }
    let num: u64 = s[..end].parse().ok()?;
    let unit = &s[end..];
    if matches!(unit, "m" | "h" | "d" | "s") {
        Some((num, unit))
    } else {
        None
    }
}

/// Parse a natural-language schedule (Indonesian or English) into a cron
/// expression or a one-shot time. Deterministic — no model involved.
pub fn parse_natural(input: &str, now: DateTime<Local>) -> Result<ParsedSchedule> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PurrError::Config(
            "Empty schedule: try 'tiap pagi jam 8', 'setiap Senin jam 9', 'every 30 menit', or a cron expression like '0 9 * * *'".to_string(),
        ));
    }
    let err = || {
        PurrError::Config(format!(
            "Cannot understand schedule '{trimmed}': try 'tiap pagi jam 8', 'setiap Senin jam 9', 'every 30 menit', 'in 10 minutes', 'besok jam 8', or a cron expression like '0 9 * * *'"
        ))
    };

    // 1. Raw 5-field cron passthrough (keeps the user's spelling).
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() == 5 {
        if let Ok(expr) = CronExpr::parse(trimmed) {
            let next = expr.next_after(now).ok_or_else(err)?;
            return Ok(ParsedSchedule {
                expr: Some(trimmed.to_string()),
                display: format!("cron '{trimmed}' (local time)"),
                next_run: next,
                once: false,
            });
        }
    }

    // 2. ISO one-shot: "2026-11-13T09:00" / "2026-11-13 09:00".
    if let Some(dt) = parse_iso_datetime(trimmed, now) {
        if dt <= now {
            return Err(PurrError::Config(format!(
                "Schedule time {dt} is in the past"
            )));
        }
        return Ok(ParsedSchedule {
            expr: None,
            display: format!("once at {}", dt.format("%Y-%m-%d %H:%M")),
            next_run: dt,
            once: true,
        });
    }

    // Tokenize: lowercase, split on whitespace and commas.
    let tokens: Vec<String> = trimmed
        .to_ascii_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();
    let has = |w: &str| tokens.iter().any(|t| t == w);

    // 3. Relative one-shot: "in 10 minutes" / "dalam 10 menit".
    if has("in") || has("dalam") {
        let anchor = tokens
            .iter()
            .position(|t| t == "in" || t == "dalam")
            .unwrap_or(0);
        if let Some((mins, _)) = parse_duration(&tokens, anchor + 1) {
            if mins == 0 {
                return Err(PurrError::Config(
                    "Interval under one minute is not supported (cron is minute-resolved)"
                        .to_string(),
                ));
            }
            let target = now + chrono::Duration::minutes(mins as i64);
            return Ok(ParsedSchedule {
                expr: None,
                display: format!("once in {mins} minute(s)"),
                next_run: target,
                once: true,
            });
        }
        return Err(err());
    }

    // 4. Interval: "every 30m" / "tiap 2 jam" / "30 menit" / "every 2 hours".
    let interval_start = tokens
        .iter()
        .position(|t| t == "every" || t == "tiap" || t == "setiap")
        .map(|p| p + 1)
        .unwrap_or(0);
    if let Some((mins, _)) = parse_duration(&tokens, interval_start) {
        if mins == 0 {
            return Err(PurrError::Config(
                "Interval under one minute is not supported (cron is minute-resolved)".to_string(),
            ));
        }
        let expr = interval_expr(mins)?;
        let next = expr.next_after(now).ok_or_else(err)?;
        return Ok(ParsedSchedule {
            expr: Some(expr.to_expr()),
            display: interval_display(mins),
            next_run: next,
            once: false,
        });
    }
    // A bare duration with no other words is also an interval.
    if tokens.len() <= 2 && parse_duration(&tokens, 0).is_some() {
        let (mins, _) = parse_duration(&tokens, 0).unwrap();
        if mins == 0 {
            return Err(PurrError::Config(
                "Interval under one minute is not supported (cron is minute-resolved)".to_string(),
            ));
        }
        let expr = interval_expr(mins)?;
        let next = expr.next_after(now).ok_or_else(err)?;
        return Ok(ParsedSchedule {
            expr: Some(expr.to_expr()),
            display: interval_display(mins),
            next_run: next,
            once: false,
        });
    }

    // 5. "tomorrow"/"besok" + time -> one-shot.
    if has("tomorrow") || has("besok") {
        let anchor = tokens
            .iter()
            .position(|t| t == "tomorrow" || t == "besok")
            .unwrap_or(0);
        // Clock may come after "jam" ("besok jam 8") or directly.
        for j in anchor + 1..tokens.len() {
            if tokens[j] == "jam" {
                if let Some(((h, m), _)) = read_clock(&tokens, j + 1) {
                    let target = next_day_at(now, h, m);
                    return Ok(ParsedSchedule {
                        expr: None,
                        display: format!("once tomorrow at {h:02}:{m:02}"),
                        next_run: target,
                        once: true,
                    });
                }
            } else if let Some(((h, m), _)) = read_clock(&tokens, j) {
                let target = next_day_at(now, h, m);
                return Ok(ParsedSchedule {
                    expr: None,
                    display: format!("once tomorrow at {h:02}:{m:02}"),
                    next_run: target,
                    once: true,
                });
            }
        }
        return Err(err());
    }

    // 6. Weekday list + time, or daily time.
    // Collect weekday tokens.
    let mut dows: Vec<u32> = Vec::new();
    for t in &tokens {
        if let Some((_, dow)) = WEEKDAYS.iter().find(|(n, _)| *n == t.as_str()) {
            if !dows.contains(dow) {
                dows.push(*dow);
            }
        }
    }
    if has("weekday") || (has("hari") && has("kerja")) {
        dows = vec![1, 2, 3, 4, 5];
    } else if has("weekend") || (has("akhir") && has("pekan")) {
        dows = vec![6, 0];
    }

    // Find the clock: after "jam"/"at", else the first clock-like token.
    let clock: Option<(u32, u32)> = {
        let mut found = None;
        for (i, t) in tokens.iter().enumerate() {
            if t == "jam" || t == "at" {
                if let Some((c, _)) = read_clock(&tokens, i + 1) {
                    found = Some(c);
                    break;
                }
            }
        }
        if found.is_none() {
            for i in 0..tokens.len() {
                if let Some((c, _)) = read_clock(&tokens, i) {
                    found = Some(c);
                    break;
                }
            }
        }
        found
    };

    // A time marker ("jam"/"at") with no parseable clock is an error, not a
    // silent default: "setiap hari jam 99" must not become 09:00.
    if clock.is_none() && tokens.iter().any(|t| t == "jam" || t == "at") {
        return Err(err());
    }

    if let Some((h, m)) = clock {
        if !dows.is_empty() {
            let expr = CronExpr {
                minutes: vec![m],
                hours: vec![h],
                dom: (1..=31).collect(),
                months: (1..=12).collect(),
                dow: dows.clone(),
            };
            let next = expr.next_after(now).ok_or_else(err)?;
            let names: Vec<&str> = dows.iter().map(|d| weekday_name(*d)).collect();
            return Ok(ParsedSchedule {
                expr: Some(expr.to_expr()),
                display: format!("{} at {:02}:{:02}", names.join(", "), h, m),
                next_run: next,
                once: false,
            });
        }
        // Daily at this time ("tiap pagi jam 8", "every day at 9").
        let expr = CronExpr {
            minutes: vec![m],
            hours: vec![h],
            dom: (1..=31).collect(),
            months: (1..=12).collect(),
            dow: vec![0, 1, 2, 3, 4, 5, 6],
        };
        let next = expr.next_after(now).ok_or_else(err)?;
        return Ok(ParsedSchedule {
            expr: Some(expr.to_expr()),
            display: format!("every day at {:02}:{:02}", h, m),
            next_run: next,
            once: false,
        });
    }

    // "every day" / "setiap hari" with no time defaults to 09:00.
    if (has("every") || has("tiap") || has("setiap")) && (has("day") || has("hari")) {
        let expr = CronExpr {
            minutes: vec![0],
            hours: vec![9],
            dom: (1..=31).collect(),
            months: (1..=12).collect(),
            dow: vec![0, 1, 2, 3, 4, 5, 6],
        };
        let next = expr.next_after(now).ok_or_else(err)?;
        return Ok(ParsedSchedule {
            expr: Some(expr.to_expr()),
            display: "every day at 09:00".to_string(),
            next_run: next,
            once: false,
        });
    }

    Err(err())
}

fn interval_expr(mins: u64) -> Result<CronExpr> {
    // Not every interval is expressible as a 5-field cron schedule: minute
    // steps must divide 60 evenly, hour steps must divide 24 evenly, and
    // multi-day steps do not repeat evenly across month boundaries
    // (e.g. `*/7` fires Nov 29 then Dec 1 — a 2-day gap). A stored expr
    // that claims a different period than the user asked for is worse than
    // a clear rejection, so unrepresentable intervals are errors.
    if mins < 60 {
        if !60u64.is_multiple_of(mins) {
            return Err(PurrError::Config(format!(
                "Interval {} cannot be expressed as a cron schedule (minutes must divide 60 evenly: 1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30)",
                interval_display(mins)
            )));
        }
        Ok(CronExpr {
            minutes: steps(0, 59, mins as u32),
            hours: (0..=23).collect(),
            dom: (1..=31).collect(),
            months: (1..=12).collect(),
            dow: vec![0, 1, 2, 3, 4, 5, 6],
        })
    } else if mins < 60 * 24 {
        let hours = mins / 60;
        if !mins.is_multiple_of(60) || !24u64.is_multiple_of(hours) {
            return Err(PurrError::Config(format!(
                "Interval {} cannot be expressed as a cron schedule (hours must divide 24 evenly: 1, 2, 3, 4, 6, 8, 12, 24 hours)",
                interval_display(mins)
            )));
        }
        Ok(CronExpr {
            minutes: vec![0],
            hours: steps(0, 23, hours as u32),
            dom: (1..=31).collect(),
            months: (1..=12).collect(),
            dow: vec![0, 1, 2, 3, 4, 5, 6],
        })
    } else {
        if mins != 60 * 24 {
            return Err(PurrError::Config(
                "Interval of more than one day cannot be expressed as a cron schedule (day steps do not repeat evenly across month boundaries); only daily (every 24 hours) is supported"
                    .to_string(),
            ));
        }
        Ok(CronExpr {
            minutes: vec![0],
            hours: vec![0],
            dom: (1..=31).collect(),
            months: (1..=12).collect(),
            dow: vec![0, 1, 2, 3, 4, 5, 6],
        })
    }
}

/// Human-readable form of an interval for the creation echo
/// ("every 2 hours", not "every 120 minute(s)").
fn interval_display(mins: u64) -> String {
    if mins < 60 {
        format!("every {mins} minute(s)")
    } else if mins < 60 * 24 {
        format!("every {} hour(s)", mins / 60)
    } else {
        format!("every {} day(s)", mins / (60 * 24))
    }
}

fn steps(lo: u32, hi: u32, step: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut v = lo;
    while v <= hi {
        out.push(v);
        v += step;
    }
    out
}

fn next_day_at(now: DateTime<Local>, hour: u32, minute: u32) -> DateTime<Local> {
    let date = now.date_naive() + chrono::Duration::days(1);
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap());
    Local.from_local_datetime(&naive).unwrap()
}

fn parse_iso_datetime(input: &str, _now: DateTime<Local>) -> Option<DateTime<Local>> {
    let cleaned = input.trim().replace('T', " ");
    // Full datetime with optional seconds.
    for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&cleaned, fmt) {
            return Local.from_local_datetime(&naive).single();
        }
    }
    // Date only -> 09:00 that day.
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&cleaned, "%Y-%m-%d") {
        let naive = date.and_hms_opt(9, 0, 0)?;
        return Local.from_local_datetime(&naive).single();
    }
    None
}

#[cfg(test)]
mod tests {
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
}
