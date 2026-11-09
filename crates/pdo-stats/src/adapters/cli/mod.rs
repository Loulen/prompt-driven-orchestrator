//! `pdo stats`: the Stats surface of the running daemon, tab for tab, over
//! `/stats/*`. The same period presets and cohort defaults as the surface, so
//! `pdo stats runs --period 7d` reads the figures the Runs tab shows. A thin
//! client: the folds stay behind the route; the text sums a tab's headline
//! figures (totals, failure rate, busiest bucket) as the tab does
//! (`render.rs`), and `--json` prints the route's answer as it is.

mod period;
mod render;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use pdo_platform::adapters::cli::daemon::{print_json, Daemon};
use serde_json::{json, Value};

pub use period::Preset;
use period::Window;

#[derive(Subcommand, Debug)]
pub enum StatsAction {
    /// The Runs tab: Runs, failed, failure rate, busiest bucket, then one
    /// line per bucket (bucket, runs, failed).
    Runs(Filters),
    /// The Sessions tab: node sessions, by harness, then by Pipeline.
    Sessions(Filters),
    /// The Triggers tab: fires that created a Run, Triggers fired of the
    /// enabled ones, fires, then fires by Pipeline.
    Triggers(Filters),
    /// The Cost tab: total and median, by harness, then by Pipeline, Projet
    /// and model.
    Cost(Filters),
    /// The Performance tab: median peak context, median duration and steered
    /// share by harness, then by Pipeline and for the Infrastructure.
    Performance {
        #[command(flatten)]
        filters: Filters,
        /// Which duration: the tab's Total / Active / Waiting toggle (it opens
        /// on Active).
        #[arg(long, value_enum, default_value_t = DurationMode::Active)]
        duration: DurationMode,
        /// Read the transcripts again instead of the memo (the tab's Refresh).
        #[arg(long)]
        refresh: bool,
    },
    /// The absorptions: rows Stats counts under another (Combine).
    Absorptions {
        #[arg(long)]
        json: bool,
    },
    /// Combine rows: count the members under the absorbent from now on.
    Combine {
        /// What the rows are.
        #[arg(long, value_enum)]
        dimension: Dimension,
        /// The Pipeline row a Node absorption is scoped to (`<key>[=<name>]`).
        #[arg(long, value_name = "KEY[=NAME]")]
        scope: Option<String>,
        /// The row the others are counted under (`<key>[=<name>]`).
        #[arg(long, value_name = "KEY[=NAME]")]
        into: String,
        /// A row counted under it (repeatable, `<key>[=<name>]`).
        #[arg(long = "member", value_name = "KEY[=NAME]", required = true)]
        members: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Count a member on its own row again.
    Uncombine {
        #[arg(long, value_enum)]
        dimension: Dimension,
        /// The Pipeline key a Node absorption is scoped to.
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        member: String,
        #[arg(long)]
        json: bool,
    },
}

/// The filters every tab shares.
#[derive(Args, Debug, Clone)]
pub struct Filters {
    /// The period, as the surface's presets: the last 7, 30 or 90 days
    /// (today included), or all time. 30 days by default, as Stats opens.
    #[arg(long, value_enum, default_value_t = Preset::Days30)]
    pub period: Preset,
    /// A window of your own instead (`YYYY-MM-DD` or RFC 3339), `--to`
    /// excluded; with `--bucket`.
    #[arg(long, requires = "to")]
    pub from: Option<String>,
    #[arg(long, requires = "from")]
    pub to: Option<String>,
    /// `day`, `week` or `month`; the preset's by default.
    #[arg(long)]
    pub bucket: Option<String>,
    /// Count only the Runs that completed (the tab's « completed runs only »).
    #[arg(long, conflicts_with = "every_run")]
    pub completed_only: bool,
    /// Count every Run, whatever its outcome.
    #[arg(long)]
    pub every_run: bool,
    /// Read the rows as the log wrote them, without the absorptions.
    #[arg(long)]
    pub uncombined: bool,
    #[arg(long)]
    pub json: bool,
}

/// The duration the Performance tab reads: total, active (without the
/// waits), or the waits alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DurationMode {
    Total,
    Active,
    Waiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Dimension {
    Pipeline,
    Node,
    Model,
    Effort,
    Couple,
}

impl Dimension {
    fn wire(self) -> &'static str {
        match self {
            Dimension::Pipeline => "pipeline",
            Dimension::Node => "node",
            Dimension::Model => "model",
            Dimension::Effort => "effort",
            Dimension::Couple => "couple",
        }
    }
}

impl Filters {
    /// The cohort of the tab: the flags', else the tab's default (only
    /// Performance opens on completed Runs).
    fn completed_only(&self, tab_default: bool) -> bool {
        if self.completed_only {
            true
        } else if self.every_run {
            false
        } else {
            tab_default
        }
    }

    fn window(&self) -> Result<Window> {
        let mut window = match (&self.from, &self.to) {
            (Some(from), Some(to)) => Window::between(from, to)?,
            _ => self.period.window(chrono::Utc::now()),
        };
        if let Some(bucket) = &self.bucket {
            window.bucket = bucket.clone();
        }
        Ok(window)
    }

    /// The query string of an `/stats/*` read.
    fn query(&self, tab_default: bool, bucket: bool) -> Result<String> {
        let window = self.window()?;
        let mut query = format!(
            "from={}&to={}&completed_only={}&uncombined={}",
            encode(&window.from),
            encode(&window.to),
            self.completed_only(tab_default),
            self.uncombined
        );
        if bucket {
            query.push_str(&format!("&bucket={}", encode(&window.bucket)));
        }
        Ok(query)
    }
}

pub fn run_stats(action: StatsAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        StatsAction::Runs(f) => {
            let overview = overview(&daemon, &f)?;
            out(&overview, f.json, || {
                render::runs(&overview, f.completed_only(false))
            });
        }
        StatsAction::Sessions(f) => {
            let overview = overview(&daemon, &f)?;
            out(&overview, f.json, || render::sessions(&overview));
        }
        StatsAction::Triggers(f) => {
            let overview = overview(&daemon, &f)?;
            out(&overview, f.json, || render::triggers(&overview));
        }
        StatsAction::Cost(f) => {
            let query = f.query(false, true)?;
            let cost = daemon.get(&format!("/stats/cost?{query}"))?.ok()?.json()?;
            out(&cost, f.json, || render::cost(&cost));
        }
        StatsAction::Performance {
            filters,
            duration,
            refresh,
        } => {
            let query = filters.query(true, false)?;
            let performance = daemon
                .get(&format!("/stats/performance?{query}&refresh={refresh}"))?
                .ok()?
                .json()?;
            out(&performance, filters.json, || {
                render::performance(&performance, duration)
            });
        }
        StatsAction::Absorptions { json } => {
            let list = daemon.get("/stats/absorptions")?.ok()?.json()?;
            out(&list, json, || render::absorptions(&list));
        }
        StatsAction::Combine {
            dimension,
            scope,
            into,
            members,
            json,
        } => {
            let (scope_key, scope_name) = scope.as_deref().map(named).unwrap_or_default();
            let row = |raw: &str| {
                let (key, name) = named(raw);
                let mut row = json!({ "key": key, "name": name });
                if !scope_key.is_empty() {
                    row["scope"] = json!(scope_key);
                }
                row
            };
            let body = json!({
                "dimension": dimension.wire(),
                "scope": scope_key,
                "scope_name": scope_name,
                "absorbent": row(&into),
                "members": members.iter().map(|m| row(m)).collect::<Vec<_>>(),
            });
            let list = daemon
                .post("/stats/absorptions", Some(&body))?
                .ok()?
                .json()?;
            out(&list, json, || render::absorptions(&list));
        }
        StatsAction::Uncombine {
            dimension,
            scope,
            member,
            json,
        } => {
            let list = daemon
                .delete(
                    &format!(
                        "/stats/absorptions/member?dimension={}&scope={}&member={}",
                        dimension.wire(),
                        encode(scope.as_deref().unwrap_or("")),
                        encode(&member)
                    ),
                    None,
                )?
                .ok()?
                .json()?;
            out(&list, json, || render::absorptions(&list));
        }
    }
    Ok(())
}

fn overview(daemon: &Daemon, filters: &Filters) -> Result<Value> {
    let query = filters.query(false, true)?;
    daemon
        .get(&format!("/stats/overview?{query}"))?
        .ok()?
        .json()
}

fn out(value: &Value, json: bool, text: impl FnOnce() -> String) {
    if json {
        print_json(value);
    } else {
        print!("{}", text());
    }
}

/// `<key>[=<name>]`: the name is the key when none is given.
fn named(raw: &str) -> (String, String) {
    match raw.split_once('=') {
        Some((key, name)) => (key.to_string(), name.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}

/// A query-string value, percent-encoded outside the unreserved set.
fn encode(raw: &str) -> String {
    pdo_platform::adapters::cli::daemon::segment(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_a_key_and_a_name_the_key_by_default() {
        assert_eq!(named("ship"), ("ship".into(), "ship".into()));
        assert_eq!(named("ship=Ship it"), ("ship".into(), "Ship it".into()));
    }

    /// The CLI reads the period the surface opens on (CORE-231): Stats opens
    /// on 30 days, so `pdo stats` without `--period` shows the same figures.
    #[test]
    fn the_default_period_is_the_one_stats_opens_on() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            filters: Filters,
        }
        let cli = <Cli as clap::Parser>::parse_from(["pdo"]);
        assert_eq!(cli.filters.period, Preset::Days30);
    }

    #[test]
    fn only_performance_opens_on_completed_runs() {
        let filters = |completed_only, every_run| Filters {
            period: Preset::Days7,
            from: None,
            to: None,
            bucket: None,
            completed_only,
            every_run,
            uncombined: false,
            json: false,
        };
        assert!(!filters(false, false).completed_only(false));
        assert!(filters(false, false).completed_only(true));
        assert!(filters(true, false).completed_only(false));
        assert!(!filters(false, true).completed_only(true));
    }
}
