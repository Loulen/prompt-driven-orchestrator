//! `pdo pipeline library`: the node library of the running daemon, over
//! `/library*`, and the import of a Claude Code workflow.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use serde_json::json;

use super::body::{library_entry, workflow_body};
use super::render::{answer, library_lines, node_yaml, warn};
use pdo_platform::adapters::cli::daemon::read_text;

#[derive(Subcommand, Debug)]
pub enum LibraryAction {
    /// List the library's entries, one tab-separated line each: name, type,
    /// model, effort (`-` for the account default).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Star a node into the library, from the entry the editor sends: a JSON
    /// (or YAML) file with `name`, `type`, `inputs`, `outputs`, `prompt`… (`-`
    /// for the standard input). An entry of the same name is replaced.
    Save {
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a library entry.
    Delete {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// The node an entry instantiates on a canvas, as YAML (its spec and its
    /// prompt).
    Instantiate {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Import a Claude Code workflow (`.js`) as a new pipeline.
    Import {
        file: String,
        #[arg(long)]
        json: bool,
    },
}

pub(super) fn run_library(daemon: &Daemon, action: LibraryAction) -> Result<()> {
    match action {
        LibraryAction::List { json } => {
            let entries = daemon.get("/library")?.ok()?.json()?;
            if json {
                print_json(&entries);
            } else {
                print!("{}", library_lines(&entries));
            }
        }
        LibraryAction::Save { file, json } => {
            let entry = library_entry(&read_text(&file)?)?;
            let saved = daemon.post("/library", Some(&entry))?.ok()?.json()?;
            answer(&saved, json, |s| {
                format!("{} saved", s["name"].as_str().unwrap_or_default())
            });
        }
        LibraryAction::Delete { name, json } => {
            daemon
                .delete(&format!("/library/{}", segment(&name)), None)?
                .ok()?;
            answer(&json!({ "name": &name, "deleted": true }), json, |_| {
                format!("{name} deleted")
            });
        }
        LibraryAction::Instantiate { name, json } => {
            let node = daemon
                .post(&format!("/library/{}/instantiate", segment(&name)), None)?
                .ok()?
                .json()?;
            if json {
                print_json(&node);
            } else {
                print!("{}", node_yaml(&node));
            }
        }
        LibraryAction::Import { file, json } => {
            let body = workflow_body(&file, read_text(&file)?);
            let imported = daemon.post("/library/import", Some(&body))?.ok()?.json()?;
            if !json {
                warn(&imported);
            }
            answer(&imported, json, |i| {
                format!("{} imported", i["id"].as_str().unwrap_or_default())
            });
        }
    }
    Ok(())
}
