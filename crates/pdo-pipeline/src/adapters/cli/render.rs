//! What `pdo pipeline` prints without `--json`.

use pdo_platform::adapters::cli::daemon::print_json;
use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

fn items(list: &Value) -> &[Value] {
    list.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// The answer with `--json`, else the one line `line` makes of it.
pub(super) fn answer(value: &Value, json: bool, line: impl FnOnce(&Value) -> String) {
    if json {
        print_json(value);
    } else {
        println!("{}", line(value));
    }
}

/// The warnings of an import, one per line on the standard error.
pub(super) fn warn(answer: &Value) {
    for warning in items(&answer["warnings"]) {
        eprintln!("warning: {}", warning.as_str().unwrap_or_default());
    }
}

/// `all`, `all (first: p1,p2)`, `none` or `projects: p1,p2`.
pub(super) fn validation(validation: &Value) -> String {
    let ids = items(&validation["project_ids"])
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    match text(validation, "kind") {
        "none" => "none".to_string(),
        "projects" => format!("projects: {ids}"),
        _ if ids.is_empty() => "all".to_string(),
        _ => format!("all (first: {ids})"),
    }
}

/// One tab-separated line per pipeline: id, name, last version, node count,
/// validation, and why its file does not parse when it does not.
pub(super) fn list_lines(pipelines: &Value) -> String {
    items(pipelines)
        .iter()
        .map(|p| {
            let version = p["version"]["number"]
                .as_u64()
                .map_or_else(|| "-".to_string(), |n| format!("v{n}"));
            format!(
                "{}\t{}\t{version}\t{} nodes\t{}\t{}\n",
                text(p, "id"),
                text(p, "name"),
                p["node_count"].as_u64().unwrap_or(0),
                validation(&p["validation"]),
                p["invalid"]
                    .as_str()
                    .map(|why| format!("invalid: {why}"))
                    .unwrap_or_default(),
            )
        })
        .collect()
}

/// Who an Actor is, in one word: `local_operator`, `agent:<run>/<node>`,
/// `trigger:<id>`, `<namespace>:<id>`.
pub(super) fn actor(actor: &Value) -> String {
    match text(actor, "kind") {
        "agent" => format!("agent:{}/{}", text(actor, "run_id"), text(actor, "node_id")),
        "trigger" => format!("trigger:{}", text(actor, "trigger_id")),
        "identified" => format!("{}:{}", text(actor, "namespace"), text(actor, "id")),
        other => other.to_string(),
    }
}

/// One tab-separated line per version, newest first: number, title, when,
/// who (`external` for a file written outside PDO, `-` when unknown), and
/// the Runs that launched it.
pub(super) fn version_lines(history: &Value) -> String {
    items(&history["versions"])
        .iter()
        .map(|v| {
            let author = if v["external"] == true {
                "external".to_string()
            } else if v["author"].is_null() {
                "-".to_string()
            } else {
                actor(&v["author"])
            };
            format!(
                "v{}\t{}\t{}\t{author}\t{} runs\n",
                v["number"],
                text(v, "title"),
                text(v, "saved_at"),
                v["runs"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

/// The YAML document a pipeline or a version holds, as it is.
pub(super) fn yaml(detail: &Value) -> String {
    let yaml = text(detail, "yaml");
    if yaml.ends_with('\n') || yaml.is_empty() {
        yaml.to_string()
    } else {
        format!("{yaml}\n")
    }
}

/// `<id> saved as v<n>`, and the pipeline kept or the rename when there is one.
pub(super) fn saved_line(saved: &Value, from: &str) -> String {
    let id = text(saved, "id");
    let mut line = format!("{id} saved as v{}", saved["version"]["number"]);
    if saved["renamed"] == true && id != from {
        line.push_str(&format!(" (renamed from {from})"));
    }
    if let Some(kept) = saved["kept"]["id"].as_str() {
        line.push_str(&format!(", previous version kept as {kept}"));
    }
    line
}

/// One tab-separated line per library entry: name, type, model, effort
/// (`-` for the account default).
pub(super) fn library_lines(entries: &Value) -> String {
    items(entries)
        .iter()
        .map(|e| {
            let or_default = |key: &str| e[key].as_str().unwrap_or("-").to_string();
            format!(
                "{}\t{}\t{}\t{}\n",
                text(e, "name"),
                text(e, "type"),
                or_default("model"),
                or_default("effort"),
            )
        })
        .collect()
}

/// A node spec (`spec` of an instantiation or a parse) as YAML, its prompt
/// beside its fields; the empty and null fields left out.
pub(super) fn node_yaml(answer: &Value) -> String {
    let mut node = serde_json::Map::new();
    if let Some(spec) = answer["spec"].as_object() {
        for (key, value) in spec {
            let empty = value.is_null()
                || value.as_array().is_some_and(Vec::is_empty)
                || value.as_object().is_some_and(serde_json::Map::is_empty);
            if !empty {
                node.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(prompt) = answer["prompt"].as_str().filter(|p| !p.is_empty()) {
        node.insert("prompt".into(), prompt.into());
    }
    serde_yaml::to_string(&Value::Object(node)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_pipeline_is_one_line() {
        assert_eq!(
            list_lines(&json!([
                {
                    "id": "solo", "name": "solo", "node_count": 3,
                    "validation": { "kind": "projects", "project_ids": ["p1", "p2"] },
                    "version": { "number": 2, "title": "v2" }
                },
                {
                    "id": "broken", "name": "broken", "node_count": 0,
                    "validation": { "kind": "all" }, "invalid": "missing name"
                }
            ])),
            "solo\tsolo\tv2\t3 nodes\tprojects: p1,p2\t\n\
             broken\tbroken\t-\t0 nodes\tall\tinvalid: missing name\n"
        );
        assert_eq!(
            validation(&json!({ "kind": "all", "project_ids": ["p1"] })),
            "all (first: p1)"
        );
        assert_eq!(validation(&json!({ "kind": "none" })), "none");
    }

    #[test]
    fn a_version_says_who_saved_it() {
        assert_eq!(
            version_lines(&json!({ "versions": [
                {
                    "number": 2, "title": "v2", "saved_at": "2026-11-05T10:00:00Z",
                    "author": { "kind": "local_operator" }, "external": false, "runs": 1
                },
                {
                    "number": 1, "title": "v1", "saved_at": "2026-11-04T10:00:00Z",
                    "author": null, "external": false, "runs": 0
                },
                {
                    "number": 0, "title": "edit", "saved_at": "t",
                    "author": null, "external": true, "runs": 0
                }
            ]})),
            "v2\tv2\t2026-11-05T10:00:00Z\tlocal_operator\t1 runs\n\
             v1\tv1\t2026-11-04T10:00:00Z\t-\t0 runs\n\
             v0\tedit\tt\texternal\t0 runs\n"
        );
        assert_eq!(
            actor(&json!({ "kind": "agent", "run_id": "r1", "node_id": "n" })),
            "agent:r1/n"
        );
    }

    #[test]
    fn a_save_says_its_version_and_what_moved() {
        assert_eq!(
            saved_line(
                &json!({ "id": "solo", "renamed": false, "version": { "number": 3 } }),
                "solo"
            ),
            "solo saved as v3"
        );
        assert_eq!(
            saved_line(
                &json!({
                    "id": "next", "renamed": true, "version": { "number": 4 },
                    "kept": { "id": "solo-v3" }
                }),
                "solo"
            ),
            "next saved as v4 (renamed from solo), previous version kept as solo-v3"
        );
    }

    #[test]
    fn a_library_entry_is_one_line_and_a_node_reads_as_yaml() {
        assert_eq!(
            library_lines(&json!([
                { "name": "reviewer", "type": "agent", "model": "opus", "effort": null }
            ])),
            "reviewer\tagent\topus\t-\n"
        );
        let node = node_yaml(&json!({
            "spec": { "name": "reviewer", "type": "agent", "inputs": [], "model": null },
            "prompt": "Review.",
        }));
        assert_eq!(
            serde_yaml::from_str::<Value>(&node).unwrap(),
            json!({ "name": "reviewer", "type": "agent", "prompt": "Review." })
        );
    }

    #[test]
    fn a_document_ends_with_a_newline() {
        assert_eq!(yaml(&json!({ "yaml": "name: x" })), "name: x\n");
        assert_eq!(yaml(&json!({ "yaml": "name: x\n" })), "name: x\n");
    }
}
