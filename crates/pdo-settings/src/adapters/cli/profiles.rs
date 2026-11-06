//! `pdo settings agent-profile` and `pdo settings sandbox-profile`: the
//! agentic profiles of Settings › Agents and the staging profiles of
//! Settings › Sandbox. An edit reads the profile, changes what the flags name
//! and writes it back whole, as the editors do.

use std::collections::BTreeSet;

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, referent_lines, segment, Daemon, NotFound};
use serde_json::{json, Map, Value};

use super::render::inline;

#[derive(Subcommand, Debug)]
pub enum AgentProfileAction {
    /// The profiles, one tab-separated line each: id, name, harness, model,
    /// effort.
    List {
        #[arg(long)]
        json: bool,
    },
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
    /// Delete a profile. The daemon refuses while something names it.
    Delete {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SandboxProfileAction {
    /// The staging profiles, one tab-separated line each: name, `virtual` or
    /// `stored`, the number of paths a Run created now would stage.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a profile: the paths it stages, its environment, its image.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Create or change a profile: only the flags given change. A profile
    /// not stored yet starts from the default entries.
    Set {
        name: String,
        /// Stop staging a default entry (repeatable).
        #[arg(long, value_name = "PATH")]
        disable: Vec<String>,
        /// Stage a default entry again (repeatable).
        #[arg(long, value_name = "PATH")]
        enable: Vec<String>,
        /// Stage one more path (repeatable).
        #[arg(long, value_name = "PATH")]
        extra: Vec<String>,
        /// Stop staging an extra path (repeatable).
        #[arg(long, value_name = "PATH")]
        remove_extra: Vec<String>,
        /// Set an environment variable of the container (repeatable).
        #[arg(long, value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// Remove an environment variable (repeatable).
        #[arg(long, value_name = "KEY")]
        unset_env: Vec<String>,
        /// Build the image from this Dockerfile.
        #[arg(long, conflicts_with_all = ["registry", "default_image"])]
        dockerfile: Option<String>,
        /// Pull this image reference.
        #[arg(long, conflicts_with = "default_image")]
        registry: Option<String>,
        /// Back to the default image.
        #[arg(long)]
        default_image: bool,
        #[arg(long)]
        json: bool,
    },
    /// Who names the profile: the instance default, Triggers, live Runs.
    Referents {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a stored profile.
    Delete {
        name: String,
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

pub(super) fn sandbox_profile(daemon: &Daemon, action: SandboxProfileAction) -> Result<()> {
    match action {
        SandboxProfileAction::List { json } => {
            let list = daemon.get("/settings/sandbox-profiles")?.ok()?.json()?;
            if json {
                print_json(&list);
            } else {
                let profiles = list["profiles"].as_array().or_else(|| list.as_array());
                for profile in profiles.into_iter().flatten() {
                    println!("{}", sandbox_line(profile));
                }
            }
        }
        SandboxProfileAction::Show { name, json } => {
            let profile = read_sandbox(daemon, &name)?;
            if json {
                print_json(&profile);
            } else {
                print!("{}", sandbox_text(&profile));
            }
        }
        SandboxProfileAction::Set {
            name,
            disable,
            enable,
            extra,
            remove_extra,
            env,
            unset_env,
            dockerfile,
            registry,
            default_image,
            json,
        } => {
            // A profile not stored yet is the default, with nothing posed.
            let current = match read_sandbox(daemon, &name) {
                Ok(profile) => profile,
                Err(e) if e.downcast_ref::<NotFound>().is_some() => json!({}),
                Err(e) => return Err(e),
            };
            let mut disabled = strings(&current["disabled"]);
            disabled.extend(disable);
            disabled.retain(|path| !enable.contains(path));
            let mut extras = strings(&current["extras"]);
            extras.extend(extra);
            extras.retain(|path| !remove_extra.contains(path));
            let mut variables: Map<String, Value> =
                current["env"].as_object().cloned().unwrap_or_default();
            for raw in &env {
                let (key, value) = raw
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--env expects KEY=VALUE, got {raw:?}"))?;
                variables.insert(key.to_string(), json!(value));
            }
            for key in &unset_env {
                variables.remove(key);
            }
            let image = match (dockerfile, registry, default_image) {
                (Some(path), _, _) => json!({ "kind": "dockerfile", "path": path }),
                (_, Some(image_ref), _) => json!({ "kind": "registry", "ref": image_ref }),
                (_, _, true) => Value::Null,
                _ => current["image"].clone(),
            };
            let body = json!({
                "disabled": disabled,
                "extras": extras,
                "env": variables,
                "image": image,
            });
            let profile = daemon
                .put(
                    &format!("/settings/sandbox-profiles/{}", segment(&name)),
                    &body,
                )?
                .ok()?
                .json()?;
            done(&profile, json, &sandbox_line(&profile), "saved");
        }
        SandboxProfileAction::Referents { name, json } => {
            let referents = daemon
                .get(&format!(
                    "/settings/sandbox-profiles/{}/referents",
                    segment(&name)
                ))?
                .ok()?
                .json()?;
            referents_out(&referents, json);
        }
        SandboxProfileAction::Delete { name, json } => {
            daemon
                .delete(
                    &format!("/settings/sandbox-profiles/{}", segment(&name)),
                    None,
                )?
                .ok()?;
            deleted(&name, json);
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

fn read_sandbox(daemon: &Daemon, name: &str) -> Result<Value> {
    daemon
        .get(&format!("/settings/sandbox-profiles/{}", segment(name)))?
        .ok()?
        .json()
}

/// The strings of a JSON list, in order, without repeats.
fn strings(list: &Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| seen.insert(s.to_string()))
        .map(str::to_string)
        .collect()
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

fn sandbox_line(profile: &Value) -> String {
    let kind = if profile["virtual"] == true {
        "virtual"
    } else {
        "stored"
    };
    let staged = profile["resolved"].as_array().map_or(0, Vec::len);
    format!("{}\t{kind}\t{staged} path(s)", inline(&profile["name"]))
}

fn sandbox_text(profile: &Value) -> String {
    let mut out = format!("{}\n", sandbox_line(profile));
    for path in profile["resolved"].as_array().into_iter().flatten() {
        out.push_str(&format!("stages\t{}\n", inline(path)));
    }
    for path in profile["disabled"].as_array().into_iter().flatten() {
        out.push_str(&format!("disabled\t{}\n", inline(path)));
    }
    if let Some(env) = profile["env"].as_object() {
        for (key, value) in env {
            out.push_str(&format!("env\t{key}={}\n", value.as_str().unwrap_or("")));
        }
    }
    let image = &profile["image"];
    let image = match image["kind"].as_str() {
        Some("dockerfile") => format!("dockerfile {}", inline(&image["path"])),
        Some("registry") => format!("registry {}", inline(&image["ref"])),
        _ => "default".to_string(),
    };
    out.push_str(&format!("image\t{image}\n"));
    out
}

fn done(subject: &Value, json: bool, line: &str, verb: &str) {
    if json {
        print_json(subject);
    } else {
        println!("{line} {verb}");
    }
}

fn deleted(id: &str, json: bool) {
    if json {
        print_json(&json!({ "id": id, "deleted": true }));
    } else {
        println!("{id} deleted");
    }
}

fn referents_out(referents: &Value, json: bool) {
    if json {
        print_json(referents);
    } else {
        print!("{}", referent_lines(referents));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_as_its_strings_once_each() {
        assert_eq!(strings(&json!(["a", "b", "a", 3])), ["a", "b"]);
        assert!(strings(&Value::Null).is_empty());
    }

    #[test]
    fn a_staging_profile_shows_what_it_stages_its_env_and_image() {
        let profile = json!({
            "name": "web", "virtual": false, "resolved": [".claude", ".gitconfig"],
            "disabled": [".ssh"], "env": { "NODE_ENV": "test" },
            "image": { "kind": "registry", "ref": "node:22" },
        });
        assert_eq!(
            sandbox_text(&profile),
            "web\tstored\t2 path(s)\nstages\t.claude\nstages\t.gitconfig\n\
             disabled\t.ssh\nenv\tNODE_ENV=test\nimage\tregistry node:22\n"
        );
    }
}
