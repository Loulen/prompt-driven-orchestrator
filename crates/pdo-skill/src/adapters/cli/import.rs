//! `pdo skill scan|import` and `pdo skill folder`: a Source's skills into the
//! bank, and the folders they live in. The import is the dialog's two steps
//! in one command: the scan, then the import of the candidates it checks by
//! default (the valid, new ones; a name the bank holds as `--on-collision`
//! says).

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use serde_json::{json, Map, Value};

use super::render::{entry_lines, folder_lines};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Collision {
    /// Leave the bank's skill as it is.
    Skip,
    /// Overwrite the bank's skill of that name (its id kept).
    Replace,
}

#[derive(Subcommand, Debug)]
pub enum FolderAction {
    /// The folders, one tab-separated line each: id, name, parent, Source.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create a folder.
    Add {
        name: String,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rename a folder or move it. Only the flags given change.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "root")]
        parent: Option<String>,
        /// Move it to the root of the bank.
        #[arg(long)]
        root: bool,
        #[arg(long)]
        json: bool,
    },
    /// Delete a folder and what it holds.
    Remove {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Compare a folder imported from a Source with the Source now: one line
    /// per skill (status, path, name). Changes nothing.
    Rescan {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Bring a folder imported from a Source up to date: the skills changed
    /// at the Source (`updated`), plus those new at the Source with `--new`,
    /// or only the paths given with `--only`.
    Sync {
        id: String,
        /// Also import the skills new at the Source.
        #[arg(long)]
        new: bool,
        /// Only this path (repeatable).
        #[arg(long = "only", value_name = "PATH")]
        only: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

/// Where imported skills go.
pub(super) struct Destination {
    pub folder: Option<String>,
    pub folder_name: Option<String>,
    pub parent: Option<String>,
}

fn new_scan_id(given: Option<String>) -> String {
    given.unwrap_or_else(|| format!("cli-{}", uuid::Uuid::new_v4()))
}

pub(super) fn scan(daemon: &Daemon, source: &str, scan_id: Option<String>) -> Result<Value> {
    let scan_id = new_scan_id(scan_id);
    daemon
        .post(
            "/settings/skills/scan",
            Some(&json!({ "scan_id": scan_id, "source": source })),
        )?
        .ok()?
        .json()
}

/// What the import dialog checks by default, as items: a new valid candidate
/// is imported, a name the bank holds follows `collision`, the rest skipped.
/// `only`, when given, narrows the import to those paths.
pub(super) fn items(report: &Value, only: &[String], collision: Collision) -> Vec<Value> {
    report["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| only.is_empty() || only.iter().any(|p| c["path"] == p.as_str()))
        .filter_map(|c| {
            let action = match (c["status"].as_str(), collision) {
                (Some("new"), _) => "import",
                (Some("name_taken"), Collision::Replace) => "replace",
                _ => return None,
            };
            Some(json!({ "path": c["path"], "action": action }))
        })
        .collect()
}

pub(super) fn import(
    daemon: &Daemon,
    source: &str,
    scan_id: Option<String>,
    only: &[String],
    collision: Collision,
    destination: Destination,
) -> Result<Value> {
    let report = scan(daemon, source, scan_id)?;
    let items = items(&report, only, collision);
    anyhow::ensure!(
        !items.is_empty(),
        "nothing to import from {source}: no new valid skill{}",
        if collision == Collision::Skip {
            " (a name the bank holds is skipped: --on-collision replace)"
        } else {
            ""
        }
    );
    let mut folder = Map::new();
    if let Some(id) = destination.folder {
        folder.insert("id".into(), json!(id));
    }
    if let Some(name) = destination.folder_name {
        folder.insert("name".into(), json!(name));
    }
    if let Some(parent) = destination.parent {
        folder.insert("parent_id".into(), json!(parent));
    }
    daemon
        .post(
            "/settings/skills/import",
            Some(&json!({
                "scan_id": report["scan_id"],
                "source": source,
                "folder": if folder.is_empty() { Value::Null } else { Value::Object(folder) },
                "items": items,
            })),
        )?
        .ok()?
        .json()
}

/// The entries of a rescan to apply: those changed at the Source, plus the
/// new ones with `new`; or only the paths given.
pub(super) fn sync_items(rescan: &Value, new: bool, only: &[String]) -> Vec<Value> {
    rescan["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let action = match entry["status"].as_str() {
                Some("updated") => "update",
                Some("new") => "import",
                _ => return None,
            };
            let chosen = if only.is_empty() {
                action == "update" || new
            } else {
                only.iter().any(|p| entry["path"] == p.as_str())
            };
            chosen.then(|| json!({ "path": entry["path"], "action": action }))
        })
        .collect()
}

pub(super) fn folder(daemon: &Daemon, action: FolderAction) -> Result<()> {
    match action {
        FolderAction::List { json } => {
            let folders = daemon.get("/settings/skill-folders")?.ok()?.json()?;
            if json {
                print_json(&folders);
            } else {
                print!("{}", folder_lines(&folders));
            }
        }
        FolderAction::Add { name, parent, json } => {
            let folder = daemon
                .post(
                    "/settings/skill-folders",
                    Some(&json!({ "name": name, "parent_id": parent })),
                )?
                .ok()?
                .json()?;
            done(&folder, json, "added");
        }
        FolderAction::Update {
            id,
            name,
            parent,
            root,
            json,
        } => {
            let mut body = Map::new();
            if let Some(name) = name {
                body.insert("name".into(), json!(name));
            }
            if let Some(parent) = parent {
                body.insert("parent_id".into(), json!(parent));
            } else if root {
                body.insert("parent_id".into(), Value::Null);
            }
            anyhow::ensure!(
                !body.is_empty(),
                "nothing to change: give at least one flag (see --help)"
            );
            let folder = daemon
                .put(
                    &format!("/settings/skill-folders/{}", segment(&id)),
                    &Value::Object(body),
                )?
                .ok()?
                .json()?;
            done(&folder, json, "updated");
        }
        FolderAction::Remove { id, json } => {
            daemon
                .delete(&format!("/settings/skill-folders/{}", segment(&id)), None)?
                .ok()?;
            if json {
                print_json(&json!({ "id": id, "deleted": true }));
            } else {
                println!("{id} removed");
            }
        }
        FolderAction::Rescan { id, json } => {
            let rescan = rescan(daemon, &id)?;
            if json {
                print_json(&rescan);
            } else {
                print!("{}", entry_lines(&rescan));
            }
        }
        FolderAction::Sync {
            id,
            new,
            only,
            json,
        } => {
            let rescan = rescan(daemon, &id)?;
            let items = sync_items(&rescan, new, &only);
            if items.is_empty() {
                if json {
                    print_json(&json!({ "rescan": rescan, "applied": [] }));
                } else {
                    println!("{id} is up to date with its Source");
                }
                return Ok(());
            }
            let report = daemon
                .post(
                    &format!("/settings/skill-folders/{}/update", segment(&id)),
                    Some(&json!({ "scan_id": rescan["scan_id"], "items": items })),
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&report);
            } else {
                println!("{id} synced: {} skill(s)", items.len());
            }
        }
    }
    Ok(())
}

fn rescan(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .post(
            &format!("/settings/skill-folders/{}/rescan", segment(id)),
            Some(&json!({ "scan_id": new_scan_id(None) })),
        )?
        .ok()?
        .json()
}

fn done(folder: &Value, json: bool, verb: &str) {
    if json {
        print_json(folder);
    } else {
        println!(
            "{}\t{} {verb}",
            folder["id"].as_str().unwrap_or(""),
            folder["name"].as_str().unwrap_or("")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Value {
        json!({ "scan_id": "s", "candidates": [
            { "path": "a", "status": "new" },
            { "path": "b", "status": "name_taken" },
            { "path": "c", "status": "same_commit" },
            { "path": "d", "status": "invalid" },
        ]})
    }

    #[test]
    fn the_import_takes_what_the_dialog_checks_by_default() {
        assert_eq!(
            items(&report(), &[], Collision::Skip),
            vec![json!({ "path": "a", "action": "import" })]
        );
        assert_eq!(
            items(&report(), &[], Collision::Replace),
            vec![
                json!({ "path": "a", "action": "import" }),
                json!({ "path": "b", "action": "replace" })
            ]
        );
        assert!(items(&report(), &["c".into()], Collision::Replace).is_empty());
    }

    #[test]
    fn a_sync_applies_the_updated_then_the_new_on_request() {
        let rescan = json!({ "entries": [
            { "path": "a", "status": "updated" },
            { "path": "b", "status": "new" },
            { "path": "c", "status": "gone" },
        ]});
        assert_eq!(
            sync_items(&rescan, false, &[]),
            vec![json!({ "path": "a", "action": "update" })]
        );
        assert_eq!(sync_items(&rescan, true, &[]).len(), 2);
        assert_eq!(
            sync_items(&rescan, false, &["b".into()]),
            vec![json!({ "path": "b", "action": "import" })]
        );
    }
}
