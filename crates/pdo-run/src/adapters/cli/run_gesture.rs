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
use super::run_repos::edit_repos;

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
    /// End a loop region, run it N more laps (`--bump`), set its lap cap
    /// (`--max`) or its cost cap in dollars (`--max-cost`, `none` removes it).
    Region {
        run_id: String,
        region_id: String,
        #[arg(
            long,
            conflicts_with_all = ["bump", "max", "max_cost"],
            required_unless_present_any = ["bump", "max", "max_cost"]
        )]
        end: bool,
        #[arg(long, conflicts_with_all = ["max", "max_cost"])]
        bump: Option<i64>,
        #[arg(long, conflicts_with = "max_cost")]
        max: Option<i64>,
        /// The loop's cost cap in dollars, `none` to remove it. Applies live:
        /// under what the loop already spent, it stops at its next re-entry.
        #[arg(long, value_name = "USD|none", value_parser = cost_cap)]
        max_cost: Option<CostCap>,
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
            let answer = edit_repos(&daemon, &run_id, &add, read_only, &remove)?;
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
            max_cost,
            json,
        } => {
            let (body, line) = match (bump, max, max_cost) {
                (_, _, Some(CostCap(usd))) => (
                    json!({ "kind": "set_region_max_cost", "region_id": region_id, "max_cost_usd": usd }),
                    match usd {
                        Some(usd) => format!("{region_id} cost capped at ${usd}"),
                        None => format!("{region_id} cost cap removed"),
                    },
                ),
                (Some(n), _, _) => (
                    json!({ "kind": "bump_region", "region_id": region_id, "additional_iter": n }),
                    format!("{region_id} runs {n} more lap(s)"),
                ),
                (None, Some(n), _) => (
                    json!({ "kind": "set_region_max_iter", "region_id": region_id, "max_iter": n }),
                    format!("{region_id} capped at {n} lap(s)"),
                ),
                (None, None, None) => (
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

/// A `--max-cost` value: a dollar amount, or `None` for `none` (remove the cap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostCap(pub Option<f64>);

/// `--max-cost`'s parser: `none`, or a number. Whether the amount is above 0
/// is the daemon's call, so the CLI answers its words.
fn cost_cap(raw: &str) -> std::result::Result<CostCap, String> {
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("none") {
        return Ok(CostCap(None));
    }
    raw.trim_start_matches('$')
        .parse::<f64>()
        .ok()
        .filter(|usd| usd.is_finite())
        .map(|usd| CostCap(Some(usd)))
        .ok_or_else(|| format!("`{raw}` is not an amount in dollars nor `none`"))
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
        .unwrap_or_else(|_| Value::String(answer.text().trim().to_string())))
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
