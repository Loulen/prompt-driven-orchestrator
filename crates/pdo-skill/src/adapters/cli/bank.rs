//! A skill of the bank, as the commands name it and read it.

use anyhow::Result;
use pdo_platform::adapters::cli::daemon::{segment, Daemon};
use serde_json::Value;

/// The id of the skill a command names: its id, or its name as `add` and
/// `list` print it (a name is unique in the bank). A key that is neither is
/// passed on as is, and the daemon answers it is no skill.
pub(super) fn skill_id(daemon: &Daemon, key: &str) -> Result<String> {
    let bank = daemon.get("/settings/skills")?.ok()?.json()?;
    Ok(skill_id_in(&bank, key).unwrap_or_else(|| key.to_string()))
}

fn skill_id_in(bank: &Value, key: &str) -> Option<String> {
    let skills = bank["skills"].as_array()?;
    let id_of = |skill: &Value| skill["id"].as_str().map(str::to_string);
    skills
        .iter()
        .find(|skill| skill["id"] == key)
        .or_else(|| skills.iter().find(|skill| skill["name"] == key))
        .and_then(id_of)
}

pub(super) fn read(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/settings/skills/{}", segment(id)))?
        .ok()?
        .json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_skill_is_named_by_its_id_or_its_name() {
        let bank = json!({ "skills": [
            { "id": "a1b2", "name": "fp-demo-skill" },
            { "id": "tdd", "name": "tdd" },
            { "id": "c3d4", "name": "a1b2" },
        ]});
        assert_eq!(skill_id_in(&bank, "fp-demo-skill").as_deref(), Some("a1b2"));
        assert_eq!(skill_id_in(&bank, "tdd").as_deref(), Some("tdd"));
        // An id wins over a name that spells it.
        assert_eq!(skill_id_in(&bank, "a1b2").as_deref(), Some("a1b2"));
        assert_eq!(skill_id_in(&bank, "nope"), None);
    }
}
