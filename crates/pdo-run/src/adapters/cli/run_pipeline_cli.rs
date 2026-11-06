//! `pdo run pipeline show|export|save|overwrite-default`: a Run's own
//! pipeline, as the run view's editor reads, edits and copies it over the
//! shared one (ADR-0085), over `/runs/{id}/pipeline*`.

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, read_json_file, segment, Daemon};
use serde_json::{json, Value};

use super::run_gesture::{said, value};
use super::run_render::write_bytes;
use pdo_platform::adapters::cli::daemon::read_text;

#[derive(Subcommand, Debug)]
pub enum RunPipelineAction {
    /// The Run's pipeline: its YAML (text), or with `--json` the YAML, the
    /// parsed pipeline, its prompts and its diagnostics.
    Show {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// The Run's pipeline as a portable document (YAML with its prompts),
    /// written to `--output`, else to stdout. `--skills <zip>` also writes the
    /// skills sidecar, when the pipeline carries skills.
    Export {
        run_id: String,
        #[arg(long, short)]
        output: Option<std::path::PathBuf>,
        #[arg(long, value_name = "ZIP")]
        skills: Option<std::path::PathBuf>,
    },
    /// Edit the Run's pipeline mid-run, within what its progress allows. Give
    /// the YAML (`--yaml`, the prompts stay), or the `{yaml, prompts}` object
    /// `show --json` prints (`--file`).
    Save {
        run_id: String,
        #[arg(long, conflicts_with = "file", required_unless_present = "file")]
        yaml: Option<String>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Copy the Run's pipeline over the shared pipeline it was launched from.
    /// `--preview` only says what it would touch (whether the shared one
    /// changed since the launch, the Triggers that fire it).
    OverwriteDefault {
        run_id: String,
        #[arg(long)]
        preview: bool,
        #[arg(long)]
        json: bool,
    },
}

pub(super) fn run_run_pipeline(daemon: &Daemon, action: RunPipelineAction) -> Result<()> {
    match action {
        RunPipelineAction::Show { run_id, json } => {
            let detail = read(daemon, &run_id)?;
            if json {
                print_json(&detail);
            } else {
                print!("{}", detail["yaml"].as_str().unwrap_or_default());
            }
        }
        RunPipelineAction::Export {
            run_id,
            output,
            skills,
        } => {
            let document =
                daemon.get_bytes(&format!("/runs/{}/pipeline/document", segment(&run_id)))?;
            write_bytes(&document, output.as_deref())?;
            if let Some(zip) = skills {
                let sidecar = daemon.get_bytes(&format!(
                    "/runs/{}/pipeline/document/skills",
                    segment(&run_id)
                ))?;
                if sidecar.is_empty() {
                    eprintln!("no skills sidecar: the pipeline carries no skill");
                } else {
                    std::fs::write(&zip, sidecar)
                        .with_context(|| format!("cannot write {}", zip.display()))?;
                }
            }
        }
        RunPipelineAction::Save {
            run_id,
            yaml,
            file,
            json,
        } => {
            let body = match (yaml, file) {
                (Some(yaml), _) => {
                    let current = read(daemon, &run_id)?;
                    json!({ "yaml": read_text(&yaml)?, "prompts": current["prompts"] })
                }
                (None, Some(file)) => {
                    let given = read_json_file(&file)?;
                    json!({ "yaml": given["yaml"], "prompts": given["prompts"] })
                }
                (None, None) => unreachable!("clap requires --yaml or --file"),
            };
            let answer =
                value(daemon.put(&format!("/runs/{}/pipeline", segment(&run_id)), &body)?)?;
            said(&answer, json, &format!("{run_id} pipeline saved"));
        }
        RunPipelineAction::OverwriteDefault {
            run_id,
            preview,
            json,
        } => {
            if preview {
                let answer = daemon
                    .get(&format!(
                        "/runs/{}/pipeline/overwrite-preview",
                        segment(&run_id)
                    ))?
                    .ok()?
                    .json()?;
                said(&answer, json, &preview_line(&answer));
            } else {
                let answer = value(daemon.post(
                    &format!("/runs/{}/pipeline/overwrite-default", segment(&run_id)),
                    None,
                )?)?;
                let pipeline = answer["pipeline_id"].as_str().unwrap_or_default();
                said(
                    &answer,
                    json,
                    &format!("{pipeline} overwritten by {run_id}"),
                );
            }
        }
    }
    Ok(())
}

fn read(daemon: &Daemon, run_id: &str) -> Result<Value> {
    daemon
        .get(&format!("/runs/{}/pipeline", segment(run_id)))?
        .ok()?
        .json()
}

/// What an overwrite would touch, in a line.
fn preview_line(preview: &Value) -> String {
    let id = preview["pipeline_id"].as_str().unwrap_or_default();
    if preview["pipeline_exists"] != true {
        return format!("{id}: no such pipeline any more, nothing to overwrite");
    }
    let modified = match preview["modified_since_launch"].as_bool() {
        Some(true) => "changed since the launch",
        Some(false) => "unchanged since the launch",
        None => "launch version unknown",
    };
    let triggers = preview["triggers"].as_array().map_or(0, Vec::len);
    format!("{id}: {modified} · fired by {triggers} Trigger(s)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_says_what_the_overwrite_touches() {
        let preview = json!({
            "pipeline_id": "solo", "pipeline_exists": true,
            "modified_since_launch": true, "triggers": [{"id": "t1"}]
        });
        assert_eq!(
            preview_line(&preview),
            "solo: changed since the launch · fired by 1 Trigger(s)"
        );
        let gone = json!({ "pipeline_id": "solo", "pipeline_exists": false });
        assert_eq!(
            preview_line(&gone),
            "solo: no such pipeline any more, nothing to overwrite"
        );
        let unknown = json!({
            "pipeline_id": "solo", "pipeline_exists": true,
            "modified_since_launch": null, "triggers": []
        });
        assert!(preview_line(&unknown).contains("launch version unknown"));
    }
}
