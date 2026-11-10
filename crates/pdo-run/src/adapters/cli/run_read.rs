//! `pdo run events|costs|artifact|file|refs|source-drift|search|stale-health`:
//! what the run view reads of a Run, over `/runs/{id}*` and `/stale/health`.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};

use super::run_gesture::shown;
use super::run_render::{
    actor_names, cost_lines, event_actors, event_lines, ref_lines, search_lines, write_bytes,
    ActorNames,
};

/// What a Run shows. Exits **4** when the daemon knows no such Run.
#[derive(Subcommand, Debug)]
pub enum RunRead {
    /// The Run's event log, one line per event: when, kind, node, iteration,
    /// and who made it (its display name, else its id; empty when no Actor
    /// did).
    Events {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// The cost of Runs, one line each: id, cost in USD (`?` unknown).
    Costs {
        #[arg(required = true)]
        run_ids: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// One artifact of the Run (a path under its artifacts), or with `--zip`
    /// a port folder zipped. Written to `--output`, else to stdout.
    Artifact {
        run_id: String,
        path: String,
        #[arg(long)]
        zip: bool,
        #[arg(long, short)]
        output: Option<std::path::PathBuf>,
    },
    /// A file of the Run's repository at one of its refs (default: its working
    /// tree while it exists, else its tip).
    File {
        run_id: String,
        path: String,
        #[arg(long = "ref")]
        git_ref: Option<String>,
        #[arg(long, short)]
        output: Option<std::path::PathBuf>,
    },
    /// The Run's refs (fork, deliveries, branches, tip), one line each: id,
    /// sha, label.
    Refs {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// How far the source branch moved since the Run forked from it.
    SourceDrift {
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Search the transcripts of the Run's nodes.
    Search {
        run_id: String,
        query: String,
        #[arg(long)]
        json: bool,
    },
    /// The liveness sweep's heartbeat: its last tick and the nodes it found
    /// blocked on a usage limit.
    StaleHealth {
        #[arg(long)]
        json: bool,
    },
}

/// The display names of the Actors `events` name, by the seam the interface
/// uses (`POST /actors/display`); none when it does not answer, and the
/// lines show the ids.
fn names_of(daemon: &Daemon, events: &serde_json::Value) -> ActorNames {
    let actors = event_actors(events);
    if actors.is_empty() {
        return ActorNames::new();
    }
    let body = serde_json::json!({ "actors": actors });
    daemon
        .post("/actors/display", Some(&body))
        .and_then(|answer| answer.ok())
        .and_then(|answer| answer.json())
        .map(|displays| actor_names(&actors, &displays))
        .unwrap_or_default()
}

pub fn run_run_read(read: RunRead) -> Result<()> {
    let daemon = Daemon::from_env();
    match read {
        RunRead::Events { run_id, json } => {
            let events = daemon
                .get(&format!("/runs/{}/events", segment(&run_id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&events);
            } else {
                print!("{}", event_lines(&events, &names_of(&daemon, &events)));
            }
        }
        RunRead::Costs { run_ids, json } => {
            let ids = run_ids.join(",");
            let costs = daemon
                .get(&format!("/runs/costs?ids={}", segment(&ids)))?
                .ok()?
                .json()?;
            shown(&costs, json, cost_lines);
        }
        RunRead::Artifact {
            run_id,
            path,
            zip,
            output,
        } => {
            let bytes = if zip {
                daemon.get_bytes(&format!(
                    "/runs/{}/artifact/zip?path={}",
                    segment(&run_id),
                    segment(&path)
                ))?
            } else {
                daemon.get_bytes(&format!(
                    "/runs/{}/artifact?path={}",
                    segment(&run_id),
                    segment(&path)
                ))?
            };
            write_bytes(&bytes, output.as_deref())?;
        }
        RunRead::File {
            run_id,
            path,
            git_ref,
            output,
        } => {
            let at = git_ref
                .map(|r| format!("&ref={}", segment(&r)))
                .unwrap_or_default();
            let bytes = daemon.get_bytes(&format!(
                "/runs/{}/file?path={}{at}",
                segment(&run_id),
                segment(&path)
            ))?;
            write_bytes(&bytes, output.as_deref())?;
        }
        RunRead::Refs { run_id, json } => {
            let refs = daemon
                .get(&format!("/runs/{}/refs", segment(&run_id)))?
                .ok()?
                .json()?;
            shown(&refs, json, ref_lines);
        }
        RunRead::SourceDrift { run_id, json: _ } => {
            let drift = daemon
                .get(&format!("/runs/{}/source-drift", segment(&run_id)))?
                .ok()?
                .json()?;
            print_json(&drift);
        }
        RunRead::Search {
            run_id,
            query,
            json,
        } => {
            let found = daemon
                .get(&format!(
                    "/runs/{}/transcripts/search?q={}",
                    segment(&run_id),
                    segment(&query)
                ))?
                .ok()?
                .json()?;
            shown(&found, json, search_lines);
        }
        RunRead::StaleHealth { json } => {
            let health = daemon.get("/stale/health")?.ok()?.json()?;
            if json {
                print_json(&health);
            } else {
                let blocked = health["blocked_on_limit"].as_array().map_or(0, Vec::len);
                println!(
                    "last tick {} · every {}s · {blocked} node(s) blocked on a usage limit",
                    health["last_tick_at"].as_str().unwrap_or("never"),
                    health["tick_interval_secs"],
                );
            }
        }
    }
    Ok(())
}
