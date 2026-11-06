//! `pdo project`: the Projets of the running daemon, gesture for gesture with
//! Settings › Projects, over `/projects*`. A thin client: every rule (who may,
//! a valid name, the normalised skills selection, the first member as the
//! default primary) stays behind the route.

mod body;
mod render;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use pdo_platform::adapters::cli::daemon::{
    absolute_path as member_path, print_json, read_json_file, rules_lines, segment, Daemon,
};
use serde_json::{json, Map, Value};

use body::{selected, selection_of, update_body, with_skill, without_skill, UpdateFlags};
use render::{done, list_lines, members_line, show, skill_lines};

#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// List the Projets you may read, one tab-separated line each: id, name,
    /// member count, primary repository.
    List {
        /// The deleted Projets instead, which the Runs launched in them keep.
        #[arg(long)]
        deleted: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show one Projet: its name, members, primary repository, harness or
    /// agentic profile, auto-fail preference and skills.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a Projet from a name, with its first member repositories.
    Create {
        name: String,
        /// A member repository path (repeatable). A relative path is made
        /// absolute from the working directory.
        #[arg(long = "member", value_name = "PATH")]
        members: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Rename a Projet or change the settings it carries. Only the flags given
    /// change; the others are left as they are.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        /// The harness its Runs use (a harness name).
        #[arg(long, conflicts_with = "clear_harness")]
        harness: Option<String>,
        /// Clear the harness: the instance's applies.
        #[arg(long)]
        clear_harness: bool,
        /// The agentic profile its Runs use (a profile id of Settings › Agents).
        #[arg(long, conflicts_with_all = ["agent_harness", "clear_agent"])]
        profile: Option<String>,
        /// An inline combination instead of a profile: this harness, with
        /// `--model` and `--effort`.
        #[arg(long, conflicts_with = "clear_agent")]
        agent_harness: Option<String>,
        #[arg(long, requires = "agent_harness")]
        model: Option<String>,
        #[arg(long, requires = "agent_harness")]
        effort: Option<String>,
        /// Clear the agentic profile: the harness, then the instance, apply.
        #[arg(long)]
        clear_agent: bool,
        /// Fail a node whose session ends without a completion: `on`, `off`,
        /// or `inherit` the instance's preference.
        #[arg(long, value_enum)]
        auto_fail: Option<Preference>,
        /// The member a launch in this Projet preselects as its primary
        /// repository.
        #[arg(long, conflicts_with = "first_primary")]
        primary_repo: Option<String>,
        /// Back to the first member as the primary repository.
        #[arg(long)]
        first_primary: bool,
        #[arg(long)]
        json: bool,
    },
    /// Add or remove a member repository.
    Member {
        #[command(subcommand)]
        action: MemberAction,
    },
    /// The skills the Projet selects for every Run launched in it.
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Show the Projet's provisioning rules, or replace them with `--file`.
    Provisioning {
        id: String,
        /// The new rules, as JSON (`-` for the standard input), shaped as the
        /// command prints them.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a Projet. Its Runs keep it, under its last name.
    Delete {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MemberAction {
    /// Attach a repository to the Projet (a no-op when it already is one).
    Add {
        id: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Detach a repository from the Projet.
    Remove {
        id: String,
        path: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// The skills the Projet selects, one `id<TAB>name` line each.
    List {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Add a skill of the bank to the selection.
    Add {
        id: String,
        skill: String,
        /// The label shown beside it; defaults to the skill id.
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remove a skill from the selection.
    Remove {
        id: String,
        skill: String,
        #[arg(long)]
        json: bool,
    },
    /// Replace the whole selection; no skill clears it. A skill already
    /// selected keeps its label.
    Set {
        id: String,
        skills: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Preference {
    On,
    Off,
    Inherit,
}

pub fn run_project(action: ProjectAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        ProjectAction::List { deleted, json } => {
            let projects = if deleted {
                daemon.get("/projects/deleted")?.ok()?.json()?
            } else {
                daemon.get("/projects")?.ok()?.json()?
            };
            if json {
                print_json(&projects);
            } else {
                print!("{}", list_lines(&projects, deleted));
            }
        }
        ProjectAction::Show { id, json } => {
            let project = daemon
                .get(&format!("/projects/{}", segment(&id)))?
                .ok()?
                .json()?;
            show(&project, json);
        }
        ProjectAction::Create {
            name,
            members,
            json,
        } => {
            let members = members
                .iter()
                .map(|m| member_path(m))
                .collect::<Result<Vec<_>>>()?;
            let project = daemon
                .post(
                    "/projects",
                    Some(&json!({ "name": name, "members": members })),
                )?
                .ok()?
                .json()?;
            done(&project, json, "created");
        }
        ProjectAction::Update {
            id,
            name,
            harness,
            clear_harness,
            profile,
            agent_harness,
            model,
            effort,
            clear_agent,
            auto_fail,
            primary_repo,
            first_primary,
            json,
        } => {
            let body = update_body(UpdateFlags {
                name,
                harness,
                clear_harness,
                profile,
                agent_harness,
                model,
                effort,
                clear_agent,
                auto_fail,
                primary_repo: primary_repo.map(|p| member_path(&p)).transpose()?,
                first_primary,
            });
            if body.as_object().is_some_and(Map::is_empty) {
                anyhow::bail!("nothing to change: give at least one flag (see --help)");
            }
            let project = patch(&daemon, &id, &body)?;
            done(&project, json, "updated");
        }
        ProjectAction::Member { action } => {
            let (id, body, json, verb) = match action {
                MemberAction::Add { id, path, json } => {
                    (id, json!({ "path": member_path(&path)? }), json, "added")
                }
                MemberAction::Remove { id, path, json } => {
                    (id, json!({ "path": member_path(&path)? }), json, "removed")
                }
            };
            let answer = if verb == "added" {
                daemon.post(&format!("/projects/{}/members", segment(&id)), Some(&body))?
            } else {
                daemon.delete(&format!("/projects/{}/members", segment(&id)), Some(&body))?
            };
            let project = answer.ok()?.json()?;
            if json {
                print_json(&project);
            } else if project.is_null() {
                // The member was a path no Projet kept: nothing left to show.
                println!("{} {verb}", body["path"].as_str().unwrap_or_default());
            } else {
                println!(
                    "{} {verb}: {}",
                    body["path"].as_str().unwrap_or_default(),
                    members_line(&project)
                );
            }
        }
        ProjectAction::Skill { action } => skill(&daemon, action)?,
        ProjectAction::Provisioning { id, file, json } => {
            let id = segment(&id);
            let rules = match file {
                Some(file) => daemon
                    .put(
                        &format!("/projects/{id}/provisioning"),
                        &read_json_file(&file)?,
                    )?
                    .ok()?
                    .json()?,
                None => daemon
                    .get(&format!("/projects/{id}/provisioning"))?
                    .ok()?
                    .json()?,
            };
            if json {
                print_json(&rules);
            } else {
                print!("{}", rules_lines(&rules));
            }
        }
        ProjectAction::Delete { id, json } => {
            daemon
                .delete(&format!("/projects/{}", segment(&id)), None)?
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

fn patch(daemon: &Daemon, id: &str, body: &Value) -> Result<Value> {
    daemon
        .patch(&format!("/projects/{}", segment(id)), body)?
        .ok()?
        .json()
}

fn skill(daemon: &Daemon, action: SkillAction) -> Result<()> {
    let read = |id: &str| -> Result<Value> {
        daemon
            .get(&format!("/projects/{}", segment(id)))?
            .ok()?
            .json()
    };
    let (id, skills, json) = match action {
        SkillAction::List { id, json } => {
            let project = read(&id)?;
            if json {
                print_json(&json!(selected(&project)));
            } else {
                print!("{}", skill_lines(&project));
            }
            return Ok(());
        }
        SkillAction::Add {
            id,
            skill,
            name,
            json,
        } => {
            let current = selected(&read(&id)?);
            (id, with_skill(current, &skill, name.as_deref()), json)
        }
        SkillAction::Remove { id, skill, json } => {
            let current = selected(&read(&id)?);
            if !current.iter().any(|s| s["id"] == skill.as_str()) {
                anyhow::bail!("{skill} is not a skill of this Projet");
            }
            (id, without_skill(current, &skill), json)
        }
        SkillAction::Set { id, skills, json } => {
            let current = selected(&read(&id)?);
            (id, selection_of(&current, &skills), json)
        }
    };
    let project = patch(daemon, &id, &json!({ "skills": skills }))?;
    if json {
        print_json(&json!(selected(&project)));
    } else {
        print!("{}", skill_lines(&project));
    }
    Ok(())
}
