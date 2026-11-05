//! `pdo whoami`: the Actor the daemon names the caller, over `GET /actors/me`
//! — the local operator, the agent of the node session the command runs in,
//! or the one an extension establishes from `PDO_TOKEN`.

use anyhow::Result;
use pdo_platform::adapters::cli::daemon::{print_json, Daemon};
use serde_json::Value;

pub fn run_whoami(json: bool) -> Result<()> {
    let me = Daemon::from_env().get("/actors/me")?.ok()?.json()?;
    if json {
        print_json(&me);
    } else {
        println!("{}", line(&me));
    }
    Ok(())
}

/// `<who>` and, when an extension names it, `(<display name>)`.
fn line(me: &Value) -> String {
    let actor = &me["actor"];
    let who = match actor["kind"].as_str().unwrap_or("") {
        "agent" => format!(
            "agent:{}/{}",
            actor["run_id"].as_str().unwrap_or(""),
            actor["node_id"].as_str().unwrap_or("")
        ),
        "trigger" => format!("trigger:{}", actor["trigger_id"].as_str().unwrap_or("")),
        "identified" => format!(
            "{}:{}",
            actor["namespace"].as_str().unwrap_or(""),
            actor["id"].as_str().unwrap_or("")
        ),
        other => other.to_string(),
    };
    match me["display"]["name"].as_str() {
        Some(name) => format!("{who} ({name})"),
        None => who,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_caller_reads_as_one_line() {
        assert_eq!(
            line(&json!({ "actor": { "kind": "local_operator" }, "display": null })),
            "local_operator"
        );
        assert_eq!(
            line(&json!({
                "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
                "display": { "name": "Alice", "avatar_url": null }
            })),
            "test:alice (Alice)"
        );
        assert_eq!(
            line(&json!({ "actor": { "kind": "agent", "run_id": "r1", "node_id": "n" } })),
            "agent:r1/n"
        );
    }
}
