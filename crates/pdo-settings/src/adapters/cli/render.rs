//! The text `pdo settings` prints: what the daemon answers, one fact a line.

use serde_json::Value;

/// A value on one line: a string as it is, anything else as compact JSON,
/// `-` for none.
pub(super) fn inline(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(s) if s.is_empty() => "\"\"".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The instance's own tiers on the page: settings `set` writes that carry no
/// `{effective, source}`, as they are stored.
const TIERS: [&str; 2] = ["agent_choice", "skills"];

/// The settings of a page, one tab-separated line each: key, effective value,
/// its source. A setting carries `{effective, source, …}`; the instance's
/// tiers (its agentic profile, its skills) are printed as stored, with no
/// source. Anything else on the page (a fact shown beside the settings: the
/// harnesses' catalogues, the price table) is printed whole when `whole` is
/// set, and left out otherwise.
pub(super) fn knob_lines(page: &Value, whole: bool) -> String {
    let Some(entries) = page.as_object() else {
        return String::new();
    };
    let mut out = String::new();
    for (key, value) in entries {
        match value.get("effective") {
            Some(effective) if value.get("source").is_some() => out.push_str(&format!(
                "{key}\t{}\t{}\n",
                inline(effective),
                value["source"].as_str().unwrap_or("-")
            )),
            _ if whole || TIERS.contains(&key.as_str()) => {
                out.push_str(&format!("{key}\t{}\n", inline(value)))
            }
            _ => {}
        }
    }
    out
}

/// A harness's catalogue, probed again.
pub(super) fn refreshed_line(name: &str, entry: &Value) -> String {
    let models = entry["models"].as_array().map_or(0, Vec::len);
    let version = entry["version"].as_str().unwrap_or("unknown version");
    format!("{name} refreshed · {version} · {models} model(s)")
}

/// The caller's Apparence, one setting a line; the interface's defaults while
/// it was never written.
pub(super) fn appearance_lines(document: &Value) -> String {
    if document.is_null() {
        return "the interface's defaults (never written)\n".to_string();
    }
    let mut out = String::new();
    for key in ["theme", "dark_mode", "compact_nodes"] {
        out.push_str(&format!("{key}\t{}\n", inline(&document[key])));
    }
    if let Some(elements) = document["elements"].as_object() {
        for (id, visible) in elements {
            let shown = if visible == true { "shown" } else { "hidden" };
            out.push_str(&format!("element\t{id}\t{shown}\n"));
        }
    }
    out
}

/// The state of the OpenTelemetry export.
pub(super) fn telemetry_line(status: &Value) -> String {
    let exporting = if status["exporting"] == true {
        format!("exporting every {}s", status["interval_secs"])
    } else {
        "not exporting".to_string()
    };
    let metrics = if status["metrics_endpoint"] == true {
        "/metrics on"
    } else {
        "/metrics off"
    };
    let last = &status["last_export"];
    let last = if last.is_null() {
        "no export yet".to_string()
    } else {
        format!(
            "last {} {} at {}: {}",
            last["signal"].as_str().unwrap_or("export"),
            if last["ok"] == true { "ok" } else { "failed" },
            last["at"].as_str().unwrap_or("?"),
            last["detail"].as_str().unwrap_or("")
        )
    };
    format!("{exporting} · {metrics} · {last}")
}

/// What a price sync did.
pub(super) fn sync_line(report: &Value) -> String {
    if report["noop"] == true {
        return report["reason"]
            .as_str()
            .unwrap_or("table already up to date")
            .to_string();
    }
    let count = |key: &str| report[key].as_array().map_or(0, Vec::len);
    format!(
        "price table synced: {} row(s), {} added, {} updated",
        report["rows"],
        count("added"),
        count("updated")
    )
}

/// A provisioning preview: one tab-separated line per path (mode, path, the
/// level that brings it, its pattern), then one per conflict.
pub(super) fn plan_lines(plan: &Value) -> String {
    let mut out = String::new();
    for entry in plan["entries"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}{}\n",
            inline(&entry["mode"]),
            inline(&entry["relative_path"]),
            inline(&entry["origin_scope"]),
            inline(&entry["pattern"]),
            if entry["provided_by_git"] == true {
                "\t(provided by git)"
            } else {
                ""
            }
        ));
    }
    for conflict in plan["conflicts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "conflict\t{}\t{}\t{}\n",
            inline(&conflict["relative_path"]),
            inline(&conflict["scope"]),
            inline(&conflict["modes"])
        ));
    }
    if out.is_empty() {
        out.push_str("nothing provisioned\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_setting_shows_its_effective_value_and_source() {
        let page = json!({
            "session_cap": { "effective": 8, "source": "stored", "stored": 8, "env": null, "default": 6 },
            "public_url": { "effective": null, "source": "default" },
            "skills": [{ "id": "tdd" }],
            "price_table": { "rows": 12 },
        });
        assert_eq!(
            knob_lines(&page, false),
            "public_url\t-\tdefault\nsession_cap\t8\tstored\nskills\t[{\"id\":\"tdd\"}]\n"
        );
        assert_eq!(
            knob_lines(&page, true),
            "price_table\t{\"rows\":12}\npublic_url\t-\tdefault\nsession_cap\t8\tstored\nskills\t[{\"id\":\"tdd\"}]\n"
        );
    }

    #[test]
    fn an_appearance_never_written_says_so() {
        assert!(appearance_lines(&Value::Null).contains("defaults"));
        let lines = appearance_lines(&json!({
            "version": 1, "theme": "classic", "dark_mode": true,
            "elements": { "runs.cost": false },
        }));
        assert_eq!(
            lines,
            "theme\tclassic\ndark_mode\ttrue\ncompact_nodes\t-\nelement\truns.cost\thidden\n"
        );
    }

    #[test]
    fn a_price_sync_says_what_moved_or_why_nothing_did() {
        assert_eq!(
            sync_line(&json!({ "noop": true, "reason": "table already up to date" })),
            "table already up to date"
        );
        assert_eq!(
            sync_line(&json!({ "rows": 12, "added": ["a"], "updated": ["b", "c"] })),
            "price table synced: 12 row(s), 1 added, 2 updated"
        );
    }

    #[test]
    fn a_preview_lists_its_paths_then_its_conflicts() {
        let plan = json!({
            "entries": [{ "relative_path": ".env", "mode": "copy", "origin_scope": "instance",
                          "pattern": ".env", "provided_by_git": false }],
            "rules": [],
            "conflicts": [{ "scope": "project", "relative_path": "data", "modes": ["copy", "symlink"] }],
        });
        assert_eq!(
            plan_lines(&plan),
            "copy\t.env\tinstance\t.env\nconflict\tdata\tproject\t[\"copy\",\"symlink\"]\n"
        );
        assert_eq!(plan_lines(&json!({})), "nothing provisioned\n");
    }
}
