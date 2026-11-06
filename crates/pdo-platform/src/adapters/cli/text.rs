//! What the operator's commands read from a command line and print from an
//! answer, the same way across contexts: a `<key>=<value>`, who still names a
//! subject, provisioning rules (a Projet's, the instance's).

use anyhow::{Context, Result};
use serde_json::Value;

/// A `<key>=<value>` of a command line. The value is read as JSON when it is
/// JSON (`5`, `true`, `null`, `"quoted"`, `[…]`, `{…}`), as a string otherwise:
/// `public_url=https://pdo.example.net` sends a string, `session_cap=5` a
/// number, `session_cap='"5"'` the string `"5"`.
pub fn assignment(raw: &str) -> Result<(String, Value)> {
    let (key, value) = raw
        .split_once('=')
        .with_context(|| format!("expected <key>=<value>, got {raw:?}"))?;
    let key = key.trim();
    anyhow::ensure!(!key.is_empty(), "expected <key>=<value>, got {raw:?}");
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()));
    Ok((key.to_string(), value))
}

/// Who still names a subject (a skill, a profile), as a referents answer lists
/// them: one tab-separated line each, the tier then its id and name (`runs`
/// name a Run by `run_id`), `instance` when the instance's defaults name it.
/// `none` when nobody does.
pub fn referent_lines(referents: &Value) -> String {
    let mut out = String::new();
    let Some(tiers) = referents.as_object() else {
        return "none\n".to_string();
    };
    for (tier, value) in tiers {
        match value {
            Value::Bool(true) if matches!(tier.as_str(), "instance" | "instance_default") => {
                out.push_str("instance\n");
            }
            Value::Array(items) => {
                for item in items {
                    let id = item["id"]
                        .as_str()
                        .or_else(|| item["run_id"].as_str())
                        .unwrap_or("");
                    let mut line = format!("{tier}\t{id}\t{}", item["name"].as_str().unwrap_or(""));
                    if let Some(node) = item["node_id"].as_str() {
                        line.push_str(&format!("\tnode {node}"));
                    }
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        out.push_str("none\n");
    }
    out
}

/// One line per kind of provisioning rule: `copy: .env, data/`, or `none`.
pub fn rules_lines(rules: &Value) -> String {
    ["copy", "hardlink", "symlink"]
        .iter()
        .map(|kind| {
            let globs: Vec<&str> = rules[kind]
                .as_array()
                .map(|g| g.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let globs = if globs.is_empty() {
                "none".to_string()
            } else {
                globs.join(", ")
            };
            format!("{kind}: {globs}\n")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisioning_rules_show_one_line_per_kind() {
        let rules = serde_json::json!({ "copy": [".env", "data/"], "hardlink": [], "symlink": [] });
        assert_eq!(
            rules_lines(&rules),
            "copy: .env, data/\nhardlink: none\nsymlink: none\n"
        );
    }

    #[test]
    fn an_assignment_reads_its_value_as_json_or_as_a_string() {
        assert_eq!(
            assignment("session_cap=5").unwrap(),
            ("session_cap".into(), serde_json::json!(5))
        );
        assert_eq!(
            assignment("public_url=https://pdo.example.net").unwrap(),
            (
                "public_url".into(),
                serde_json::json!("https://pdo.example.net")
            )
        );
        assert_eq!(
            assignment(r#"session_cap="5""#).unwrap().1,
            serde_json::json!("5")
        );
        assert_eq!(
            assignment("default_sandbox=").unwrap().1,
            serde_json::json!("")
        );
        assert!(assignment("no-equal-sign").is_err());
        assert!(assignment("=5").is_err());
    }

    #[test]
    fn referents_are_one_line_per_tier_entry() {
        let referents = serde_json::json!({
            "skill_id": "s-1",
            "instance": true,
            "projects": [{ "id": "prj-1", "name": "Web" }],
            "pipelines": [{ "id": "p", "name": "Ship", "node_id": "n1", "scope": "instance" }],
            "runs": [{ "run_id": "r-1", "name": "nightly" }],
        });
        assert_eq!(
            referent_lines(&referents),
            "instance\npipelines\tp\tShip\tnode n1\nprojects\tprj-1\tWeb\nruns\tr-1\tnightly\n"
        );
        assert_eq!(
            referent_lines(&serde_json::json!({ "instance": false, "projects": [] })),
            "none\n"
        );
    }
}
