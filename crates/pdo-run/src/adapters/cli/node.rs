//! `pdo node`: the gestures on one node of a Run, as the run view's node
//! panel and the Pipeline Manager make them (start, stop, retry, skip,
//! restart, kill, recover, complete, release, extend a cycle, import files),
//! and what the panel reads (prompt, inputs and outputs, diff, transcript).
//! Over `/runs/{id}/nodes/{node}*` and `/runs/{id}/commands`; the rules stay
//! behind the routes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};

use super::node_read::{run_node_read, NodeRead};
use super::run_gesture::{command, said, value};
use super::run_render::retry_preview_line;

#[derive(Subcommand, Debug)]
pub enum NodeAction {
    /// Retry a node: what it fed downstream is invalidated and it runs again,
    /// reopening a finished Run. `--preview` only says what would be invalidated.
    Retry {
        run_id: String,
        node_id: String,
        #[arg(long)]
        preview: bool,
        #[arg(long)]
        json: bool,
    },
    /// Skip a node: it counts as done with empty outputs (or those of
    /// `--output port=content`) and the Run goes on.
    Skip {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        /// An output written in place of the node's, repeatable.
        #[arg(long = "output", value_name = "PORT=CONTENT")]
        outputs: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Restart an iteration of a node from scratch.
    Restart {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Start a node that is not running yet (the canvas's Start).
    Start {
        run_id: String,
        node_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Stop a running node; it is `stopped`, never failed (the canvas's Stop).
    Stop {
        run_id: String,
        node_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Kill an iteration of a node: it fails.
    Kill {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Recover an interrupted node: resumed in place when its harness can,
    /// else restarted with its artifacts.
    Recover {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Mark an iteration of a node done, as its agent's `pdo complete` would.
    Done {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Release the completion an interactive node holds for its user.
    Release {
        run_id: String,
        node_id: String,
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Run a cycle node N more iterations.
    Extend {
        run_id: String,
        node_id: String,
        additional_iter: i64,
        #[arg(long)]
        json: bool,
    },
    /// Import files into a node that holds a live session.
    Attach {
        run_id: String,
        node_id: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// What the node panel shows (prompt, inputs and outputs, diff,
    /// transcript).
    #[command(flatten)]
    Read(NodeRead),
}

pub fn run_node(action: NodeAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        NodeAction::Retry {
            run_id,
            node_id,
            preview,
            json,
        } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            if preview {
                let preview = daemon
                    .get(&format!("/runs/{r}/nodes/{n}/retry/preview"))?
                    .ok()?
                    .json()?;
                if json {
                    print_json(&preview);
                } else {
                    print!("{}", retry_preview_line(&node_id, &preview));
                }
            } else {
                let answer = value(daemon.post(&format!("/runs/{r}/nodes/{n}/retry"), None)?)?;
                said(&answer, json, &format!("{node_id} retried"));
            }
        }
        NodeAction::Skip {
            run_id,
            node_id,
            iter,
            outputs,
            json,
        } => {
            let iter = iteration(&daemon, &run_id, &node_id, iter)?;
            let mut body = json!({ "kind": "skip_node", "node_id": node_id, "iter": iter });
            if !outputs.is_empty() {
                body["overrides"] = json!(port_contents(&outputs)?);
            }
            let answer = command(&daemon, &run_id, body)?;
            said(&answer, json, &format!("{node_id} skipped"));
        }
        NodeAction::Restart {
            run_id,
            node_id,
            iter,
            json,
        } => node_command(
            &daemon,
            &run_id,
            "restart_node",
            &node_id,
            iter,
            "restarted",
            json,
        )?,
        NodeAction::Start {
            run_id,
            node_id,
            json,
        } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            let answer = value(daemon.post(&format!("/runs/{r}/nodes/{n}/start"), None)?)?;
            said(&answer, json, &format!("{node_id} started"));
        }
        NodeAction::Stop {
            run_id,
            node_id,
            json,
        } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            let answer = value(daemon.post(&format!("/runs/{r}/nodes/{n}/stop"), None)?)?;
            said(&answer, json, &format!("{node_id} stopped"));
        }
        NodeAction::Kill {
            run_id,
            node_id,
            iter,
            json,
        } => node_command(
            &daemon,
            &run_id,
            "kill_node",
            &node_id,
            iter,
            "killed",
            json,
        )?,
        NodeAction::Recover {
            run_id,
            node_id,
            iter,
            json,
        } => node_command(
            &daemon,
            &run_id,
            "recover_node",
            &node_id,
            iter,
            "recovered",
            json,
        )?,
        NodeAction::Done {
            run_id,
            node_id,
            iter,
            json,
        } => node_command(
            &daemon,
            &run_id,
            "mark_node_done",
            &node_id,
            iter,
            "done",
            json,
        )?,
        NodeAction::Release {
            run_id,
            node_id,
            iter,
            json,
        } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            let answer = value(daemon.post(
                &format!("/runs/{r}/nodes/{n}/release"),
                Some(&json!({ "iter": iter })),
            )?)?;
            said(&answer, json, &format!("{node_id} released"));
        }
        NodeAction::Extend {
            run_id,
            node_id,
            additional_iter,
            json,
        } => {
            let body = json!({
                "kind": "extend_cycle", "node_id": node_id, "additional_iter": additional_iter
            });
            let answer = command(&daemon, &run_id, body)?;
            said(
                &answer,
                json,
                &format!("{node_id} runs {additional_iter} more iteration(s)"),
            );
        }
        NodeAction::Attach {
            run_id,
            node_id,
            files,
            iter,
            json,
        } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            let mut form = Form::new();
            for file in &files {
                let bytes = std::fs::read(file)
                    .with_context(|| format!("cannot read {}", file.display()))?;
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                form = form.part("files", Part::bytes(bytes).file_name(name));
            }
            let url = match iter {
                Some(i) => format!("/runs/{r}/nodes/{n}/attachments?iter={i}"),
                None => format!("/runs/{r}/nodes/{n}/attachments"),
            };
            let answer = value(daemon.post_multipart(&url, form)?)?;
            let imported: Vec<String> = answer["files"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|f| f["path"].as_str().unwrap_or_default().to_string())
                .collect();
            said(
                &answer,
                json,
                &format!("{node_id} received {}", imported.join(", ")),
            );
        }
        NodeAction::Read(read) => run_node_read(&daemon, read)?,
    }
    Ok(())
}

/// A Pipeline Manager command on one iteration of a node.
fn node_command(
    daemon: &Daemon,
    run_id: &str,
    kind: &str,
    node_id: &str,
    iter: Option<i64>,
    verb: &str,
    json: bool,
) -> Result<()> {
    let iter = iteration(daemon, run_id, node_id, iter)?;
    let body = json!({ "kind": kind, "node_id": node_id, "iter": iter });
    let answer: Value = command(daemon, run_id, body)?;
    said(&answer, json, &format!("{node_id} {verb}"));
    Ok(())
}

/// The iteration a gesture targets: the one given, else the node's latest,
/// the one the node panel shows first (`1` for a node not started yet).
pub(super) fn iteration(
    daemon: &Daemon,
    run_id: &str,
    node_id: &str,
    given: Option<i64>,
) -> Result<i64> {
    if let Some(iter) = given {
        return Ok(iter);
    }
    let run = daemon
        .get(&format!("/runs/{}", segment(run_id)))?
        .ok()?
        .json()?;
    Ok(run["nodes"][node_id]["iter"].as_i64().unwrap_or(1))
}

/// `port=content` pairs, as the skip's overrides.
fn port_contents(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    pairs
        .iter()
        .map(|pair| {
            let (port, content) = pair
                .split_once('=')
                .with_context(|| format!("--output {pair}: expected PORT=CONTENT"))?;
            Ok((port.to_string(), content.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_is_a_port_and_its_content() {
        let outputs = port_contents(&["out=done".into(), "notes=a=b".into()]).unwrap();
        assert_eq!(outputs["out"], "done");
        assert_eq!(outputs["notes"], "a=b", "only the first `=` splits");
        assert!(port_contents(&["nothing".into()]).is_err());
    }
}
