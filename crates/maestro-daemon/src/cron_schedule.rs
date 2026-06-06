//! Pure cron scheduling: parse a 5-field cron expression and compute the next
//! fire time strictly after a given instant, in the local timezone.
//!
//! Scope (per CONTEXT.md → *Trigger* / ADR-0012): standard 5-field Unix cron
//! (`minute hour day-of-month month day-of-week`), minute resolution, no
//! seconds and no year field. No I/O — the public entry point takes a `now`
//! and returns the next matching local datetime.

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike};

/// A parsed 5-field cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days_of_month: Vec<u32>,
    months: Vec<u32>,
    days_of_week: Vec<u32>,
    /// Whether the original day-of-month field was restricted (not `*`).
    dom_restricted: bool,
    /// Whether the original day-of-week field was restricted (not `*`).
    dow_restricted: bool,
}

/// Error parsing a cron expression.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CronError {
    #[error("cron expression must have exactly 5 fields, got {0}")]
    FieldCount(usize),
    #[error("invalid value in cron field '{field}': {detail}")]
    Field { field: String, detail: String },
}

impl CronSchedule {
    /// Parse a standard 5-field cron expression
    /// (`minute hour day-of-month month day-of-week`).
    pub fn parse(expr: &str) -> Result<Self, CronError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronError::FieldCount(fields.len()));
        }
        let minutes = parse_field(fields[0], 0, 59)?;
        let hours = parse_field(fields[1], 0, 23)?;
        let days_of_month = parse_field(fields[2], 1, 31)?;
        let months = parse_field(fields[3], 1, 12)?;
        let days_of_week = parse_dow(fields[4])?;
        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    /// The next instant strictly after `now` whose wall-clock fields match this
    /// schedule, in the same timezone as `now`. Minute resolution: seconds and
    /// nanoseconds of the result are always zero.
    ///
    /// Returns `None` only if no match is found within a bounded search horizon
    /// (e.g. an impossible expression like Feb 30) — callers treat that as
    /// "never fires".
    pub fn next_fire_after<Tz: TimeZone>(&self, now: DateTime<Tz>) -> Option<DateTime<Tz>> {
        // Start from the next whole minute strictly after `now`.
        let mut candidate = now
            .with_second(0)?
            .with_nanosecond(0)?
            .checked_add_signed(Duration::minutes(1))?;

        // Search horizon: 4 years of minutes is enough to find any match a
        // standard cron can express, and bounds impossible expressions.
        let max_minutes = 4 * 366 * 24 * 60;
        for _ in 0..max_minutes {
            if self.matches(&candidate) {
                return Some(candidate);
            }
            candidate = candidate.checked_add_signed(Duration::minutes(1))?;
        }
        None
    }

    fn matches<Tz: TimeZone>(&self, dt: &DateTime<Tz>) -> bool {
        if !self.minutes.contains(&dt.minute()) {
            return false;
        }
        if !self.hours.contains(&dt.hour()) {
            return false;
        }
        if !self.months.contains(&dt.month()) {
            return false;
        }
        // Day matching follows Vixie cron: when *both* day-of-month and
        // day-of-week are restricted, a match on *either* fires. When only one
        // is restricted, that one must match. When neither is restricted, any
        // day matches.
        let dom_match = self.days_of_month.contains(&dt.day());
        // chrono weekday: Mon=0..Sun=6; cron: Sun=0..Sat=6.
        let cron_dow = dt.weekday().num_days_from_sunday();
        let dow_match = self.days_of_week.contains(&cron_dow);
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom_match || dow_match,
            (true, false) => dom_match,
            (false, true) => dow_match,
            (false, false) => true,
        }
    }
}

/// Parse one numeric cron field (`*`, `a`, `a-b`, `*/n`, `a-b/n`, and
/// comma-separated lists thereof) into the sorted set of matching values.
fn parse_field(field: &str, min: u32, max: u32) -> Result<Vec<u32>, CronError> {
    let mut values = std::collections::BTreeSet::new();
    for part in field.split(',') {
        let (range_part, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s.parse().map_err(|_| CronError::Field {
                    field: field.to_string(),
                    detail: format!("invalid step '{s}'"),
                })?;
                if step == 0 {
                    return Err(CronError::Field {
                        field: field.to_string(),
                        detail: "step must be > 0".to_string(),
                    });
                }
                (r, step)
            }
            None => (part, 1),
        };

        let (lo, hi) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            (parse_num(a, field)?, parse_num(b, field)?)
        } else {
            let v = parse_num(range_part, field)?;
            (v, v)
        };

        if lo < min || hi > max || lo > hi {
            return Err(CronError::Field {
                field: field.to_string(),
                detail: format!("value out of range {min}-{max}: {range_part}"),
            });
        }

        let mut v = lo;
        while v <= hi {
            values.insert(v);
            v += step;
        }
    }
    Ok(values.into_iter().collect())
}

/// Day-of-week field: like `parse_field` over 0-7, but `7` is normalised to `0`
/// (both mean Sunday).
fn parse_dow(field: &str) -> Result<Vec<u32>, CronError> {
    let raw = parse_field(field, 0, 7)?;
    let mut values: std::collections::BTreeSet<u32> = raw
        .into_iter()
        .map(|v| if v == 7 { 0 } else { v })
        .collect();
    // `*` already covered 0-6; if 7 was present it folds into 0.
    if values.is_empty() {
        values.insert(0);
    }
    Ok(values.into_iter().collect())
}

fn parse_num(s: &str, field: &str) -> Result<u32, CronError> {
    s.parse().map_err(|_| CronError::Field {
        field: field.to_string(),
        detail: format!("not a number: '{s}'"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// `every minute` (`* * * * *`) advances to the next whole minute strictly
    /// after `now`, dropping sub-minute components.
    #[test]
    fn every_minute_advances_to_next_minute() {
        let sched = CronSchedule::parse("* * * * *").expect("valid cron");
        let now = chrono::FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 6, 6, 10, 30, 45)
            .unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        let expected = chrono::FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 6, 6, 10, 31, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    /// `daily at 09:00` (`0 9 * * *`) rolls forward to tomorrow when `now` is
    /// already past today's slot.
    #[test]
    fn daily_at_nine_rolls_to_next_day() {
        let sched = CronSchedule::parse("0 9 * * *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 10, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 7, 9, 0, 0).unwrap());
    }

    /// When `now` is before today's daily slot, it fires today.
    #[test]
    fn daily_at_nine_fires_today_when_before_slot() {
        let sched = CronSchedule::parse("0 9 * * *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 8, 59, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 6, 9, 0, 0).unwrap());
    }

    /// `every 15 min` (`*/15 * * * *`) lands on the next quarter-hour boundary.
    #[test]
    fn every_fifteen_minutes_lands_on_next_quarter() {
        let sched = CronSchedule::parse("*/15 * * * *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 10, 17, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 6, 10, 30, 0).unwrap());
    }

    /// A `*/15` slot at :45 rolls into the next hour at :00.
    #[test]
    fn every_fifteen_minutes_rolls_into_next_hour() {
        let sched = CronSchedule::parse("*/15 * * * *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 10, 45, 30).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 6, 11, 0, 0).unwrap());
    }

    /// Day-of-week field (`0 9 * * 1` = Mondays at 09:00) skips to the next
    /// matching weekday. 2026-06-06 is a Saturday; next Monday is 2026-06-08.
    #[test]
    fn day_of_week_skips_to_next_matching_weekday() {
        let sched = CronSchedule::parse("0 9 * * 1").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 12, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 8, 9, 0, 0).unwrap());
    }

    /// Sunday is both `0` and `7`.
    #[test]
    fn day_of_week_seven_is_sunday() {
        let sched = CronSchedule::parse("0 0 * * 7").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        // Sat 2026-06-06 -> next Sunday 2026-06-07 00:00.
        let now = tz.with_ymd_and_hms(2026, 6, 6, 12, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 7, 0, 0, 0).unwrap());
    }

    /// Crossing a year boundary: `0 0 1 1 *` (midnight Jan 1) from December.
    #[test]
    fn rolls_across_year_boundary() {
        let sched = CronSchedule::parse("0 0 1 1 *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 12, 31, 23, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap());
    }

    /// `now` exactly on a matching minute fires the *next* slot, never `now`
    /// itself (strictly-after contract).
    #[test]
    fn strictly_after_skips_the_current_matching_minute() {
        let sched = CronSchedule::parse("* * * * *").expect("valid cron");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 6, 6, 10, 30, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next, tz.with_ymd_and_hms(2026, 6, 6, 10, 31, 0).unwrap());
    }

    /// An impossible expression (Feb 30) never fires: `None` within the horizon.
    #[test]
    fn impossible_expression_never_fires() {
        let sched = CronSchedule::parse("0 0 30 2 *").expect("parses fine");
        let tz = chrono::FixedOffset::east_opt(0).unwrap();
        let now = tz.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(sched.next_fire_after(now), None);
    }

    /// Spring-forward DST gap (Europe/Paris, 2026-03-29: 02:00 -> 03:00). A
    /// `30 2 * * *` slot does not exist that night; the schedule fires the next
    /// valid occurrence (the next day at 02:30), never inside the gap.
    #[test]
    fn dst_spring_forward_skips_nonexistent_local_time() {
        use chrono_tz::Europe::Paris;
        let sched = CronSchedule::parse("30 2 * * *").expect("valid cron");
        // Just before the gap night, in Paris local time.
        let now = Paris.with_ymd_and_hms(2026, 3, 29, 1, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        // 2026-03-29 02:30 Paris does not exist (clocks jump 02:00->03:00), so
        // the next 02:30 is the following day.
        assert_eq!(next, Paris.with_ymd_and_hms(2026, 3, 30, 2, 30, 0).unwrap());
    }

    /// Fall-back DST (Europe/Paris, 2026-10-25: 03:00 -> 02:00, 02:30 occurs
    /// twice). The schedule fires at 02:30; we accept the earlier occurrence
    /// and never loop forever on the ambiguous hour.
    #[test]
    fn dst_fall_back_fires_on_ambiguous_local_time() {
        use chrono_tz::Europe::Paris;
        let sched = CronSchedule::parse("30 2 * * *").expect("valid cron");
        let now = Paris.with_ymd_and_hms(2026, 10, 25, 1, 0, 0).unwrap();
        let next = sched.next_fire_after(now).expect("a next fire exists");
        assert_eq!(next.hour(), 2);
        assert_eq!(next.minute(), 30);
        assert_eq!(next.day(), 25);
    }

    #[test]
    fn rejects_wrong_field_count() {
        assert_eq!(
            CronSchedule::parse("* * * *"),
            Err(CronError::FieldCount(4))
        );
        assert_eq!(
            CronSchedule::parse("* * * * * *"),
            Err(CronError::FieldCount(6))
        );
    }

    #[test]
    fn rejects_out_of_range_values() {
        assert!(matches!(
            CronSchedule::parse("60 * * * *"),
            Err(CronError::Field { .. })
        ));
        assert!(matches!(
            CronSchedule::parse("* 24 * * *"),
            Err(CronError::Field { .. })
        ));
        assert!(matches!(
            CronSchedule::parse("* * * 13 *"),
            Err(CronError::Field { .. })
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(CronSchedule::parse("hello world foo bar baz").is_err());
        assert!(CronSchedule::parse("* * * * */0").is_err());
    }
}
