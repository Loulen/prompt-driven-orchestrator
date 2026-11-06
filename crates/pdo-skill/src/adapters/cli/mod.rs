//! `pdo skill`: the Banque de skills of the running daemon, gesture for
//! gesture with Settings › Skills, over `/settings/skills*` and
//! `/settings/skill-folders*`. A thin client: a locked skill (one an extension
//! provides) is refused by the daemon, as in the UI, and so is a duplicate
//! name or an invalid `SKILL.md`.

mod import;
mod render;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{
    print_json, read_file_bytes, referent_lines, segment, Daemon,
};
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};

pub use import::{Collision, FolderAction};
use render::{file_text, list_lines, show_text};

#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// The bank, one tab-separated line per skill: id, name, folder, and
    /// `locked` for a skill an extension provides.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a skill: name, description, folder, files, then its `SKILL.md`.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Add a skill from its `SKILL.md` (a file, `-` for the standard input),
    /// frontmatter included.
    Add {
        file: String,
        /// The bank label; the frontmatter `name` by default.
        #[arg(long)]
        name: Option<String>,
        /// The folder it goes to; the root by default.
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rename a skill, move it, or replace its `SKILL.md`. Only the flags
    /// given change.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "root")]
        folder: Option<String>,
        /// Move it to the root of the bank.
        #[arg(long)]
        root: bool,
        /// Its new `SKILL.md` (a file, `-` for the standard input).
        #[arg(long, value_name = "FILE")]
        content: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a skill from the bank. Who still selects it keeps the
    /// selection (see `referents`).
    Remove {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Who selects the skill: the instance, Projets, Triggers, pipeline
    /// nodes, Runs not started yet.
    Referents {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The reference files of a skill, besides its `SKILL.md`.
    File {
        #[command(subcommand)]
        action: FileAction,
    },
    /// The folders of the bank.
    Folder {
        #[command(subcommand)]
        action: FolderAction,
    },
    /// Scan a Source (a git URL or a local repository) for skills, without
    /// importing anything: one line per candidate (status, path, name).
    Scan {
        source: String,
        /// The scan's id, for `cancel-scan` from another shell.
        #[arg(long)]
        scan_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stop a scan in flight.
    CancelScan {
        scan_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Import the skills of a Source: scan it, then import its valid
    /// candidates into a folder (a new one named after the Source by
    /// default).
    Import {
        source: String,
        /// Only this candidate path (repeatable).
        #[arg(long = "only", value_name = "PATH")]
        only: Vec<String>,
        /// A skill whose name the bank already holds: `skip` it (default) or
        /// `replace` the bank's content.
        #[arg(long, value_enum, default_value_t = Collision::Skip)]
        on_collision: Collision,
        /// Import into this existing folder.
        #[arg(long, conflicts_with = "folder_name")]
        folder: Option<String>,
        /// Import into a new folder of this name.
        #[arg(long)]
        folder_name: Option<String>,
        /// The new folder's parent.
        #[arg(long, requires = "folder_name")]
        parent: Option<String>,
        #[arg(long)]
        scan_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The Sources imported lately.
    Sources {
        #[arg(long)]
        json: bool,
    },
    /// Export skills and folders (their sub-folders included) as a zip.
    Export {
        #[arg(long = "skill", value_name = "ID")]
        skills: Vec<String>,
        #[arg(long = "folder", value_name = "ID")]
        folders: Vec<String>,
        /// Where to write the zip.
        #[arg(short, long)]
        output: String,
    },
    /// Upload a zip of skills, as `export` writes it: its skills go to a
    /// folder named after the file.
    Upload {
        file: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum FileAction {
    /// Add reference files to a skill (repeatable), under their file name or
    /// `--as`.
    Add {
        id: String,
        #[arg(required = true)]
        files: Vec<String>,
        /// The destination in the skill's folder, for a single file
        /// (`examples/login.spec.ts`).
        #[arg(long = "as", value_name = "PATH")]
        dest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print a file of a skill (`SKILL.md` included).
    Show {
        id: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Overwrite a text file of a skill with a file (`-` for the standard
    /// input). Writing `SKILL.md` validates it as the editor does.
    Write {
        id: String,
        path: String,
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// Delete a reference file.
    Remove {
        id: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
}

pub fn run_skill(action: SkillAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        SkillAction::List { json } => {
            let bank = daemon.get("/settings/skills")?.ok()?.json()?;
            if json {
                print_json(&bank);
            } else {
                print!("{}", list_lines(&bank));
            }
        }
        SkillAction::Show { id, json } => {
            let skill = read(&daemon, &id)?;
            if json {
                print_json(&skill);
            } else {
                let folders = daemon.get("/settings/skill-folders")?.ok()?.json()?;
                print!("{}", show_text(&skill, &folders));
            }
        }
        SkillAction::Add {
            file,
            name,
            folder,
            json,
        } => {
            let content = read_text(&file)?;
            let skill = daemon
                .post(
                    "/settings/skills",
                    Some(&json!({ "content": content, "name": name, "folder_id": folder })),
                )?
                .ok()?
                .json()?;
            done(&skill, json, "added");
        }
        SkillAction::Update {
            id,
            name,
            folder,
            root,
            content,
            json,
        } => {
            anyhow::ensure!(
                name.is_some() || folder.is_some() || root || content.is_some(),
                "nothing to change: give at least one flag (see --help)"
            );
            if let Some(file) = content {
                daemon
                    .put_text(
                        &format!("/settings/skills/{}/files/SKILL.md", segment(&id)),
                        read_text(&file)?,
                    )?
                    .ok()?;
            }
            let mut body = serde_json::Map::new();
            if let Some(name) = name {
                body.insert("name".into(), json!(name));
            }
            if let Some(folder) = folder {
                body.insert("folder_id".into(), json!(folder));
            } else if root {
                body.insert("folder_id".into(), Value::Null);
            }
            let skill = if body.is_empty() {
                read(&daemon, &id)?
            } else {
                daemon
                    .put(
                        &format!("/settings/skills/{}", segment(&id)),
                        &Value::Object(body),
                    )?
                    .ok()?
                    .json()?
            };
            done(&skill, json, "updated");
        }
        SkillAction::Remove { id, json } => {
            daemon
                .delete(&format!("/settings/skills/{}", segment(&id)), None)?
                .ok()?;
            removed(&id, json);
        }
        SkillAction::Referents { id, json } => {
            let referents = daemon
                .get(&format!("/settings/skills/{}/referents", segment(&id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&referents);
            } else {
                print!("{}", referent_lines(&referents));
            }
        }
        SkillAction::File { action } => file(&daemon, action)?,
        SkillAction::Folder { action } => import::folder(&daemon, action)?,
        SkillAction::Scan {
            source,
            scan_id,
            json,
        } => {
            let report = import::scan(&daemon, &source, scan_id)?;
            if json {
                print_json(&report);
            } else {
                print!("{}", render::candidate_lines(&report));
            }
        }
        SkillAction::CancelScan { scan_id, json } => {
            let answer = daemon
                .post(
                    &format!("/settings/skills/scan/{}/cancel", segment(&scan_id)),
                    None,
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&answer);
            } else if answer["cancelled"] == true {
                println!("scan {scan_id} cancelled");
            } else {
                println!("scan {scan_id} was not running");
            }
        }
        SkillAction::Import {
            source,
            only,
            on_collision,
            folder,
            folder_name,
            parent,
            scan_id,
            json,
        } => {
            let destination = import::Destination {
                folder,
                folder_name,
                parent,
            };
            let report = import::import(
                &daemon,
                &source,
                scan_id,
                &only,
                on_collision,
                destination,
            )?;
            if json {
                print_json(&report);
            } else {
                print!("{}", render::import_lines(&report));
            }
        }
        SkillAction::Sources { json } => {
            let sources = daemon
                .get("/settings/skills/sources/recent")?
                .ok()?
                .json()?;
            if json {
                print_json(&sources);
            } else {
                print!("{}", render::source_lines(&sources));
            }
        }
        SkillAction::Export {
            skills,
            folders,
            output,
        } => {
            anyhow::ensure!(
                !skills.is_empty() || !folders.is_empty(),
                "nothing to export: give --skill or --folder"
            );
            let zip = daemon
                .post(
                    "/settings/skills/export",
                    Some(&json!({ "skill_ids": skills, "folder_ids": folders })),
                )?
                .ok()?;
            std::fs::write(&output, &zip.bytes).with_context(|| format!("cannot write {output}"))?;
            println!("{output} written ({} bytes)", zip.bytes.len());
        }
        SkillAction::Upload { file, json } => {
            let name = Path::new(&file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let report = daemon
                .post_bytes(
                    &format!("/settings/skills/upload?name={}", segment(&name)),
                    "application/zip",
                    read_file_bytes(&file)?,
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&report);
            } else {
                print!("{}", render::import_lines(&report));
            }
        }
    }
    Ok(())
}

fn file(daemon: &Daemon, action: FileAction) -> Result<()> {
    match action {
        FileAction::Add {
            id,
            files,
            dest,
            json,
        } => {
            anyhow::ensure!(
                dest.is_none() || files.len() == 1,
                "--as names the destination of a single file"
            );
            let mut form = Form::new();
            for path in &files {
                let name = Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .with_context(|| format!("{path} names no file"))?;
                let bytes = std::fs::read(path).with_context(|| format!("cannot read {path}"))?;
                form = form
                    .text("path", dest.clone().unwrap_or_else(|| name.clone()))
                    .part("file", Part::bytes(bytes).file_name(name));
            }
            let answer = daemon
                .post_multipart(&format!("/settings/skills/{}/files", segment(&id)), form)?
                .ok()?
                .json()?;
            if json {
                print_json(&answer);
            } else {
                for file in answer["uploaded"].as_array().into_iter().flatten() {
                    println!(
                        "{}\t{} bytes added",
                        file["path"].as_str().unwrap_or(""),
                        file["size"]
                    );
                }
            }
        }
        FileAction::Show { id, path, json } => {
            let file = daemon
                .get(&format!(
                    "/settings/skills/{}/files/{}",
                    segment(&id),
                    file_path(&path)
                ))?
                .ok()?
                .json()?;
            if json {
                print_json(&file);
            } else {
                print!("{}", file_text(&file));
            }
        }
        FileAction::Write {
            id,
            path,
            file,
            json,
        } => {
            let saved = daemon
                .put_text(
                    &format!(
                        "/settings/skills/{}/files/{}",
                        segment(&id),
                        file_path(&path)
                    ),
                    read_text(&file)?,
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&saved);
            } else {
                println!(
                    "{}\t{} bytes written",
                    saved["path"].as_str().unwrap_or(&path),
                    saved["size"]
                );
            }
        }
        FileAction::Remove { id, path, json } => {
            daemon
                .delete(
                    &format!(
                        "/settings/skills/{}/files/{}",
                        segment(&id),
                        file_path(&path)
                    ),
                    None,
                )?
                .ok()?;
            removed(&path, json);
        }
    }
    Ok(())
}

/// A file's path in a URL: each segment encoded, the `/` kept.
fn file_path(path: &str) -> String {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn read(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/settings/skills/{}", segment(id)))?
        .ok()?
        .json()
}

fn read_text(file: &str) -> Result<String> {
    String::from_utf8(read_file_bytes(file)?).with_context(|| format!("{file} is not UTF-8 text"))
}

fn done(skill: &Value, json: bool, verb: &str) {
    if json {
        print_json(skill);
    } else {
        println!(
            "{}\t{} {verb}",
            skill["id"].as_str().unwrap_or(""),
            skill["name"].as_str().unwrap_or("")
        );
    }
}

fn removed(id: &str, json: bool) {
    if json {
        print_json(&json!({ "id": id, "deleted": true }));
    } else {
        println!("{id} removed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_path_keeps_its_folders() {
        assert_eq!(file_path("examples/login spec.ts"), "examples/login%20spec.ts");
        assert_eq!(file_path("/SKILL.md"), "SKILL.md");
    }
}
