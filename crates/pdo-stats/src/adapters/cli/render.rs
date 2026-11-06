//! The text of `pdo stats`: the figures each tab of the surface shows, worked
//! out as the tab works them out, one fact a line.

use serde_json::Value;

fn sum(rows: &Value, field: &str) -> u64 {
    rows.as_array()
        .into_iter()
        .flatten()
        .map(|row| row[field].as_u64().unwrap_or(0))
        .sum()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// A cost as the surface writes it: `$` and 4 decimals under a dollar, 2
/// above; `~` when estimated, `†` when partial; `—` when unknown.
pub(super) fn cost_amount(usd: &Value, partial: &Value, estimated: &Value) -> String {
    let Some(usd) = usd.as_f64() else {
        return "—".to_string();
    };
    let precision = if usd < 1.0 { 4 } else { 2 };
    let amount = format!("${usd:.precision$}");
    format!(
        "{}{amount}{}",
        if estimated == true { "~" } else { "" },
        if partial == true { "†" } else { "" }
    )
}

fn cost_of(row: &Value, field: &str) -> String {
    cost_amount(&row[field], &row["partial"], &row["estimated"])
}

/// The Runs tab: its four figures, then one line per bucket.
pub(super) fn runs(overview: &Value, completed_only: bool) -> String {
    let runs = sum(&overview["runs"], "count");
    let failed = sum(&overview["errors"], "count");
    let busiest = overview["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row["count"].as_u64().unwrap_or(0))
        .max()
        .unwrap_or(0);
    let rate = if runs == 0 {
        "—".to_string()
    } else {
        format!(
            "{}%",
            ((failed as f64 / runs as f64) * 100.0).round().min(100.0)
        )
    };
    let failed_label = if completed_only {
        "errors (completed only)"
    } else {
        "failed"
    };
    let mut out =
        format!("runs {runs} · {failed_label} {failed} · failure rate {rate} · busiest {busiest}\n");
    let count_at = |series: &Value, bucket: &str| {
        series
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["bucket"] == bucket)
            .and_then(|row| row["count"].as_u64())
            .unwrap_or(0)
    };
    for bucket in overview["buckets"].as_array().into_iter().flatten() {
        let bucket = text(bucket);
        out.push_str(&format!(
            "{bucket}\t{}\t{}\n",
            count_at(&overview["runs"], bucket),
            count_at(&overview["errors"], bucket)
        ));
    }
    out
}

/// The Sessions tab: the node sessions, by harness, then by Pipeline (most
/// first).
pub(super) fn sessions(overview: &Value) -> String {
    let pipelines = &overview["sessions_by_pipeline"];
    let mut out = format!("sessions {}\n", sum(pipelines, "executions"));
    for harness in overview["session_harnesses"].as_array().into_iter().flatten() {
        let executions: u64 = pipelines
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|row| row["harnesses"].as_array().cloned().unwrap_or_default())
            .filter(|item| item["harness"] == *harness)
            .map(|item| item["executions"].as_u64().unwrap_or(0))
            .sum();
        out.push_str(&format!("harness\t{}\t{executions}\n", text(harness)));
    }
    let mut rows: Vec<&Value> = pipelines.as_array().into_iter().flatten().collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row["executions"].as_u64().unwrap_or(0)));
    for row in rows {
        out.push_str(&format!(
            "pipeline\t{}\t{}\n",
            text(&row["name"]),
            row["executions"]
        ));
    }
    out
}

/// The Triggers tab: its three figures, then the fires by Pipeline.
pub(super) fn triggers(overview: &Value) -> String {
    let kpi = &overview["triggers_created_runs"];
    let fires = &overview["fires_by_pipeline"];
    let mut out = format!(
        "fires that created a run {} · triggers fired {} of {} enabled · fires {}\n",
        kpi["fired"],
        kpi["distinct_triggers"],
        kpi["enabled_triggers"],
        sum(fires, "count")
    );
    for fire in fires.as_array().into_iter().flatten() {
        out.push_str(&format!(
            "pipeline\t{}\t{}\n",
            text(&fire["name"]),
            fire["count"]
        ));
    }
    out
}

/// The Cost tab: the total and its median per Run, by harness, then one line
/// per Pipeline, Projet and model.
pub(super) fn cost(cost: &Value) -> String {
    let total = &cost["total"];
    let mut out = format!(
        "total {} · median {} per Run · {} execution(s), {} without computable cost\n",
        cost_of(total, "usd"),
        cost_of(total, "median_usd"),
        total["executions"],
        total["unknown"]
    );
    for harness in total["harnesses"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "harness\t{}\t{}\t{} median\n",
            text(&harness["harness"]),
            cost_of(harness, "usd"),
            cost_of(harness, "median_usd")
        ));
    }
    for (axis, key) in [
        ("pipeline", "by_pipeline"),
        ("project", "by_project"),
        ("model", "by_model"),
    ] {
        for row in cost[key].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "{axis}\t{}\t{}\t{} median\n",
                text(&row["name"]),
                cost_of(row, "usd"),
                cost_of(row, "median_usd")
            ));
        }
    }
    out
}

/// A duration in milliseconds as the surface writes it: `42s`, `3m05s`.
fn duration(ms: f64) -> String {
    let seconds = (ms / 1000.0).round() as u64;
    let minutes = seconds / 60;
    if minutes > 0 {
        format!("{minutes}m{:02}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

/// A context size as the surface writes it: `850`, `42k`.
fn context(tokens: f64) -> String {
    if tokens >= 1000.0 {
        format!("{}k", (tokens / 1000.0).round())
    } else {
        format!("{}", tokens.round())
    }
}

fn median(distribution: &Value) -> Option<f64> {
    distribution["stats"]["median"].as_f64()
}

/// One harness's three headline figures.
fn performance_line(item: &Value) -> String {
    let steered = &item["steered"];
    let steered = match steered["readable"].as_u64() {
        Some(readable) if readable > 0 => format!(
            "{} %",
            ((steered["steered"].as_u64().unwrap_or(0) as f64 / readable as f64) * 100.0).round()
        ),
        _ => "—".to_string(),
    };
    format!(
        "{}\t{}\t{}\t{steered}",
        text(&item["harness"]),
        median(&item["context"]).map_or("—".to_string(), context),
        median(&item["duration"]).map_or("—".to_string(), duration),
    )
}

/// The Performance tab: by harness, median peak context, median duration and
/// steered share; then the same per Pipeline.
pub(super) fn performance(performance: &Value) -> String {
    let mut out = String::from("harness\tmedian peak context\tmedian duration\tsteered\n");
    for item in performance["total"]["harnesses"]
        .as_array()
        .into_iter()
        .flatten()
    {
        out.push_str(&format!("{}\n", performance_line(item)));
    }
    for row in performance["by_pipeline"].as_array().into_iter().flatten() {
        for item in row["harnesses"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "pipeline\t{}\t{}\n",
                text(&row["name"]),
                performance_line(item)
            ));
        }
    }
    out
}

/// The absorptions: one line per member, under its absorbent.
pub(super) fn absorptions(list: &Value) -> String {
    let mut out = String::new();
    for absorption in list["absorptions"].as_array().into_iter().flatten() {
        let scope = match absorption["scope_name"].as_str().filter(|s| !s.is_empty()) {
            Some(name) => format!(" ({name})"),
            None => String::new(),
        };
        for member in absorption["members"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "{}{scope}\t{}\t<- {}\n",
                text(&absorption["dimension"]),
                text(&absorption["absorbent"]["name"]),
                text(&member["name"])
            ));
        }
    }
    if out.is_empty() {
        out.push_str("no absorption\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_runs_tab_counts_runs_failures_and_the_busiest_bucket() {
        let overview = json!({
            "buckets": ["2026-11-05", "2026-11-06"],
            "runs": [{ "bucket": "2026-11-05", "count": 3 }, { "bucket": "2026-11-06", "count": 1 }],
            "errors": [{ "bucket": "2026-11-05", "count": 1 }],
        });
        assert_eq!(
            runs(&overview, false),
            "runs 4 · failed 1 · failure rate 25% · busiest 3\n\
             2026-11-05\t3\t1\n2026-11-06\t1\t0\n"
        );
        assert_eq!(
            runs(&json!({ "buckets": [], "runs": [], "errors": [] }), true),
            "runs 0 · errors (completed only) 0 · failure rate — · busiest 0\n"
        );
    }

    #[test]
    fn the_sessions_tab_sums_by_harness_and_sorts_pipelines() {
        let overview = json!({
            "session_harnesses": ["claude", "pi"],
            "sessions_by_pipeline": [
                { "name": "Ship", "executions": 2, "harnesses": [{ "harness": "claude", "executions": 2 }] },
                { "name": "Fix", "executions": 5, "harnesses": [
                    { "harness": "claude", "executions": 1 }, { "harness": "pi", "executions": 4 }] },
            ],
        });
        assert_eq!(
            sessions(&overview),
            "sessions 7\nharness\tclaude\t3\nharness\tpi\t4\npipeline\tFix\t5\npipeline\tShip\t2\n"
        );
    }

    #[test]
    fn the_triggers_tab_says_fires_and_triggers_fired() {
        let overview = json!({
            "triggers_created_runs": { "fired": 3, "distinct_triggers": 1, "enabled_triggers": 2 },
            "fires_by_pipeline": [{ "pipeline_id": "p", "name": "Nightly", "count": 4 }],
        });
        assert_eq!(
            triggers(&overview),
            "fires that created a run 3 · triggers fired 1 of 2 enabled · fires 4\npipeline\tNightly\t4\n"
        );
    }

    #[test]
    fn a_cost_is_written_as_the_surface_writes_it() {
        assert_eq!(cost_amount(&json!(0.5), &json!(false), &json!(false)), "$0.5000");
        assert_eq!(cost_amount(&json!(12.345), &json!(true), &json!(true)), "~$12.35†");
        assert_eq!(cost_amount(&Value::Null, &json!(false), &json!(false)), "—");
    }

    #[test]
    fn durations_and_contexts_read_as_on_the_surface() {
        assert_eq!(duration(42_400.0), "42s");
        assert_eq!(duration(185_000.0), "3m05s");
        assert_eq!(context(850.0), "850");
        assert_eq!(context(42_499.0), "42k");
    }

    #[test]
    fn the_absorptions_list_each_member_under_its_absorbent() {
        let list = json!({ "absorptions": [{
            "dimension": "node", "scope": "ship", "scope_name": "Ship",
            "absorbent": { "key": "build", "name": "Build" },
            "members": [{ "key": "build-old", "name": "Build (old)", "origin": "x", "created_at": "t" }],
        }]});
        assert_eq!(absorptions(&list), "node (Ship)\tBuild\t<- Build (old)\n");
        assert_eq!(absorptions(&json!({ "absorptions": [] })), "no absorption\n");
    }
}
