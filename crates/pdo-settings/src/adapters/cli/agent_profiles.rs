//! `pdo settings agent-profile`: the agentic profiles of Settings › Agents. An
//! edit reads the profile, changes what the flags name and writes it back
//! whole, as the editor does.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, referent_lines, segment, Daemon};
use serde_json::{json, Value};

use super::render::inline;

#[derive(Subcommand, Debug)]
pub enum AgentProfileAction {
    /// The profiles, one tab-separated line each: id, name, harness, model,
    /// effort.
    List {
        #[arg(long)]
        json: bool,
    },
    /// A profile, as `list` prints it.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a profile: a name over a harness, with its model and effort.
    Create {
        name: String,
        #[arg(long)]
        harness: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Change a profile: only the flags given change; an empty `--model` or
    /// `--effort` clears it.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Who names the profile: the instance, Projets, Triggers, pipeline nodes.
    Referents {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a profile, even one something still names (`referents` lists
    /// who): each of them falls back to the next tier. `Default` is never
    /// deleted.
    Delete {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

pub(super) fn agent_profile(daemon: &Daemon, action: AgentProfileAction) -> Result<()> {
    match action {
        AgentProfileAction::List { json } => {
            let list = daemon.get("/settings/agent-profiles")?.ok()?.json()?;
            if json {
                print_json(&list);
            } else {
                for profile in list["profiles"].as_array().into_iter().flatten() {
                    println!("{}", agent_line(profile));
                }
            }
        }
        AgentProfileAction::Show { id, json } => {
            let profile = read_agent(daemon, &id)?;
            if json {
                print_json(&profile);
            } else {
                println!("{}", agent_line(&profile));
            }
        }
        AgentProfileAction::Create {
            name,
            harness,
            model,
            effort,
            json,
        } => {
            let profile = daemon
                .post(
                    "/settings/agent-profiles",
                    Some(&json!({ "name": name, "harness": harness, "model": model, "effort": effort })),
                )?
                .ok()?
                .json()?;
            done(&profile, json, &agent_line(&profile), "created");
        }
        AgentProfileAction::Update {
            id,
            name,
            harness,
            model,
            effort,
            json,
        } => {
            anyhow::ensure!(
                name.is_some() || harness.is_some() || model.is_some() || effort.is_some(),
                "nothing to change: give at least one flag (see --help)"
            );
            let current = read_agent(daemon, &id)?;
            let mut body = json!({
                "name": current["name"],
                "harness": current["harness"],
                "model": current["model"],
                "effort": current["effort"],
            });
            for (key, value) in [("name", name), ("harness", harness)] {
                if let Some(value) = value {
                    body[key] = json!(value);
                }
            }
            for (key, value) in [("model", model), ("effort", effort)] {
                if let Some(value) = value {
                    body[key] = if value.is_empty() {
                        Value::Null
                    } else {
                        json!(value)
                    };
                }
            }
            let profile = daemon
                .put(&format!("/settings/agent-profiles/{}", segment(&id)), &body)?
                .ok()?
                .json()?;
            done(&profile, json, &agent_line(&profile), "updated");
        }
        AgentProfileAction::Referents { id, json } => {
            let referents = daemon
                .get(&format!(
                    "/settings/agent-profiles/{}/referents",
                    segment(&id)
                ))?
                .ok()?
                .json()?;
            referents_out(&referents, json);
        }
        AgentProfileAction::Delete { id, json } => {
            daemon
                .delete(&format!("/settings/agent-profiles/{}", segment(&id)), None)?
                .ok()?;
            deleted(&id, json);
        }
    }
    Ok(())
}

fn read_agent(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/settings/agent-profiles/{}", segment(id)))?
        .ok()?
        .json()
}

fn agent_line(profile: &Value) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        inline(&profile["id"]),
        inline(&profile["name"]),
        inline(&profile["harness"]),
        inline(&profile["model"]),
        inline(&profile["effort"])
    )
}

pub(super) fn done(subject: &Value, json: bool, line: &str, verb: &str) {
    if json {
        print_json(subject);
    } else {
        println!("{line} {verb}");
    }
}

pub(super) fn deleted(id: &str, json: bool) {
    if json {
        print_json(&json!({ "id": id, "deleted": true }));
    } else {
        println!("{id} deleted");
    }
}

pub(super) fn referents_out(referents: &Value, json: bool) {
    if json {
        print_json(referents);
    } else {
        print!("{}", referent_lines(referents));
    }
}
