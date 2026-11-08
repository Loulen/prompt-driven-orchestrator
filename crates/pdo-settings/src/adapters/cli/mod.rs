//! `pdo settings`: the Settings surface of the running daemon, gesture for
//! gesture, over `/settings*` and the routes its sections call (the
//! caller's Apparence, the instance's provisioning rules, the telemetry
//! export, the price table). A thin client: every rule (the ranges, the
//! references that must resolve, who may) stays behind the route, so a value
//! the UI refuses is refused here with the same reason.

mod agent_profiles;
mod render;
mod sandbox_profiles;

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{
    absolute_path, assignment, print_json, read_json_file, rules_lines, segment, Daemon,
};
use serde_json::{json, Map, Value};

pub use agent_profiles::AgentProfileAction;
use render::{knob_lines, plan_lines, refreshed_line, sync_line, telemetry_line};
pub use sandbox_profiles::SandboxProfileAction;

#[derive(Subcommand, Debug)]
pub enum SettingsAction {
    /// The instance settings, one tab-separated line per setting: key,
    /// effective value, where it comes from (`stored`, `env`, `default`).
    /// With keys, those settings only, each with its whole value.
    Get {
        keys: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Change instance settings as Settings saves them: `<key>=<value>`, the
    /// value read as JSON when it is JSON (`session_cap=5`,
    /// `review_agent_can_resolve=true`), as a string otherwise
    /// (`public_url=https://pdo.example.net`); `<key>=` clears a text setting.
    /// The daemon validates the whole change and refuses it with its reason,
    /// writing nothing. Prints the settings changed, as `get` does.
    Set {
        #[arg(value_name = "KEY=VALUE", required_unless_present = "file")]
        assignments: Vec<String>,
        /// The change as a JSON object (`-` for the standard input), the
        /// assignments applied on top.
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Probe a harness's catalogue (models, efforts) again now.
    RefreshHarness {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// The agentic profiles of Settings › Agents.
    AgentProfile {
        #[command(subcommand)]
        action: AgentProfileAction,
    },
    /// The staging profiles of Settings › Sandbox.
    SandboxProfile {
        #[command(subcommand)]
        action: SandboxProfileAction,
    },
    /// The instance's provisioning rules: the files a new worktree receives.
    Provisioning {
        #[command(subcommand)]
        action: ProvisioningAction,
    },
    /// Your Apparence: Theme, Mode sombre, Palette daltonienne, Nodes compactes,
    /// Éléments masquables.
    Appearance {
        /// Replace it with the document of this file (`-` for the standard
        /// input), shaped as `--json` prints it under `appearance`.
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The OpenTelemetry export: its state, or a test export (`--test`).
    Telemetry {
        /// Export one test measure now and print the state after it.
        #[arg(long)]
        test: bool,
        #[arg(long)]
        json: bool,
    },
    /// Refresh the price table of the harnesses' models from its source.
    SyncPrices {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProvisioningAction {
    /// The instance's rules, one line per kind (`copy`, `hardlink`, `symlink`).
    Show {
        #[arg(long)]
        json: bool,
    },
    /// Replace the instance's rules with the JSON of a file (`-` for the
    /// standard input), shaped as `show --json` prints them.
    Set {
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// What a worktree of a repository would receive: one tab-separated line
    /// per path (mode, path, the level that brings it, its pattern), then
    /// the conflicts.
    Preview {
        /// The repository.
        #[arg(long)]
        repo: String,
        /// The ref its worktree starts from.
        #[arg(long, default_value = "HEAD")]
        git_ref: String,
        /// The Projet the Run is launched in, whose rules it inherits.
        #[arg(long)]
        project: Option<String>,
        /// Rules to preview instead of the stored instance rules (JSON file,
        /// `-` for the standard input).
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run_settings(action: SettingsAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        SettingsAction::Get { keys, json } => {
            let page = daemon.get("/settings")?.ok()?.json()?;
            let page = if keys.is_empty() {
                page
            } else {
                Value::Object(only(&page, &keys)?)
            };
            if json {
                print_json(&page);
            } else {
                print!("{}", knob_lines(&page, !keys.is_empty()));
            }
        }
        SettingsAction::Set {
            assignments,
            file,
            json,
        } => {
            let mut change = match file {
                Some(file) => match read_json_file(&file)? {
                    Value::Object(change) => change,
                    _ => anyhow::bail!("{file}: expected a JSON object of settings"),
                },
                None => Map::new(),
            };
            for raw in &assignments {
                let (key, value) = assignment(raw)?;
                change.insert(key, value);
            }
            anyhow::ensure!(!change.is_empty(), "nothing to change");
            let keys: Vec<String> = change.keys().cloned().collect();
            // The daemon ignores a field it does not know: a mistyped key is
            // refused here, against the page it serves, before anything is
            // written.
            only(&daemon.get("/settings")?.ok()?.json()?, &keys)?;
            let page = daemon
                .put("/settings", &Value::Object(change))?
                .ok()?
                .json()?;
            let changed = Value::Object(only(&page, &keys)?);
            if json {
                print_json(&changed);
            } else {
                // Each setting changed, whole: a tier such as `skills` has no
                // effective value to stand for it.
                print!("{}", knob_lines(&changed, true));
            }
        }
        SettingsAction::RefreshHarness { name, json } => {
            let entry = daemon
                .post(
                    &format!("/settings/harnesses/{}/refresh", segment(&name)),
                    None,
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&entry);
            } else {
                println!("{}", refreshed_line(&name, &entry));
            }
        }
        SettingsAction::AgentProfile { action } => agent_profiles::agent_profile(&daemon, action)?,
        SettingsAction::SandboxProfile { action } => {
            sandbox_profiles::sandbox_profile(&daemon, action)?
        }
        SettingsAction::Provisioning { action } => provisioning(&daemon, action)?,
        SettingsAction::Appearance { file, json } => {
            let answer = match file {
                Some(file) => daemon
                    .put("/actors/me/appearance", &read_json_file(&file)?)?
                    .ok()?
                    .json()?,
                None => daemon.get("/actors/me/appearance")?.ok()?.json()?,
            };
            if json {
                print_json(&answer);
            } else {
                print!("{}", render::appearance_lines(&answer["appearance"]));
            }
        }
        SettingsAction::Telemetry { test, json } => {
            let status = if test {
                daemon.post("/telemetry/test-export", None)?.ok()?.json()?
            } else {
                daemon.get("/telemetry/status")?.ok()?.json()?
            };
            if json {
                print_json(&status);
            } else {
                println!("{}", telemetry_line(&status));
            }
        }
        SettingsAction::SyncPrices { json } => {
            let report = daemon
                .post("/settings/cost-prices/sync", None)?
                .ok()?
                .json()?;
            if json {
                print_json(&report);
            } else {
                println!("{}", sync_line(&report));
            }
        }
    }
    Ok(())
}

/// The entries of `page` named by `keys`; an unknown key is refused, naming it.
fn only(page: &Value, keys: &[String]) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for key in keys {
        let value = page
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("no setting named `{key}` (see `pdo settings get`)"))?;
        out.insert(key.clone(), value.clone());
    }
    Ok(out)
}

fn provisioning(daemon: &Daemon, action: ProvisioningAction) -> Result<()> {
    match action {
        ProvisioningAction::Show { json } => {
            let rules = daemon.get("/settings/provisioning")?.ok()?.json()?;
            rules_out(&rules, json);
        }
        ProvisioningAction::Set { file, json } => {
            let rules = daemon
                .put("/settings/provisioning", &read_json_file(&file)?)?
                .ok()?
                .json()?;
            rules_out(&rules, json);
        }
        ProvisioningAction::Preview {
            repo,
            git_ref,
            project,
            file,
            json,
        } => {
            // The rules of the level previewed: the file's, else the stored
            // ones of the Projet, else the instance's.
            let rules = match (&file, &project) {
                (Some(file), _) => read_json_file(file)?,
                (None, Some(project)) => daemon
                    .get(&format!("/projects/{}/provisioning", segment(project)))?
                    .ok()?
                    .json()?,
                (None, None) => daemon.get("/settings/provisioning")?.ok()?.json()?,
            };
            let scope = if project.is_some() {
                "project"
            } else {
                "instance"
            };
            let plan = daemon
                .post(
                    "/repos/provisioning/preview",
                    Some(&json!({
                        "repository": absolute_path(&repo)?,
                        "git_ref": git_ref,
                        "scope": scope,
                        "rules": rules,
                        "project_id": project,
                    })),
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&plan);
            } else {
                print!("{}", plan_lines(&plan));
            }
        }
    }
    Ok(())
}

fn rules_out(rules: &Value, json: bool) {
    if json {
        print_json(rules);
    } else {
        print!("{}", rules_lines(rules));
    }
}
