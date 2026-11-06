//! `pdo pipeline`: the pipelines of the running daemon's registry, their
//! versions (ADR-0088), their portable document (ADR-0059) and the node
//! library, gesture for gesture with the editor, over `/pipelines*`,
//! `/library*` and `/nodes/parse`. A thin client: parsing, naming, the
//! versions and who may, all stay behind the route.

mod body;
mod library;
mod render;

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use serde_json::{json, Value};

use body::{import_body, prompts_of, save_body, validation_body};
pub use library::LibraryAction;
use pdo_platform::adapters::cli::daemon::read_text;
use render::{answer, list_lines, saved_line, validation, version_lines, warn, yaml};

#[derive(Subcommand, Debug)]
pub enum PipelineAction {
    /// List the registry's pipelines, one tab-separated line each: id, name,
    /// last version, node count, the Projets it is validated for, and why its
    /// file does not parse when it does not.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print a pipeline's YAML, its last version (the file New Run launches).
    /// `--json`: the YAML, the editor's pipeline, its prompts and diagnostics.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Create an empty pipeline from a name.
    Create {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Save a pipeline from a YAML file (`-` for the standard input): the
    /// daemon freezes it as its next version. A changed `name:` renames it.
    Save {
        id: String,
        file: String,
        /// A node's prompt, from a file (repeatable). The prompts not given
        /// are left as they are.
        #[arg(long = "prompt", value_name = "NODE=FILE")]
        prompts: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rename a pipeline; its id follows its new name.
    Rename {
        id: String,
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Duplicate a pipeline under a free name.
    Duplicate {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Set the Projets the launch forms offer a pipeline to. No flag: every
    /// Projet.
    Validate {
        id: String,
        /// Offer it only behind « Show test pipelines ».
        #[arg(long, conflicts_with_all = ["projects", "first"])]
        none: bool,
        /// Offer it to this Projet only (repeatable, in order).
        #[arg(long = "project", value_name = "PROJECT_ID", conflicts_with = "first")]
        projects: Vec<String>,
        /// Offer it to every Projet, listed first for this one (repeatable).
        #[arg(long, value_name = "PROJECT_ID")]
        first: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a pipeline.
    Delete {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The versions of a pipeline, newest first, one tab-separated line each:
    /// number, title, saved at, by whom, the Runs that launched it.
    Versions {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Print the YAML of one version of a pipeline.
    Version {
        id: String,
        number: u32,
        #[arg(long)]
        json: bool,
    },
    /// Bring a version back: its YAML and prompts, under the pipeline's
    /// current name, saved as the next version, as the editor's Restore then
    /// Save does.
    Restore {
        id: String,
        number: u32,
        #[arg(long)]
        json: bool,
    },
    /// Export a pipeline's portable document (its prompts inlined), to the
    /// standard output or `--output`; `--skills` writes the zip of the skills
    /// it references, which `import --skills` takes back.
    Export {
        id: String,
        #[arg(long, short)]
        output: Option<String>,
        #[arg(long, value_name = "ZIP")]
        skills: Option<String>,
    },
    /// Import a portable document (`-` for the standard input) as a new
    /// pipeline, with the skills zip `export --skills` wrote.
    Import {
        file: String,
        #[arg(long, value_name = "ZIP")]
        skills: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Parse one node's YAML (`-` for the standard input) as the editor's
    /// « Paste node » does: the node it makes, as YAML, or why not.
    ParseNode {
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// The node library.
    Library {
        #[command(subcommand)]
        action: LibraryAction,
    },
}

pub fn run_pipeline(action: PipelineAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        PipelineAction::List { json } => {
            let pipelines = daemon.get("/pipelines")?.ok()?.json()?;
            if json {
                print_json(&pipelines);
            } else {
                print!("{}", list_lines(&pipelines));
            }
        }
        PipelineAction::Show { id, json } => {
            let detail = read(&daemon, &id)?;
            if json {
                print_json(&detail);
            } else {
                for diagnostic in detail["diagnostics"].as_array().into_iter().flatten() {
                    eprintln!("warning: {}", diagnostic.as_str().unwrap_or_default());
                }
                print!("{}", yaml(&detail));
            }
        }
        PipelineAction::Create { name, json } => {
            let created = daemon
                .post("/pipelines", Some(&json!({ "name": name })))?
                .ok()?
                .json()?;
            answer(&created, json, |c| format!("{} created", id_of(c)));
        }
        PipelineAction::Save {
            id,
            file,
            prompts,
            json,
        } => {
            let body = save_body(read_text(&file)?, &prompts_of(&prompts)?);
            let saved = save(&daemon, &id, &body)?;
            answer(&saved, json, |s| saved_line(s, &id));
        }
        PipelineAction::Rename { id, name, json } => {
            let renamed = daemon
                .put(
                    &format!("/pipelines/{}/rename", segment(&id)),
                    &json!({ "name": name }),
                )?
                .ok()?
                .json()?;
            answer(&renamed, json, |r| {
                format!(
                    "{}\t{} renamed",
                    id_of(r),
                    r["name"].as_str().unwrap_or(&name)
                )
            });
        }
        PipelineAction::Duplicate { id, json } => {
            let copy = daemon
                .post(&format!("/pipelines/{}/duplicate", segment(&id)), None)?
                .ok()?
                .json()?;
            answer(&copy, json, |c| {
                format!("{} duplicated from {id}", id_of(c))
            });
        }
        PipelineAction::Validate {
            id,
            none,
            projects,
            first,
            json,
        } => {
            let validated = daemon
                .put(
                    &format!("/pipelines/{}/validation", segment(&id)),
                    &validation_body(none, projects, first),
                )?
                .ok()?
                .json()?;
            answer(&validated, json, |v| {
                format!("{id} validated for {}", validation(&v["validation"]))
            });
        }
        PipelineAction::Delete { id, json } => {
            daemon
                .delete(&format!("/pipelines/{}", segment(&id)), None)?
                .ok()?;
            answer(&json!({ "id": &id, "deleted": true }), json, |_| {
                format!("{id} deleted")
            });
        }
        PipelineAction::Versions { id, json } => {
            let history = daemon
                .get(&format!("/pipelines/{}/versions", segment(&id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&history);
            } else {
                print!("{}", version_lines(&history));
            }
        }
        PipelineAction::Version { id, number, json } => {
            let version = version(&daemon, &id, number)?;
            if json {
                print_json(&version);
            } else {
                print!("{}", yaml(&version));
            }
        }
        PipelineAction::Restore { id, number, json } => {
            let saved = daemon
                .post(
                    &format!("/pipelines/{}/versions/{number}/restore", segment(&id)),
                    None,
                )?
                .ok()?
                .json()?;
            answer(&saved, json, |s| {
                format!("v{number} restored: {}", saved_line(s, &id))
            });
        }
        PipelineAction::Export { id, output, skills } => export(&daemon, &id, output, skills)?,
        PipelineAction::Import { file, skills, json } => {
            let sidecar = skills
                .map(|zip| std::fs::read(&zip).with_context(|| format!("cannot read {zip}")))
                .transpose()?;
            let body = import_body(read_text(&file)?, sidecar.as_deref());
            let imported = daemon
                .post("/pipelines/import", Some(&body))?
                .ok()?
                .json()?;
            if !json {
                warn(&imported);
            }
            answer(&imported, json, |i| format!("{} imported", id_of(i)));
        }
        PipelineAction::ParseNode { file, json } => {
            let parsed = daemon
                .post("/nodes/parse", Some(&json!({ "yaml": read_text(&file)? })))?
                .ok()?
                .json()?;
            if json {
                print_json(&parsed);
            } else {
                warn(&parsed);
                print!("{}", render::node_yaml(&parsed));
            }
        }
        PipelineAction::Library { action } => library::run_library(&daemon, action)?,
    }
    Ok(())
}

fn id_of(answer: &Value) -> &str {
    answer["id"].as_str().unwrap_or_default()
}

fn read(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/pipelines/{}", segment(id)))?
        .ok()?
        .json()
}

fn save(daemon: &Daemon, id: &str, body: &Value) -> Result<Value> {
    daemon
        .put(&format!("/pipelines/{}", segment(id)), body)?
        .ok()?
        .json()
}

fn version(daemon: &Daemon, id: &str, number: u32) -> Result<Value> {
    daemon
        .get(&format!("/pipelines/{}/versions/{number}", segment(id)))?
        .ok()?
        .json()
}

/// The portable document to `output` (the standard output when absent), and
/// its skills zip to `skills` when the pipeline references a skill.
fn export(daemon: &Daemon, id: &str, output: Option<String>, skills: Option<String>) -> Result<()> {
    let document = daemon
        .get(&format!("/pipelines/{}/document", segment(id)))?
        .ok()?
        .body;
    if let Some(zip) = skills {
        let bytes = daemon.get_bytes(&format!("/pipelines/{}/document/skills", segment(id)))?;
        if bytes.is_empty() {
            eprintln!("{id} references no skill: no zip written");
        } else {
            std::fs::write(&zip, bytes).with_context(|| format!("cannot write {zip}"))?;
            eprintln!("skills written to {zip}");
        }
    }
    match output {
        Some(path) => {
            std::fs::write(&path, document).with_context(|| format!("cannot write {path}"))?;
            eprintln!("{id} exported to {path}");
        }
        None => print!("{document}"),
    }
    Ok(())
}
