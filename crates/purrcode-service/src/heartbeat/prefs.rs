// SPDX-License-Identifier: GPL-3.0-or-later

//! `PROACTIVE_PREFERENCES.md` (v0.3 F22, ADR-014 item 6).
//!
//! The user's own allow/deny list for proactive nudges, in the shape
//! Muse's teardown showed: sections "Tell me about" / "Never tell me
//! about" / "When" (time window) / "How". The file is optional — a
//! missing file means "no preferences yet" (everything allowed, no
//! window). A corrupt or unreadable file also falls back to defaults
//! with a warning: a preferences typo must never break the tick.
//!
//! Semantics (decided in ADR-014, implemented here):
//! - "Never tell me about" is a HARD filter: a task whose text
//!   contains one of those lines (case-insensitive substring) is
//!   skipped before Phase-1 ever sees it.
//! - "Tell me about" is advisory context for the Phase-1 decider
//!   (what the user cares about), never a gate — an empty list does
//!   NOT mean "tell me nothing".
//! - "When" is a time window ("09:00 to 21:30"); outside it the whole
//!   tick is skipped — nothing is checked and nothing is delivered.

use chrono::{DateTime, Local, NaiveTime};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProactivePrefs {
    pub tell_about: Vec<String>,
    pub never_about: Vec<String>,
    /// (start, end) local times. `None` = no window (always in).
    pub window: Option<(NaiveTime, NaiveTime)>,
}

/// File name inside `~/.purrcode/`.
pub const PREFS_FILE_NAME: &str = "PROACTIVE_PREFERENCES.md";

/// Parse the preferences file. Pure function (unit-tested). Section
/// headers are case-insensitive, with optional leading `#`s; bullet
/// lines (`- `, `* `) and plain lines under a section belong to it.
/// The "When" window is the first two `HH:MM` times found in that
/// section ("09:00 to 21:30", "09:00-21:30", "09:00–21:30").
pub fn parse_prefs(text: &str) -> ProactivePrefs {
    #[derive(PartialEq)]
    enum Section {
        Tell,
        Never,
        When,
        How,
        Other,
    }
    let mut prefs = ProactivePrefs::default();
    let mut section = Section::Other;
    let mut when_lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let header = line.trim_start_matches('#').trim().to_lowercase();
        match header.as_str() {
            "tell me about" => section = Section::Tell,
            "never tell me about" => section = Section::Never,
            "when" => section = Section::When,
            "how" => section = Section::How,
            _ => {
                let item = strip_bullet(line);
                match section {
                    Section::Tell => prefs.tell_about.push(item.to_string()),
                    Section::Never => prefs.never_about.push(item.to_string()),
                    Section::When => when_lines.push(item.to_string()),
                    Section::How | Section::Other => {}
                }
            }
        }
    }
    prefs.window = parse_window(&when_lines);
    prefs
}

fn strip_bullet(line: &str) -> &str {
    let mut item = line;
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = item.strip_prefix(marker) {
            item = rest.trim_start();
            break;
        }
    }
    item.trim()
}

/// The first two `HH:MM` times in the "When" lines form the window.
/// One time alone (or none) means no window.
fn parse_window(lines: &[String]) -> Option<(NaiveTime, NaiveTime)> {
    let mut times = Vec::new();
    for line in lines {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if !c.is_ascii_digit() {
                continue;
            }
            // Collect a run like "09:00": digits, ':', digits.
            let mut run = String::from(c);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() || next == ':' {
                    run.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(t) = NaiveTime::parse_from_str(&run, "%H:%M:%S")
                .or_else(|_| NaiveTime::parse_from_str(&run, "%H:%M"))
            {
                times.push(t);
            }
        }
    }
    if times.len() >= 2 {
        Some((times[0], times[1]))
    } else {
        None
    }
}

impl ProactivePrefs {
    /// Load the preferences from `~/.purrcode/`. A missing or
    /// unreadable file yields the defaults (no logging here — the
    /// caller logs the outcome once per tick).
    pub fn load(purrcode_dir: &Path) -> Self {
        let path = purrcode_dir.join(PREFS_FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(raw) => parse_prefs(&raw),
            Err(_) => Self::default(),
        }
    }

    /// True when the task text matches a "Never tell me about" line
    /// (case-insensitive substring). Deny wins: the decider never
    /// sees a denied task.
    pub fn is_denied(&self, task_text: &str) -> bool {
        let lower = task_text.to_lowercase();
        self.never_about
            .iter()
            .any(|n| !n.is_empty() && lower.contains(&n.to_lowercase()))
    }

    /// True when `now` is inside the window. No window = always in.
    /// An overnight window (start > end, e.g. 21:00–07:00) wraps
    /// around midnight.
    pub fn is_in_window(&self, now: DateTime<Local>) -> bool {
        match self.window {
            None => true,
            Some((start, end)) => {
                let t = now.time();
                if start <= end {
                    t >= start && t <= end
                } else {
                    t >= start || t <= end
                }
            }
        }
    }

    /// The "Tell me about" lines as one prompt line for the Phase-1
    /// decider. Empty when the user listed nothing.
    pub fn tell_context(&self) -> String {
        if self.tell_about.is_empty() {
            String::new()
        } else {
            format!(
                "The user asked to be told about: {}.",
                self.tell_about.join("; ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    const SAMPLE: &str = "\
# Tell me about
- build failures
- security alerts

# Never tell me about
- social media
- sports scores

# When
Messages may arrive from 09:00 to 21:30 local time.

# How
One brief line per item.
";

    #[test]
    fn test_parse_prefs_sections() {
        let prefs = parse_prefs(SAMPLE);
        assert_eq!(prefs.tell_about, vec!["build failures", "security alerts"]);
        assert_eq!(prefs.never_about, vec!["social media", "sports scores"]);
        assert_eq!(
            prefs.window,
            Some((
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(21, 30, 0).unwrap()
            ))
        );
    }

    #[test]
    fn test_parse_prefs_missing_file_is_defaults() {
        let prefs = parse_prefs("");
        assert!(prefs.tell_about.is_empty());
        assert!(prefs.never_about.is_empty());
        assert_eq!(prefs.window, None);
        assert!(prefs.is_in_window(Local::now()));
    }

    #[test]
    fn test_is_denied_substring_case_insensitive() {
        let prefs = parse_prefs(SAMPLE);
        assert!(prefs.is_denied("Summarize SOCIAL MEDIA mentions"));
        assert!(prefs.is_denied("Check the sports scores page"));
        assert!(!prefs.is_denied("Check the build status"));
    }

    #[test]
    fn test_parse_window_accepts_seconds() {
        // "09:00:30 to 21:30:00" must parse as the 09:00:30–21:30:00
        // window, not silently drop the whole window (v0.3 F22 fix,
        // build checker OBS-1).
        let lines = vec!["Messages may arrive from 09:00:30 to 21:30:00 local time.".to_string()];
        assert_eq!(
            parse_window(&lines),
            Some((
                NaiveTime::from_hms_opt(9, 0, 30).unwrap(),
                NaiveTime::from_hms_opt(21, 30, 0).unwrap(),
            ))
        );
    }

    #[test]
    fn test_is_in_window_day_and_overnight() {
        let day = ProactivePrefs {
            window: Some((
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(21, 30, 0).unwrap(),
            )),
            ..Default::default()
        };
        let at = |h: u32, m: u32| {
            Local::now()
                .date_naive()
                .and_hms_opt(h, m, 0)
                .unwrap()
                .and_local_timezone(Local)
                .unwrap()
        };
        assert!(day.is_in_window(at(9, 0)));
        assert!(day.is_in_window(at(12, 0)));
        assert!(day.is_in_window(at(21, 30)));
        assert!(!day.is_in_window(at(8, 59)));
        assert!(!day.is_in_window(at(22, 0)));

        let night = ProactivePrefs {
            window: Some((
                NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            )),
            ..Default::default()
        };
        assert!(night.is_in_window(at(23, 0)));
        assert!(night.is_in_window(at(2, 0)));
        assert!(!night.is_in_window(at(12, 0)));
    }
}
