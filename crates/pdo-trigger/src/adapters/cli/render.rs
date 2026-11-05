//! What `pdo trigger` prints without `--json`.

use pdo_platform::adapters::cli::daemon::print_json;
use serde_json::Value;

pub(super) fn state(trigger: &Value) -> &'static str {
    if !trigger["suspension"].is_null() {
        "suspended"
    } else if trigger["enabled"] == true {
        "enabled"
    } else {
        "paused"
    }
}

pub(super) fn done(trigger: &Value, json: bool, verb: &str) {
    if json {
        print_json(trigger);
    } else {
        println!(
            "{}\t{} {verb}",
            trigger["id"].as_str().unwrap_or_default(),
            trigger["name"].as_str().unwrap_or_default()
        );
    }
}

pub(super) fn list_lines(triggers: &Value) -> String {
    let text = |t: &Value, key: &str| t[key].as_str().unwrap_or("").to_string();
    triggers
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|t| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                text(t, "id"),
                state(t),
                text(t, "cron"),
                text(t, "pipeline_id"),
                text(t, "name"),
                text(t, "next_fire_at"),
            )
        })
        .collect()
}

pub(super) fn show_text(trigger: &Value) -> String {
    let text = |key: &str| trigger[key].as_str().unwrap_or("").to_string();
    let mut out = format!("{}  {}  ({})\n", text("id"), text("name"), state(trigger));
    out.push_str(&format!("pipeline: {}\n", text("pipeline_id")));
    out.push_str(&format!("cron: {}\n", text("cron")));
    for (label, key) in [
        ("repository", "target_repo"),
        ("project", "project_id"),
        ("source branch", "source_branch"),
        ("guard", "guard_command"),
        ("sandbox", "sandbox"),
        ("harness", "harness"),
        ("next fire", "next_fire_at"),
        ("last fired", "last_fired_at"),
        ("last outcome", "last_outcome"),
    ] {
        if let Some(value) = trigger[key].as_str().filter(|v| !v.is_empty()) {
            out.push_str(&format!("{label}: {value}\n"));
        }
    }
    let mut overlap = text("overlap_policy");
    if let Some(max) = trigger["max_concurrent"].as_i64() {
        overlap.push_str(&format!(" (at most {max})"));
    }
    out.push_str(&format!("overlap: {overlap}\n"));
    if let Some(reason) = trigger["suspension"]["reason"].as_str() {
        out.push_str(&format!("suspended: {reason}\n"));
    }
    out.push_str(&format!("input: {}\n", text("input_template")));
    out
}

pub(super) fn fire_line(id: &str, answer: &Value) -> String {
    if answer["fired"] == true {
        format!(
            "{id} fired: run {}",
            answer["run_id"].as_str().unwrap_or_default()
        )
    } else {
        format!(
            "{id} not fired ({}): {}",
            answer["outcome"].as_str().unwrap_or("skipped"),
            answer["reason"].as_str().unwrap_or_default()
        )
    }
}

pub(super) fn fire_lines(fires: &Value) -> String {
    let text = |f: &Value, key: &str| f[key].as_str().unwrap_or("").to_string();
    fires
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|f| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                text(f, "ts"),
                text(f, "source"),
                text(f, "outcome"),
                text(f, "run_id"),
                text(f, "reason"),
            )
        })
        .collect()
}

pub(super) fn guard_text(result: &Value) -> String {
    let mut out = match result["outcome"].as_str() {
        Some("pass") => "pass: a fire would go ahead\n".to_string(),
        Some("skip") => format!(
            "skip: a fire would be skipped (exit {})\n",
            result["exit_code"]
        ),
        _ => format!(
            "error: {}\n",
            result["detail"].as_str().unwrap_or("the guard did not run")
        ),
    };
    for key in ["stdout", "stderr"] {
        if let Some(text) = result[key].as_str().filter(|t| !t.is_empty()) {
            out.push_str(&format!("{key}:\n{text}"));
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_trigger_reads_as_its_state() {
        assert_eq!(state(&json!({ "enabled": true })), "enabled");
        assert_eq!(state(&json!({ "enabled": false })), "paused");
        assert_eq!(
            state(
                &json!({ "enabled": true, "suspension": { "extension": "test", "reason": "r" } })
            ),
            "suspended"
        );
        assert_eq!(
            list_lines(&json!([{
                "id": "tr-1", "enabled": true, "cron": "0 3 * * *", "pipeline_id": "solo",
                "name": "nightly", "next_fire_at": "2026-11-06T03:00:00.000Z"
            }])),
            "tr-1\tenabled\t0 3 * * *\tsolo\tnightly\t2026-11-06T03:00:00.000Z\n"
        );
    }

    #[test]
    fn a_fire_says_its_run_or_why_it_skipped() {
        assert_eq!(
            fire_line("tr-1", &json!({ "fired": true, "run_id": "r1" })),
            "tr-1 fired: run r1"
        );
        assert_eq!(
            fire_line(
                "tr-1",
                &json!({ "fired": false, "outcome": "skipped", "reason": "guard exited 1" })
            ),
            "tr-1 not fired (skipped): guard exited 1"
        );
    }

    #[test]
    fn a_guard_dry_run_says_what_a_fire_would_do() {
        assert_eq!(
            guard_text(&json!({ "outcome": "pass", "stdout": "ok", "stderr": "" })),
            "pass: a fire would go ahead\nstdout:\nok\n"
        );
        assert_eq!(
            guard_text(&json!({ "outcome": "skip", "exit_code": 1, "stdout": "", "stderr": "" })),
            "skip: a fire would be skipped (exit 1)\n"
        );
    }
}
