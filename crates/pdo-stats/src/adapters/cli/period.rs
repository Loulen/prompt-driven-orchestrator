//! The period of a `pdo stats` read: the surface's presets, computed as it
//! computes them, so a preset reads the window the tab reads.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use clap::ValueEnum;

/// The presets of the Stats surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Preset {
    #[value(name = "7d")]
    Days7,
    #[value(name = "30d")]
    Days30,
    #[value(name = "90d")]
    Days90,
    All,
}

/// An `[from, to)` window and its bucket, as the routes read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub from: String,
    pub to: String,
    pub bucket: String,
}

/// Midnight UTC of `at`'s day, `days` days later (earlier when negative).
fn day_start(at: DateTime<Utc>, days: i64) -> String {
    let day = at.date_naive() + Duration::days(days);
    iso(day)
}

fn iso(day: NaiveDate) -> String {
    day.and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_utc()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Preset {
    /// The window of the preset at `now`: whole UTC days, today included,
    /// `to` the start of tomorrow; days up to 30 days, then weeks, then
    /// months for all time.
    pub fn window(self, now: DateTime<Utc>) -> Window {
        let to = day_start(now, 1);
        let (from, bucket) = match self {
            Preset::Days7 => (day_start(now, -6), "day"),
            Preset::Days30 => (day_start(now, -29), "day"),
            Preset::Days90 => (day_start(now, -89), "week"),
            Preset::All => ("1970-01-01T00:00:00.000Z".to_string(), "month"),
        };
        Window {
            from,
            to,
            bucket: bucket.to_string(),
        }
    }
}

impl Window {
    /// A window of the operator's: a day (`2026-11-01`) is its midnight UTC,
    /// anything else is passed as RFC 3339. Bucketed by day.
    pub fn between(from: &str, to: &str) -> Result<Self> {
        Ok(Window {
            from: instant(from)?,
            to: instant(to)?,
            bucket: "day".to_string(),
        })
    }
}

fn instant(raw: &str) -> Result<String> {
    if let Ok(day) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Ok(iso(day));
    }
    DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("{raw:?} is neither YYYY-MM-DD nor RFC 3339"))?;
    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-06T02:30:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn seven_days_are_today_and_the_six_before_by_day() {
        assert_eq!(
            Preset::Days7.window(now()),
            Window {
                from: "2026-10-31T00:00:00.000Z".into(),
                to: "2026-11-07T00:00:00.000Z".into(),
                bucket: "day".into(),
            }
        );
    }

    #[test]
    fn longer_presets_bucket_coarser() {
        assert_eq!(
            Preset::Days30.window(now()).from,
            "2026-10-08T00:00:00.000Z"
        );
        assert_eq!(Preset::Days90.window(now()).bucket, "week");
        let all = Preset::All.window(now());
        assert_eq!(all.from, "1970-01-01T00:00:00.000Z");
        assert_eq!(all.bucket, "month");
    }

    #[test]
    fn a_window_of_one_s_own_takes_days_or_instants() {
        let window = Window::between("2026-11-01", "2026-11-02T12:00:00Z").unwrap();
        assert_eq!(window.from, "2026-11-01T00:00:00.000Z");
        assert_eq!(window.to, "2026-11-02T12:00:00Z");
        assert!(Window::between("yesterday", "2026-11-02").is_err());
    }
}
