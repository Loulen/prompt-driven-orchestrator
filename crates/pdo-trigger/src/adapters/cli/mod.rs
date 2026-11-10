//! `pdo trigger`: the Triggers of the running daemon, gesture for gesture with
//! the Triggers surface, over `/triggers*`. A thin client: the cron, the
//! guard, the overlap gate, who may, all stay behind the route.

mod body;
mod render;

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use pdo_platform::adapters::cli::daemon::{absolute_path, print_json, segment, Daemon};
use serde_json::{json, Value};

use body::template_body;
pub use body::{Overlap, TemplateFields};
use render::{done, fire_failed, fire_line, fire_lines, guard_text, list_lines, show_text};

#[derive(Subcommand, Debug)]
pub enum TriggerAction {
    /// List the Triggers, one tab-separated line each: id, state (`enabled`,
    /// `disabled`, `suspended`), cron, pipeline, name, next fire. While every
    /// Trigger is paused (`pause --all`), standard error says so.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one Trigger.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a Trigger: a Run template its cron fires, or `pdo trigger fire`.
    Create {
        name: String,
        /// The library pipeline it fires.
        #[arg(long)]
        pipeline: String,
        /// Its schedule, a 5-field cron expression (`0 3 * * *`).
        #[arg(long)]
        cron: String,
        #[command(flatten)]
        fields: TemplateFields,
        /// Fired Runs are not named automatically.
        #[arg(long)]
        no_auto_name: bool,
        #[arg(long)]
        json: bool,
    },
    /// Change a Trigger. Only the flags given change.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        pipeline: Option<String>,
        #[arg(long)]
        cron: Option<String>,
        #[command(flatten)]
        fields: TemplateFields,
        /// Name the fired Runs automatically (`on`) or not (`off`).
        #[arg(long, value_enum)]
        auto_name: Option<Switch>,
        /// Clear the guard: every due tick fires.
        #[arg(long, conflicts_with = "guard")]
        clear_guard: bool,
        /// Clear the concurrency ceiling of `--overlap allow`.
        #[arg(long, conflicts_with = "max_concurrent")]
        clear_max_concurrent: bool,
        /// Back to the instance's sandbox mode.
        #[arg(long, conflicts_with = "sandbox")]
        clear_sandbox: bool,
        /// Back to the instance's harness.
        #[arg(long, conflicts_with = "harness")]
        clear_harness: bool,
        /// Launch the fired Runs in no Project.
        #[arg(long, conflicts_with = "project")]
        clear_project: bool,
        /// Remove every skill the fired Runs carry.
        #[arg(long, conflicts_with = "skills")]
        clear_skills: bool,
        /// Back to the harness, then the instance, for the agentic profile.
        #[arg(long, conflicts_with_all = ["profile", "agent_harness"])]
        clear_agent: bool,
        /// Fire mono-repo Runs: no secondary repository.
        #[arg(long, conflicts_with_all = ["secondary_repos", "read_only_repos"])]
        clear_secondary_repos: bool,
        #[arg(long)]
        json: bool,
    },
    /// Fire a Trigger now. A guard or overlap skip is an answer, not an error;
    /// an error outcome (a refused Run, a guard that could not run) exits 1.
    Fire {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Disable a Trigger (it does not fire), or pause every Trigger with
    /// `--all` (the global switch, which leaves each one's state alone).
    #[command(visible_alias = "disable")]
    Pause {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        id: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Enable a disabled Trigger, or lift the global switch with `--all`.
    #[command(visible_alias = "enable")]
    Resume {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        id: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// The fires of a Trigger, newest first, one tab-separated line each:
    /// when, how (`manual`, `cron`), the outcome, the Run, the reason.
    Fires {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Run a guard command once, as a fire would, without firing anything.
    TestGuard {
        /// The guard command (`sh -c`); exit 0 fires, anything else skips.
        #[arg(long)]
        guard: String,
        /// The repository it runs in.
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The heartbeat: its last tick, its interval and the global pause.
    Health {
        #[arg(long)]
        json: bool,
    },
    /// Delete a Trigger.
    Delete {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    On,
    Off,
}

pub fn run_trigger(action: TriggerAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        TriggerAction::List { json } => {
            let triggers = daemon.get("/triggers")?.ok()?.json()?;
            if json {
                print_json(&triggers);
            } else {
                print!("{}", list_lines(&triggers));
                // The global switch, apart from each one's state: stdout stays one
                // line per Trigger.
                let paused = daemon
                    .get("/triggers/health")
                    .and_then(|r| r.ok()?.json())
                    .is_ok_and(|h| h["paused"] == true);
                if paused {
                    eprintln!("every Trigger is paused: pdo trigger resume --all");
                }
            }
        }
        TriggerAction::Show { id, json } => {
            let trigger = read(&daemon, &id)?;
            if json {
                print_json(&trigger);
            } else {
                print!("{}", show_text(&trigger));
            }
        }
        TriggerAction::Create {
            name,
            pipeline,
            cron,
            fields,
            no_auto_name,
            json,
        } => {
            let mut body = template_body(fields)?;
            body.insert("name".into(), json!(name));
            body.insert("pipeline_id".into(), json!(pipeline));
            body.insert("cron".into(), json!(cron));
            body.insert("auto_name".into(), json!(!no_auto_name));
            let trigger = daemon
                .post("/triggers", Some(&Value::Object(body)))?
                .ok()?
                .json()?;
            done(&trigger, json, "created");
        }
        TriggerAction::Update {
            id,
            name,
            pipeline,
            cron,
            fields,
            auto_name,
            clear_guard,
            clear_max_concurrent,
            clear_sandbox,
            clear_harness,
            clear_project,
            clear_skills,
            clear_agent,
            clear_secondary_repos,
            json,
        } => {
            let mut body = template_body(fields)?;
            for (key, value) in [("name", name), ("pipeline_id", pipeline), ("cron", cron)] {
                if let Some(value) = value {
                    body.insert(key.into(), json!(value));
                }
            }
            if let Some(auto_name) = auto_name {
                body.insert("auto_name".into(), json!(auto_name == Switch::On));
            }
            if clear_guard {
                // The route reads a present `null` guard as "leave it": an empty
                // command is the one that runs no guard.
                body.insert("guard_command".into(), json!(""));
            }
            for (key, clear) in [
                ("max_concurrent", clear_max_concurrent),
                ("sandbox", clear_sandbox),
                ("harness", clear_harness),
                ("project_id", clear_project),
                ("agent_choice", clear_agent),
                ("target_repos", clear_secondary_repos),
            ] {
                if clear {
                    body.insert(key.into(), Value::Null);
                }
            }
            if clear_skills {
                body.insert("skills".into(), json!([]));
            }
            if body.is_empty() {
                anyhow::bail!("nothing to change: give at least one flag (see --help)");
            }
            let trigger = patch(&daemon, &id, &Value::Object(body))?;
            done(&trigger, json, "updated");
        }
        TriggerAction::Fire { id, json } => {
            let answer = daemon
                .post(&format!("/triggers/{}/fire", segment(&id)), None)?
                .ok()?
                .json()?;
            if json {
                print_json(&answer);
            }
            // An error outcome fails the command (exit 1, its line on stderr);
            // a skip is an answer.
            if fire_failed(&answer) {
                anyhow::bail!("{}", fire_line(&id, &answer));
            }
            if !json {
                println!("{}", fire_line(&id, &answer));
            }
        }
        TriggerAction::Pause { id, all, json } => switch(&daemon, id, all, true, json)?,
        TriggerAction::Resume { id, all, json } => switch(&daemon, id, all, false, json)?,
        TriggerAction::Fires { id, json } => {
            let fires = daemon
                .get(&format!("/triggers/{}/fires", segment(&id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&fires);
            } else {
                print!("{}", fire_lines(&fires));
            }
        }
        TriggerAction::TestGuard { guard, repo, json } => {
            let repo = repo.as_deref().map(absolute_path).transpose()?;
            let result = daemon
                .post(
                    "/triggers/guard/test",
                    Some(&json!({ "guard_command": guard, "target_repo": repo })),
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&result);
            } else {
                print!("{}", guard_text(&result));
            }
        }
        TriggerAction::Health { json } => {
            let health = daemon.get("/triggers/health")?.ok()?.json()?;
            if json {
                print_json(&health);
            } else {
                println!(
                    "last tick {} · every {}s · {}",
                    health["last_tick_at"].as_str().unwrap_or("never"),
                    health["tick_interval_secs"],
                    if health["paused"] == true {
                        "every Trigger paused"
                    } else {
                        "running"
                    }
                );
            }
        }
        TriggerAction::Delete { id, json } => {
            daemon
                .delete(&format!("/triggers/{}", segment(&id)), None)?
                .ok()?;
            if json {
                print_json(&json!({ "id": id, "deleted": true }));
            } else {
                println!("{id} deleted");
            }
        }
    }
    Ok(())
}

fn read(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/triggers/{}", segment(id)))?
        .ok()?
        .json()
}

fn patch(daemon: &Daemon, id: &str, body: &Value) -> Result<Value> {
    daemon
        .patch(&format!("/triggers/{}", segment(id)), body)?
        .ok()?
        .json()
}

/// `pause`/`resume`: one Trigger's `enabled`, or the global switch.
fn switch(daemon: &Daemon, id: Option<String>, all: bool, pause: bool, json: bool) -> Result<()> {
    if all {
        let answer = daemon
            .post("/triggers/pause", Some(&json!({ "paused": pause })))?
            .ok()?
            .json()?;
        if json {
            print_json(&answer);
        } else if pause {
            println!("every Trigger paused");
        } else {
            println!("Triggers resumed");
        }
        return Ok(());
    }
    let id = id.context("a Trigger id, or --all")?;
    let trigger = patch(daemon, &id, &json!({ "enabled": !pause }))?;
    // One Trigger is disabled or enabled, as the list says it: « paused » is
    // the global switch's word (CORE-232).
    done(&trigger, json, if pause { "disabled" } else { "enabled" });
    Ok(())
}
