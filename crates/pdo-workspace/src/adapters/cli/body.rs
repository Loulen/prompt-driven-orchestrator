//! The bodies `pdo project` sends: the fields of an update, the whole skills
//! selection the route replaces.

use pdo_platform::adapters::cli::daemon::agent_choice;
use serde_json::{json, Map, Value};

use super::Preference;

/// The selection a Projet carries, as the route answers it.
pub(super) fn selected(project: &Value) -> Vec<Value> {
    project["skills"].as_array().cloned().unwrap_or_default()
}

/// The selection `ids` names, each keeping the label `current` gives it.
pub(super) fn selection_of(current: &[Value], ids: &[String]) -> Vec<Value> {
    ids.iter()
        .map(|id| {
            current
                .iter()
                .find(|s| s["id"] == id.as_str())
                .cloned()
                .unwrap_or_else(|| json!({ "id": id, "name": id }))
        })
        .collect()
}

/// `current` with the skill `id` added last (left as it is when already
/// there): the whole selection the route replaces.
pub(super) fn with_skill(mut current: Vec<Value>, id: &str, name: Option<&str>) -> Vec<Value> {
    if !current.iter().any(|s| s["id"] == id) {
        current.push(json!({ "id": id, "name": name.unwrap_or(id) }));
    }
    current
}

pub(super) fn without_skill(current: Vec<Value>, id: &str) -> Vec<Value> {
    current.into_iter().filter(|s| s["id"] != id).collect()
}

/// The flags of `pdo project update`.
pub(super) struct UpdateFlags {
    pub(super) name: Option<String>,
    pub(super) harness: Option<String>,
    pub(super) clear_harness: bool,
    pub(super) profile: Option<String>,
    pub(super) agent_harness: Option<String>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) clear_agent: bool,
    pub(super) auto_fail: Option<Preference>,
    pub(super) primary_repo: Option<String>,
    pub(super) first_primary: bool,
}

/// The `PATCH /projects/{id}` body of the flags given: an absent key leaves
/// the setting, `null` clears it.
pub(super) fn update_body(flags: UpdateFlags) -> Value {
    let mut body = Map::new();
    if let Some(name) = flags.name {
        body.insert("name".into(), json!(name));
    }
    if let Some(harness) = flags.harness {
        body.insert("harness".into(), json!(harness));
    } else if flags.clear_harness {
        body.insert("harness".into(), Value::Null);
    }
    if let Some(choice) = agent_choice(
        flags.profile,
        flags.agent_harness,
        flags.model,
        flags.effort,
    ) {
        body.insert("agent_choice".into(), choice);
    } else if flags.clear_agent {
        body.insert("agent_choice".into(), Value::Null);
    }
    match flags.auto_fail {
        Some(Preference::On) => body.insert("auto_fail".into(), json!(true)),
        Some(Preference::Off) => body.insert("auto_fail".into(), json!(false)),
        Some(Preference::Inherit) => body.insert("auto_fail".into(), Value::Null),
        None => None,
    };
    if let Some(primary) = flags.primary_repo {
        body.insert("primary_repo".into(), json!(primary));
    } else if flags.first_primary {
        body.insert("primary_repo".into(), Value::Null);
    }
    Value::Object(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flags() -> UpdateFlags {
        UpdateFlags {
            name: None,
            harness: None,
            clear_harness: false,
            profile: None,
            agent_harness: None,
            model: None,
            effort: None,
            clear_agent: false,
            auto_fail: None,
            primary_repo: None,
            first_primary: false,
        }
    }

    #[test]
    fn an_update_sends_only_the_flags_given() {
        assert_eq!(update_body(flags()), json!({}));
        let body = update_body(UpdateFlags {
            name: Some("Web".into()),
            clear_harness: true,
            auto_fail: Some(Preference::Inherit),
            first_primary: true,
            ..flags()
        });
        assert_eq!(
            body,
            json!({ "name": "Web", "harness": null, "auto_fail": null, "primary_repo": null })
        );
    }

    #[test]
    fn an_agentic_profile_or_an_inline_combination_is_the_ui_s_union() {
        let body = update_body(UpdateFlags {
            profile: Some("reviewer".into()),
            ..flags()
        });
        assert_eq!(
            body["agent_choice"],
            json!({ "mode": "profile", "profile_id": "reviewer" })
        );
        let body = update_body(UpdateFlags {
            agent_harness: Some("claude".into()),
            model: Some("opus".into()),
            ..flags()
        });
        assert_eq!(
            body["agent_choice"],
            json!({ "mode": "custom", "harness": "claude", "model": "opus" })
        );
        let body = update_body(UpdateFlags {
            clear_agent: true,
            auto_fail: Some(Preference::On),
            ..flags()
        });
        assert_eq!(body, json!({ "agent_choice": null, "auto_fail": true }));
    }

    #[test]
    fn adding_a_skill_keeps_the_selection_and_never_doubles_it() {
        let current = vec![json!({ "id": "tdd", "name": "TDD" })];
        let added = with_skill(current.clone(), "review", None);
        assert_eq!(
            added,
            vec![
                json!({ "id": "tdd", "name": "TDD" }),
                json!({ "id": "review", "name": "review" })
            ]
        );
        assert_eq!(with_skill(current.clone(), "tdd", Some("x")), current);
        assert_eq!(
            without_skill(added, "tdd"),
            vec![json!({ "id": "review", "name": "review" })]
        );
    }

    #[test]
    fn a_set_selection_keeps_the_labels_it_already_had() {
        let current = vec![json!({ "id": "tdd", "name": "TDD" })];
        assert_eq!(
            selection_of(&current, &["review".into(), "tdd".into()]),
            vec![
                json!({ "id": "review", "name": "review" }),
                json!({ "id": "tdd", "name": "TDD" })
            ]
        );
    }
}
