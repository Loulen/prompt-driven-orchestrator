//! `pdo skill file`: the reference files of a skill, as the skill's editor
//! adds, reads, saves and deletes them.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use reqwest::blocking::multipart::{Form, Part};

use super::bank::skill_id;
use super::render::file_text;
use super::{read_text, removed};

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

pub(super) fn file(daemon: &Daemon, action: FileAction) -> Result<()> {
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
            let id = skill_id(daemon, &id)?;
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
            let id = skill_id(daemon, &id)?;
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
            let id = skill_id(daemon, &id)?;
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
            let id = skill_id(daemon, &id)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_path_keeps_its_folders() {
        assert_eq!(
            file_path("examples/login spec.ts"),
            "examples/login%20spec.ts"
        );
        assert_eq!(file_path("/SKILL.md"), "SKILL.md");
    }
}
