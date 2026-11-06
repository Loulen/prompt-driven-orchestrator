//! `pdo run show|cancel|pause|resume|retry|reopen|archive|forget|rename|…`:
//! the gestures of the Runs list and of the run view, over `/runs/{id}*`
//! (what a Run shows: [`super::run_read`]). A thin client of the shared one
//! (`pdo_platform::adapters::cli::daemon`): who may, which state allows what,
//! stay behind the route; a refusal prints the daemon's sentence.

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use pdo_platform::adapters::cli::daemon::{print_json, read_text, segment, Answer, Daemon};
use serde_json::{json, Value};

use super::run_pipeline_cli::{run_run_pipeline, RunPipelineAction};
use super::run_render::node_lines;

/// The gestures on one Run, and what it shows. Exits **4** when the daemon
/// knows no such Run.
#[derive(Subcommand, Debug)]
pub enum RunGesture {
    /// The Run in a line (id, status, pipeline, name), then its nodes, one
    /// tab-separated line each: id, status, iteration.
    Show {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Give up on a live Run: no node is launched any more, every running
    /// node is stopped, the Run ends `halted` (« cancelled »). It can then be
    /// retried (`pdo run retry`) or reopened (`pdo run reopen`).
    Cancel {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Pause a running Run: nothing new is launched until `pdo run resume`.
    Pause {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Resume a paused, halted or failed Run.
    Resume {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Retry a finished Run whole: it is archived and a new Run starts with the
    /// same pipeline, input and repositories. Prints the new Run.
    Retry {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Reopen a finished Run: what was done stays done, the rest runs again.
    Reopen {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Archive a Run and its finished child Runs: their worktrees go, their
    /// history stays.
    Archive {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Forget an archived Run: it leaves the list for good.
    Forget {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Rename a Run; an empty name clears it.
    Rename {
        run_id: String,
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Add or remove secondary repositories of a running Run.
    Repos {
        run_id: String,
        /// A repository to add (path or URL), repeatable.
        #[arg(long)]
        add: Vec<String>,
        /// The repositories added by `--add` are read-only.
        #[arg(long, requires = "add")]
        read_only: bool,
        /// A secondary repository to remove, repeatable.
        #[arg(long)]
        remove: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Write an artifact into the Run's blackboard (a path under its artifacts).
    Inject {
        run_id: String,
        path: String,
        #[arg(long, conflicts_with = "file", required_unless_present = "file")]
        content: Option<String>,
        /// Read the content from a file, `-` for the standard input.
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Leave a node or a loop region through `--to`, past its conditions.
    ForceRoute {
        run_id: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        json: bool,
    },
    /// End a loop region, run it N more laps (`--bump`), or set its cap (`--max`).
    Region {
        run_id: String,
        region_id: String,
        #[arg(long, conflicts_with_all = ["bump", "max"], required_unless_present_any = ["bump", "max"])]
        end: bool,
        #[arg(long, conflicts_with = "max")]
        bump: Option<i64>,
        #[arg(long)]
        max: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Start or stop the Run's Pipeline Manager.
    Manager {
        #[arg(value_enum)]
        switch: Switch,
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// The Run's own pipeline: show, export, save, overwrite the shared one.
    Pipeline {
        #[command(subcommand)]
        action: RunPipelineAction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Switch {
    Start,
    Stop,
}

pub fn run_run_gesture(gesture: RunGesture) -> Result<()> {
    let daemon = Daemon::from_env();
    match gesture {
        RunGesture::Show { run_id, json } => {
            let run = daemon
                .get(&format!("/runs/{}", segment(&run_id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&run);
            } else {
                print!("{}", node_lines(&run));
            }
        }
        RunGesture::Cancel { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "cancel_run" }))?;
            said(&answer, json, &format!("{run_id} cancelled"));
        }
        RunGesture::Pause { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "pause_run" }))?;
            said(&answer, json, &format!("{run_id} paused"));
        }
        RunGesture::Resume { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "resume_run" }))?;
            said(&answer, json, &format!("{run_id} resumed"));
        }
        RunGesture::Retry { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "retry_all" }))?;
            let new = answer["run_id"].as_str().unwrap_or_default();
            said(&answer, json, &format!("{run_id} retried as {new}"));
        }
        RunGesture::Reopen { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "reopen_run" }))?;
            said(&answer, json, &format!("{run_id} reopened"));
        }
        RunGesture::Archive { run_id, json } => {
            let answer = command(&daemon, &run_id, json!({ "kind": "cleanup_run" }))?;
            let children = answer["archived_children"].as_array().map_or(0, Vec::len);
            let line = match children {
                0 => format!("{run_id} archived"),
                n => format!("{run_id} archived, with {n} child run(s)"),
            };
            said(&answer, json, &line);
        }
        RunGesture::Forget { run_id, json } => {
            let answer = value(daemon.delete(&format!("/runs/{}", segment(&run_id)), None)?)?;
            said(&answer, json, &format!("{run_id} forgotten"));
        }
        RunGesture::Rename { run_id, name, json } => {
            let answer = command(
                &daemon,
                &run_id,
                json!({ "kind": "rename_run", "name": name }),
            )?;
            let line = if name.is_empty() {
                format!("{run_id} name cleared")
            } else {
                format!("{run_id} renamed {name}")
            };
            said(&answer, json, &line);
        }
        RunGesture::Repos {
            run_id,
            add,
            read_only,
            remove,
            json,
        } => {
            if add.is_empty() && remove.is_empty() {
                anyhow::bail!("nothing to change: give --add or --remove");
            }
            let add = add
                .iter()
                .map(|repo| Ok(json!({ "repo": repo_arg(repo)?, "read_only": read_only })))
                .collect::<Result<Vec<_>>>()?;
            let remove = remove
                .iter()
                .map(|repo| repo_arg(repo))
                .collect::<Result<Vec<_>>>()?;
            let answer = value(daemon.patch(
                &format!("/runs/{}/repos", segment(&run_id)),
                &json!({ "add": add, "remove": remove }),
            )?)?;
            said(&answer, json, &format!("{run_id} repositories updated"));
        }
        RunGesture::Inject {
            run_id,
            path,
            content,
            file,
            json,
        } => {
            let content = match (content, file) {
                (Some(content), _) => content,
                (None, Some(file)) => read_text(&file)?,
                (None, None) => unreachable!("clap requires --content or --file"),
            };
            let body = json!({ "kind": "inject_artifact", "path": path, "content": content });
            let answer = command(&daemon, &run_id, body)?;
            said(&answer, json, &format!("{run_id} {path} written"));
        }
        RunGesture::ForceRoute {
            run_id,
            from,
            to,
            json,
        } => {
            let body = json!({ "kind": "force_route", "from": from, "target": to });
            let answer = command(&daemon, &run_id, body)?;
            said(
                &answer,
                json,
                &format!("{run_id} routed from {from} to {to}"),
            );
        }
        RunGesture::Region {
            run_id,
            region_id,
            end: _,
            bump,
            max,
            json,
        } => {
            let (body, line) = match (bump, max) {
                (Some(n), _) => (
                    json!({ "kind": "bump_region", "region_id": region_id, "additional_iter": n }),
                    format!("{region_id} runs {n} more lap(s)"),
                ),
                (None, Some(n)) => (
                    json!({ "kind": "set_region_max_iter", "region_id": region_id, "max_iter": n }),
                    format!("{region_id} capped at {n} lap(s)"),
                ),
                (None, None) => (
                    json!({ "kind": "end_region", "region_id": region_id }),
                    format!("{region_id} ended"),
                ),
            };
            let answer = command(&daemon, &run_id, body)?;
            said(&answer, json, &line);
        }
        RunGesture::Manager {
            switch,
            run_id,
            json,
        } => {
            let answer = match switch {
                Switch::Start => {
                    daemon.post(&format!("/runs/{}/manager/start", segment(&run_id)), None)?
                }
                Switch::Stop => {
                    daemon.post(&format!("/runs/{}/manager/stop", segment(&run_id)), None)?
                }
            };
            let answer = value(answer)?;
            let verb = match switch {
                Switch::Start => "started",
                Switch::Stop => "stopped",
            };
            said(&answer, json, &format!("{run_id} manager {verb}"));
        }
        RunGesture::Pipeline { action } => run_run_pipeline(&daemon, action)?,
    }
    Ok(())
}

/// One Pipeline Manager command on `run_id`, as the UI sends it.
pub(super) fn command(daemon: &Daemon, run_id: &str, body: Value) -> Result<Value> {
    value(daemon.post(&format!("/runs/{}/commands", segment(run_id)), Some(&body))?)
}

/// A successful answer as JSON; a plain-text body as a JSON string.
pub(super) fn value(answer: Answer) -> Result<Value> {
    let answer = answer.ok()?;
    Ok(answer
        .json()
        .unwrap_or_else(|_| Value::String(answer.body.trim().to_string())))
}

/// `--json`: the daemon's answer; else the line.
pub(super) fn said(answer: &Value, json: bool, line: &str) {
    if json {
        print_json(answer);
    } else {
        println!("{line}");
    }
}

/// `--json`: the daemon's answer; else its text rendering.
pub(super) fn shown(value: &Value, json: bool, text: fn(&Value) -> String) {
    if json {
        print_json(value);
    } else {
        print!("{}", text(value));
    }
}

/// A repository argument as the UI sends it: a URL as is, a path absolute.
fn repo_arg(raw: &str) -> Result<String> {
    if raw.contains("://") || raw.starts_with("git@") {
        return Ok(raw.to_string());
    }
    pdo_platform::adapters::cli::daemon::absolute_path(raw)
}
