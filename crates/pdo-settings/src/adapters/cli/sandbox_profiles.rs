//! `pdo settings sandbox-profile`: the staging profiles of Settings ›
//! Sandbox. An edit reads the profile, changes what the flags name and writes
//! it back whole, as the editor does.

use std::collections::BTreeSet;

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon, NotFound};
use serde_json::{json, Map, Value};

use super::agent_profiles::{deleted, done, referents_out};
use super::render::inline;

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
