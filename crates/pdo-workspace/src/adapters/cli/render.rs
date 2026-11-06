//! What `pdo project` prints without `--json`.

use pdo_platform::adapters::cli::daemon::print_json;
use serde_json::Value;

use super::body::selected;

pub(super) fn done(project: &Value, json: bool, verb: &str) {
    if json {
        print_json(project);
    } else {
        println!(
            "{}\t{} {verb}",
            project["id"].as_str().unwrap_or_default(),
            project["name"].as_str().unwrap_or_default()
        );
    }
}

pub(super) fn show(project: &Value, json: bool) {
    if json {
        print_json(project);
        return;
    }
    print!("{}", show_text(project));
}

pub(super) fn show_text(project: &Value) -> String {
    let text = |key: &str| project[key].as_str().unwrap_or("").to_string();
    let mut out = format!("{}  {}\n", text("id"), text("name"));
    out.push_str(&format!("members: {}\n", members_line(project)));
    if let Some(primary) = project["primary_repo"].as_str() {
        out.push_str(&format!("primary repository: {primary}\n"));
    }
    out.push_str(&format!("agent: {}\n", agent_line(project)));
    let auto_fail = match project["auto_fail"].as_bool() {
        Some(true) => "on",
        Some(false) => "off",
        None => "inherit",
    };
    out.push_str(&format!("auto-fail: {auto_fail}\n"));
    let skills: Vec<String> = selected(project)
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect();
    out.push_str(&format!(
        "skills: {}\n",
        if skills.is_empty() {
            "none".to_string()
        } else {
            skills.join(", ")
        }
    ));
    out
}

pub(super) fn agent_line(project: &Value) -> String {
    let choice = &project["agent_choice"];
    match choice["mode"].as_str() {
        Some("profile") => format!("profile {}", choice["profile_id"].as_str().unwrap_or("")),
        Some("custom") => {
            let mut line = format!("harness {}", choice["harness"].as_str().unwrap_or(""));
            if let Some(model) = choice["model"].as_str() {
                line.push_str(&format!(", model {model}"));
            }
            if let Some(effort) = choice["effort"].as_str() {
                line.push_str(&format!(", effort {effort}"));
            }
            line
        }
        _ => match project["harness"].as_str().filter(|h| !h.is_empty()) {
            Some(harness) => format!("harness {harness}"),
            None => "inherit".to_string(),
        },
    }
}

pub(super) fn members_line(project: &Value) -> String {
    let members: Vec<&str> = project["members"]
        .as_array()
        .map(|m| m.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if members.is_empty() {
        "none".to_string()
    } else {
        members.join(", ")
    }
}

/// One tab-separated line per Projet: `id  name  members  primary`; a deleted
/// one is `id  name`.
pub(super) fn list_lines(projects: &Value, deleted: bool) -> String {
    projects
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|p| {
            let id = p["id"].as_str().unwrap_or("");
            let name = p["name"].as_str().unwrap_or("");
            if deleted {
                return format!("{id}\t{name}\n");
            }
            let members = p["members"].as_array().map_or(0, Vec::len);
            let primary = p["primary_repo"].as_str().unwrap_or("");
            format!("{id}\t{name}\t{members}\t{primary}\n")
        })
        .collect()
}

pub(super) fn skill_lines(project: &Value) -> String {
    selected(project)
        .iter()
        .map(|s| {
            format!(
                "{}\t{}\n",
                s["id"].as_str().unwrap_or(""),
                s["name"].as_str().unwrap_or("")
            )
        })
        .collect()
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
    use serde_json::json;

    #[test]
    fn provisioning_rules_show_one_line_per_kind() {
        let rules = json!({ "copy": [".env", "data/"], "hardlink": [], "symlink": [] });
        assert_eq!(
            rules_lines(&rules),
            "copy: .env, data/\nhardlink: none\nsymlink: none\n"
        );
    }

    #[test]
    fn a_project_shows_as_lines() {
        let project = json!({
            "id": "prj-1", "name": "Web", "members": ["/srv/web", "/srv/api"],
            "primary_repo": "/srv/web",
            "agent_choice": { "mode": "custom", "harness": "claude", "effort": "high" },
            "auto_fail": false,
            "skills": [{ "id": "tdd", "name": "TDD" }]
        });
        assert_eq!(
            show_text(&project),
            "prj-1  Web\nmembers: /srv/web, /srv/api\nprimary repository: /srv/web\n\
             agent: harness claude, effort high\nauto-fail: off\nskills: tdd\n"
        );
        assert_eq!(
            list_lines(&json!([project]), false),
            "prj-1\tWeb\t2\t/srv/web\n"
        );
        assert_eq!(
            show_text(&json!({ "id": "p", "name": "n" })),
            "p  n\nmembers: none\nagent: inherit\nauto-fail: inherit\nskills: none\n"
        );
    }
}
