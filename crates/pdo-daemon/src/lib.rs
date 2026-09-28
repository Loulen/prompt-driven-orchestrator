// Don't drop this lint (ADR-0039): the daemon is one crate of sibling modules, so
// a `pub` item with no consumer outside its file is a leak, and only this warning
// (with `clippy -D warnings`) keeps the crate surface from re-widening.
#![warn(unreachable_pub)]

mod agent_profile;
mod contexts;
mod distribution;
mod instance_config;
mod pipeline_ports;
mod price_table;
mod review_comments;
use pdo_run::adapters::run_cost;
mod run_ports;
pub(crate) use pdo_run::domain::run_refs;
mod sandbox_ports;
mod session_ports;
mod stats;
mod stats_absorption;
mod stats_performance;
mod structured_diff;
mod trigger_ports;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use clap::{Parser, Subcommand};
use rust_embed::Embed;
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio::time;
use tracing::{error, info, warn};
use ts_rs::TS;

use crate::contexts::ContextResourcesSeed;
pub use crate::contexts::{
    ContextResources, Contexts, PipelineResources, RunResources, SessionRunPorts, TriggerResources,
};
pub use pdo_platform::adapters::cli::{PageAction, ServiceAction};
pub use pdo_platform::domain::service_health::ServiceHealthOverride;

// The workspace context's modules the rest of this crate reads, until each
// context migrates onto the workspace's ports.
pub(crate) use pdo_workspace::adapters::git::worktrees;
#[cfg(test)]
use pdo_workspace::adapters::git::worktrees::create_worktree;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;
pub(crate) use pdo_workspace::adapters::sqlite as workspace_db;

// The skill context's rules the rest of this crate reads: the selection per
// tier (instance, Projet, Trigger, pipeline node, Run). Every skill gesture
// goes through `AppState::skills`.
pub(crate) use pdo_skill::domain::selection as skill_selection;

// The pipeline context's rules and adapters the rest of this crate reads: the
// document, its frontmatter, the registry's paths and the watcher. Every
// pipeline gesture of an actor goes through the context's own routes.
pub(crate) use pdo_pipeline::adapters::self_writes::SelfWrites;
pub(crate) use pdo_pipeline::adapters::watcher as pipeline_watcher;
pub(crate) use pdo_pipeline::domain::migration as pipeline_migrator;
pub(crate) use pdo_pipeline::domain::pipeline;

// The run context the rest of this crate reads (#923, #924, #925): the graph a
// Run walks, what it allows, its verdicts, the create path a Trigger fire takes,
// and what the review of a Run (its diff, its comments) still here reads. Every
// surface of a Run, every gesture on a node, the scheduler and the Pipeline
// Manager's commands live in `pdo-run`.
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_run::adapters::cli::{cli_daemon_url, cli_run_id, session_env_claim};
pub use pdo_run::adapters::cli::{run_reap, run_run_create, RunAction};
use pdo_run::adapters::disk_live_facts::run_diff_base;
use pdo_run::adapters::env::{
    env_max_attachments_mb, resolve_max_attachments_mb, MAX_ATTACHMENTS_MB_DEFAULT,
};
use pdo_run::application::boot_recovery;
use pdo_run::application::events::{
    append_event as append_run_event, load_projected as load_run_projected,
};
use pdo_run::application::manager::ensure_run_manager;
pub(crate) use pdo_run::application::node::prompt as prompt_augmenter;
use pdo_run::application::paths::{dirs_next_home, instance_pipeline_dir, resolve_pipeline_path};
use pdo_run::application::pipeline_modified::relay_pipeline_modifications;
use pdo_run::application::refs::{default_to_id, resolve_ref_param, run_worktree_dir, RefTarget};
pub(crate) use pdo_run::application::sandbox as run_sandbox;
use pdo_run::application::session_claim::{session_claim_from_headers, SessionClaim};
pub(crate) use pdo_run::application::stale::detector as stale_detector;
use pdo_run::application::stale::sweep::run_stale_detection_supervised;
use pdo_run::application::supervision::run_isolated;
pub(crate) use pdo_run::domain::admission;

/// A Run's events, its state and their projection, from the run context
/// (#923), under the one name the rest of this crate reads them by. Every
/// event enters the log through `AppState::run` ([`append_event`]).
pub(crate) mod event_log {
    pub(crate) use pdo_run::domain::event::*;
    pub(crate) use pdo_run::domain::projection::*;
    pub(crate) use pdo_run::domain::state::*;
}

// The legacy surfaces' way into the run context: the same names they always
// called, over `AppState::run`.

/// Append an event to a Run's log (guarded, stored, published; the terminal
/// merge-back of a sandboxed Run).
pub(crate) async fn append_event(state: &AppState, event: &event_log::Event) -> Result<()> {
    append_run_event(&state.run, event).await
}

pub(crate) async fn load_events(
    db: &sqlx::SqlitePool,
    run_id: &str,
) -> Result<Vec<event_log::Event>> {
    pdo_run::adapters::sqlite_event_store::load(db, run_id).await
}

pub(crate) async fn load_all_run_ids(db: &sqlx::SqlitePool) -> Result<Vec<String>> {
    pdo_run::adapters::sqlite_event_store::run_ids(db).await
}

/// A Run's own repository, or the daemon's when it names none.
fn effective_repo_root(state: &AppState, run_state: &event_log::RunState) -> PathBuf {
    pdo_run::application::paths::effective_repo_root(&state.run, run_state)
}

/// See [`pdo_run::application::events::load_projected`].
async fn load_projected(
    state: &AppState,
    run_id: &str,
) -> Result<(Vec<event_log::Event>, event_log::RunState), Box<Response>> {
    load_run_projected(&state.run, run_id).await
}

const DEFAULT_PORT: u16 = pdo_platform::domain::service_unit::DEFAULT_PORT;

#[derive(Embed)]
#[folder = "../../frontend/dist"]
struct FrontendAssets;

#[derive(Parser, Debug)]
#[command(
    name = "pdo",
    about = "PDO — deterministic Claude Code pipeline orchestrator",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the PDO daemon
    Daemon {
        /// Address to listen on. Defaults to all interfaces.
        #[arg(long, env = "PDO_BIND")]
        bind: Option<IpAddr>,
        #[arg(short, long, env = "PDO_PORT", default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Signal that the current NodeRun has completed successfully
    Complete {
        /// Runtime-initiated completion on turn end: set by the injected `Stop`
        /// hook, never by an agent typing `pdo complete`. Records
        /// `NodeAutoCompleted` (source `StopHook`).
        #[arg(long)]
        auto: bool,
    },
    /// Signal that the current NodeRun has failed
    Fail {
        #[arg(long)]
        reason: String,
    },
    /// Declare that you are waiting on the user (#588, ADR-0069): the node and
    /// its run turn `awaiting_user`, the message is shown on the run's banner.
    /// Accepted on any node with a live session; refused (exit 3) on a `script`
    /// node or without a live session; a repeat is a no-op. The wait lifts when
    /// the user answers in the PDO terminal (Enter) or releases completion.
    WaitUser {
        /// The question you are waiting on, shown verbatim on the banner
        /// (capped at 100 characters).
        #[arg(long)]
        message: Option<String>,
    },
    /// Graceful no-op: there is legitimately nothing to do, so end the run as
    /// `skipped` instead of `failed`. Use when the input/pool is empty, NOT for
    /// genuine errors — reserve `fail` for those.
    Skip {
        #[arg(long)]
        reason: String,
    },
    /// Migrate legacy pipeline YAMLs on disk (ADR-0011): dissolve
    /// `type: for-each` (and other retired control nodes) into `loops:`
    /// regions, rewiring edges. Never auto-run at load — a migration is always
    /// a deliberate act. A real run writes a `.bak` beside each migrated file.
    Migrate {
        /// Migrate only this directory of pipeline YAMLs. Default: the repo's
        /// `.pdo/pipelines`, `~/.pdo/pipelines`, and the library store.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// Report what would be migrated without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Reclaim disk from old terminal Runs surfaced by `GET /runs/reapable`.
    /// Applies a graded-TTL policy ([`reap_policy`]) and archives each match via
    /// the existing `cleanup_run` command.
    ///
    /// Driven from a pipeline `script` node, never by the runtime itself
    /// (ADR-0012a: the runtime never triggers a durable action on its own).
    Reap {
        /// Print only the number of Runs the policy would reclaim, then exit 0.
        /// A Trigger guard can gate on it: `[ "$(pdo reap --count)" -gt 0 ]`.
        #[arg(long)]
        count: bool,
        /// Show the reclaim plan without archiving anything.
        #[arg(long)]
        dry_run: bool,
        /// TTL in hours for `completed` Runs (default 24).
        #[arg(long)]
        ttl_hours: Option<i64>,
        /// TTL in hours for `failed`/`halted`/`skipped` Runs — post-mortem
        /// evidence, held longer but bounded (default 72).
        #[arg(long)]
        terminal_ttl_hours: Option<i64>,
        /// Wall-clock budget in seconds for issuing reclaims (default 40). Must
        /// stay under `SCRIPT_TIMEOUT_SECS` (60s), which kills the janitor's
        /// `script` node; past the budget the janitor stops and still exits 0,
        /// because a failed janitor Run leaks its own worktree.
        #[arg(long, default_value_t = 40)]
        budget_secs: u64,
    },
    /// Manage the persistent OS service unit for the daemon (ADR-0019).
    ///
    /// Installs the daemon as a `systemd --user` unit (Linux) or a launchd
    /// LaunchAgent (macOS, best-effort) so it starts at boot and survives logout.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Print the documentation PDO generates from its own code.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
    /// Talk to a running daemon's Run surface.
    Run {
        #[command(subcommand)]
        action: Box<RunAction>,
    },
    /// Mount directories as live pages through the daemon.
    Page {
        #[command(subcommand)]
        action: PageAction,
    },
    /// Read and answer the review comments a human left on this Run's diff
    /// (#751, ADR-0067 §4). Run from a node or manager session: the Run id comes
    /// from `PDO_RUN_ID` (`--run` overrides it) and the author from the session
    /// (`manager`, or the node id).
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum ReviewAction {
    /// List the Run's review comments as JSON, each with its thread of replies.
    /// Mapped onto the Run's current fork → tip: `outdated: true` means the line
    /// a comment points at has changed since it was written (its `excerpt` keeps
    /// the original hunk); `mapped_line` is where the line sits now.
    List {
        /// `open` (default), `resolved` or `all`.
        #[arg(long, default_value = "open")]
        state: String,
        /// The Run to read; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
    },
    /// Reply to one comment (`rc-001`, …). `--resolved` proposes a resolution —
    /// or resolves directly when the instance setting `review_agent_can_resolve`
    /// is on. The human can always reopen.
    Reply {
        /// The comment id, as shown by `pdo review list` and in the message you received.
        id: String,
        /// Your answer (markdown). `--body` is accepted as an alias.
        #[arg(long, visible_alias = "body")]
        text: String,
        /// You consider the comment addressed.
        #[arg(long)]
        resolved: bool,
        /// The Run the comment belongs to; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum DocsAction {
    /// The harness support matrix — which capability each harness has, what is
    /// absent and why, and the last validated version of each binary.
    ///
    /// Rendered from the capability declaration in `pdo_harness::probes`, its only
    /// source of truth. `--check` fails (exit 1) if the block committed in the
    /// target file has drifted from the code; `--write` puts it back.
    SupportTable {
        /// Fail if the block in the target file has drifted from the code.
        #[arg(long)]
        check: bool,
        /// Rewrite the block in the target file from the code.
        #[arg(long, conflicts_with = "check")]
        write: bool,
        /// The file carrying the generated block, between its two markers.
        #[arg(long, default_value = pdo_harness::support::DOCUMENT)]
        file: std::path::PathBuf,
    },
}

struct AppState {
    /// The workspace context: Projets, repositories, provisioning, git.
    workspace: Arc<pdo_workspace::application::Workspace>,
    /// The skill context: the Banque de skills, its import, the delivery.
    skills: Arc<pdo_skill::Skills>,
    db: sqlx::SqlitePool,
    /// The run context (#923): the event log, the one way an event enters it,
    /// and the reads of a Run.
    run: Arc<pdo_run::Runs>,
    /// The raw channel of the kernel's event bus, for the producers that still
    /// send on it (Projet, pipeline, Trigger and Run-list announcements). `/ws`
    /// relays the bus.
    bus_tx: broadcast::Sender<serde_json::Value>,
    repo_root: PathBuf,
    /// Paths the daemon has just written. The pipeline watcher suppresses
    /// `pipeline_changed` broadcasts for paths seen within the TTL window — that
    /// is how our own writes are told apart from external ones (vim, git
    /// checkout) without ignoring the latter.
    recent_writes: SelfWrites,
    /// Per-daemon override for the `docker` binary the sandbox wiring shells out
    /// to. Seeded via [`DaemonConfig`], never a process-global `std::env` read.
    docker_cmd_override: Option<String>,
    /// Per-daemon sandbox home-root override, so layer-3 staging lands under the
    /// test's tempdir. Seeded via [`DaemonConfig`].
    sandbox_home_override: Option<PathBuf>,
    /// TTL cache (~10 s) of the Docker availability probe, surfaced as
    /// `sandbox_docker` on `GET /settings`. Lazily refreshed when stale — not
    /// boot-once (a user may start Docker mid-session) and not per-fetch (a `PUT`
    /// of an unrelated knob would pay a docker round-trip). The refresh runs under
    /// `spawn_blocking` + a short `timeout`, so a cold Docker daemon never hangs
    /// the `/settings` response.
    docker_probe_cache: Arc<pdo_sandbox::application::docker_probe::DockerProbeCache>,
    /// Per-harness offered-catalogue cache (ADR-0053 §3), keyed by harness name.
    /// Freshness contract: at most one `--version` re-check per harness per
    /// version window, and a full `--help` re-probe only when that version
    /// changed — so an auto-updating binary is followed without a restart, at
    /// bounded subprocess cost on the `/settings` path. Never persisted: the truth
    /// is on disk, so a restart just re-reads it.
    harness_catalogue: Arc<pdo_harness::application::read_catalogue::CatalogueReader>,
    /// Serializes price syncs. Unlike the Triggers' tick lock, which serializes
    /// because both fires must land, a second concurrent price sync brings
    /// nothing — so take it with `try_lock` and answer **409** on failure. The
    /// boot refresh takes the same lock and, failing, gives up silently: a manual
    /// sync is in flight and does strictly better.
    price_sync_lock: tokio::sync::Mutex<()>,
    /// Price-source URL override; `None` →
    /// [`price_table::PRICE_SOURCE_URL_DEFAULT`]. Seeded per-daemon so a layer-3
    /// test points the fetch at its own fixture server, never at models.dev.
    price_source_url: Option<String>,
    /// Whether the boot pass may refresh the fetched tier. See
    /// [`DaemonConfig::price_refresh_at_boot`].
    price_refresh_at_boot: bool,
    /// The `platform` context (page mounts, update, audit journal, service
    /// health), wired by the composition root.
    platform: Arc<pdo_platform::Platform>,
    /// Extra WebSocket Origin allowlist entries from `PDO_ALLOWED_WS_ORIGINS`,
    /// parsed + normalised at boot. Shared by BOTH WS upgrades (`/ws` here and
    /// `/sessions/{id}/pty` in the session context) via
    /// [`pdo_session::adapters::http::origin::check_origin`]; the four localhost
    /// defaults are always added on top.
    allowed_ws_origins: Vec<String>,
    /// The session context (#920): tmux behind its port, the terminals, the
    /// declared wait, the reaper, the library assistant.
    session: Arc<pdo_session::Session>,
}

/// One-shot `pdo review list` / `pdo review reply` (#751): the thin CLI client of
/// the review endpoints. Blocking `reqwest`, no tokio runtime (see `main.rs`);
/// plain `Result` → `0`/`1` exit mapping. Identity travels in the session headers
/// — the daemon deduces the author (`manager` for the manager session, else the
/// node id); `--run` only changes which Run is addressed, never who speaks.
pub fn run_review(action: ReviewAction) -> Result<()> {
    let url = cli_daemon_url();
    let client = reqwest::blocking::Client::new();
    let session = session_env_claim();
    let with_identity = |request: reqwest::blocking::RequestBuilder| {
        let mut request = request.header("X-PDO-Actor", "cli");
        if let Some((run_id, node_id)) = &session {
            request = request
                .header(SESSION_RUN_HEADER, run_id)
                .header(SESSION_NODE_HEADER, node_id);
        }
        request
    };
    let run_of = |flag: Option<String>| -> Result<String> {
        match flag {
            Some(r) if !r.trim().is_empty() => Ok(r.trim().to_string()),
            _ => cli_run_id().context(
                "no Run: pass `--run <run-id>` or run this from a PDO node/manager session",
            ),
        }
    };

    match action {
        ReviewAction::List { state, run } => {
            let run_id = run_of(run)?;
            if review_comments::StateFilter::parse(&state).is_none() {
                anyhow::bail!("--state must be `open`, `resolved` or `all`, got {state:?}");
            }
            // #752: mapped onto the Run's current fork → tip, so each comment
            // says `outdated: true` when the line it points at has changed since
            // (and `mapped_line` when it merely shifted).
            let response = with_identity(client.get(format!(
                "{url}/runs/{run_id}/review/comments?state={}&from=fork&to=tip",
                state.trim()
            )))
            .send()
            .context("failed to reach daemon")?;
            let body: serde_json::Value = daemon_cli_response(response, "list review comments")?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        ReviewAction::Reply {
            id,
            text,
            resolved,
            run,
        } => {
            let run_id = run_of(run)?;
            if text.trim().is_empty() {
                anyhow::bail!("--text must not be empty");
            }
            let response = with_identity(client.post(format!(
                "{url}/runs/{run_id}/review/comments/{}/reply",
                id.trim()
            )))
            .json(&serde_json::json!({ "text": text, "resolved": resolved }))
            .send()
            .context("failed to reach daemon")?;
            let body: serde_json::Value = daemon_cli_response(response, "reply to review comment")?;
            let author = body["author"].as_str().unwrap_or("agent");
            let outcome = body["outcome"].as_str().unwrap_or("replied");
            let what = match outcome {
                "resolved" => "resolved (the instance lets agents resolve; the human can reopen)",
                "proposed" => "resolution proposed — the human decides with Resolve / Reopen",
                _ => "reply recorded",
            };
            println!("{id}: {what} · author {author} · run {run_id}");
        }
    }
    Ok(())
}

fn daemon_cli_response<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
    operation: &str,
) -> Result<T> {
    let status = response.status();
    let raw = response.text().unwrap_or_default();
    if !status.is_success() {
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        let detail = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or(raw.as_str());
        anyhow::bail!("daemon refused to {operation} ({status}): {detail}");
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("unreadable success body from daemon: {raw}"))
}

/// One-shot `pdo docs`: render the documentation PDO generates from its own code,
/// and — with `--check` — refuse a committed copy that has drifted from it.
/// `--check` is what `make check` runs, which is what makes "the table cannot lie"
/// true by construction rather than by discipline.
pub fn run_docs(action: DocsAction) -> Result<()> {
    let DocsAction::SupportTable { check, write, file } = action;
    let block = pdo_harness::support::render();

    if !check && !write {
        print!("{block}");
        return Ok(());
    }

    let document = std::fs::read_to_string(&file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    if check {
        return match pdo_harness::support::check(&document) {
            Ok(()) => {
                eprintln!(
                    "ok: {} carries the generated harness support table.",
                    file.display()
                );
                Ok(())
            }
            Err(why) => anyhow::bail!("{}: {why}", file.display()),
        };
    }

    let updated = pdo_harness::support::splice(&document, &block)
        .map_err(|why| anyhow::anyhow!("{}: {why}", file.display()))?;
    if updated == document {
        eprintln!("unchanged: {} was already current.", file.display());
        return Ok(());
    }
    std::fs::write(&file, &updated)
        .with_context(|| format!("failed to write {}", file.display()))?;
    eprintln!(
        "wrote: {} — harness support table regenerated.",
        file.display()
    );
    Ok(())
}

/// One-shot `pdo migrate` (ADR-0011): walks the pipeline stores and dissolves
/// legacy control-node YAML (Switch/Loop/ForEach) into the edge-conditional +
/// `loops:` region model via [`pipeline_migrator::migrate_pipeline_yaml`]. A real
/// migration first copies the original to `<file>.bak`.
pub fn run_migrate(dir: Option<std::path::PathBuf>, dry_run: bool) -> Result<()> {
    let dirs: Vec<PathBuf> = match dir {
        Some(d) => vec![d],
        None => {
            let mut v = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                v.push(cwd.join(".pdo").join("pipelines"));
            }
            if let Some(home) = dirs_next_home() {
                v.push(home.join(".pdo").join("pipelines"));
            }
            v.retain(|d| d.is_dir());
            v
        }
    };

    if dirs.is_empty() {
        println!("no pipeline directory found — nothing to migrate");
        return Ok(());
    }

    let mut migrated = 0usize;
    let mut up_to_date = 0usize;
    let mut errors = 0usize;

    for d in &dirs {
        let entries = match std::fs::read_dir(d) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip {}: {e}", d.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_yaml = path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml");
            if !is_yaml {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error   {}: {e}", path.display());
                    errors += 1;
                    continue;
                }
            };
            match pipeline_migrator::migrate_pipeline_yaml(&text, &path) {
                Ok(res) if res.migrated => {
                    if dry_run {
                        println!("would migrate {}", path.display());
                        migrated += 1;
                    } else {
                        let backup =
                            path.with_file_name(format!("{}.bak", entry.file_name().display()));
                        if let Err(e) = std::fs::copy(&path, &backup) {
                            eprintln!("error   {}: backup failed: {e}", path.display());
                            errors += 1;
                            continue;
                        }
                        match pdo_pipeline::adapters::fs_migration::migrate_pipeline_file(&path) {
                            Ok(_) => {
                                println!(
                                    "migrated {} (backup: {})",
                                    path.display(),
                                    backup.display()
                                );
                                migrated += 1;
                            }
                            Err(e) => {
                                eprintln!("error   {}: {e}", path.display());
                                errors += 1;
                            }
                        }
                    }
                }
                Ok(_) => up_to_date += 1,
                Err(e) => {
                    eprintln!("error   {}: {e}", path.display());
                    errors += 1;
                }
            }
        }
    }

    let verb = if dry_run { "would migrate" } else { "migrated" };
    println!("{verb} {migrated} pipeline(s); {up_to_date} already up to date; {errors} error(s)");
    if errors > 0 {
        anyhow::bail!("{errors} pipeline(s) failed to migrate");
    }
    Ok(())
}

pub struct DaemonHandle {
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<Result<()>>,
    state: Arc<AppState>,
    /// The trigger context (#927), for its test seams.
    triggers: Arc<pdo_trigger::Triggers>,
}

impl DaemonHandle {
    /// Run a single Trigger scheduler tick synchronously. Lets integration
    /// tests drive firing deterministically instead of waiting for the ~30 s
    /// background interval.
    pub async fn run_trigger_tick(&self) {
        pdo_trigger::adapters::heartbeat::tick_supervised(&self.triggers).await;
    }

    /// Run a single stale-detection sweep synchronously. Lets integration tests
    /// drive liveness detection (dead session -> node Failed) deterministically
    /// instead of waiting for the ~30 s background interval.
    pub async fn run_stale_detection_tick(&self) {
        run_stale_detection_supervised(&self.state.run).await;
    }

    /// Run the boot-recovery reconciliation pass synchronously. The daemon runs
    /// this once at startup; the seam lets integration tests drive it
    /// deterministically (orphaned Running node -> Failed at boot).
    pub async fn run_boot_recovery_tick(&self) {
        boot_recovery::run_boot_recovery(&self.state.run).await;
    }

    /// Run the boot price-table refresh synchronously. Production spawns it
    /// DETACHED at startup, and a test must never race a detached task. Honours
    /// all three gates (config flag, cache must already exist, 24 h staleness).
    pub async fn run_price_refresh_tick(&self) {
        refresh_prices_at_boot(&self.state).await;
    }

    /// Run one periodic version-check pass synchronously (#697): loads the disk
    /// cache if not yet done, then checks ONLY if the setting is on and the cache is
    /// older than the interval — production's own gates, driven deterministically.
    pub async fn run_update_check_tick(&self) {
        self.state.platform.check_for_update().periodic().await;
    }

    /// Warm the harness-catalogue cache synchronously. Production spawns it
    /// DETACHED at boot, so a test drives it here instead of racing that task.
    pub async fn run_catalogue_probe_tick(&self) {
        probe_harness_catalogues_at_boot(&self.state).await;
    }

    /// Run a single orphan-sweep (reaper) pass synchronously, instead of racing
    /// the ~60 s background interval. Resolves the TTL exactly like the background
    /// loop (`stored → env → default`).
    pub async fn run_orphan_sweep_tick(&self) {
        if let Err(e) = self.state.session.sweep().run().await {
            warn!("Reaper sweep (test seam) failed: {e}");
        }
    }

    /// Arm the one-shot stale-sweep poison: the next [`run_stale_detection`]
    /// panics, then disarms itself. Arm it *after* boot, or the immediate startup
    /// sweep consumes it first.
    pub fn arm_stale_panic(&self) {
        self.state.run.arm_stale_panic();
    }

    /// Arm the one-shot spawn poison: the next [`spawn_node`] panics inside its
    /// post-worktree span, then disarms itself. Arm it *after* boot so the run's
    /// entry-node spawn can be targeted.
    pub fn arm_spawn_panic(&self) {
        self.state.run.arm_spawn_panic();
    }

    /// Arm the terminal-tail gate: the detached tail spawned by the next
    /// `node_done`/`node_fail`/`node_skip` parks at its head until
    /// [`Self::release_node_done_gate`], so a test can drop the client connection
    /// mid-window and prove the advance still happens.
    pub fn arm_node_done_gate(&self) {
        self.state.run.arm_node_done_gate();
    }

    /// Release the terminal-tail gate: disarm, then wake every tail
    /// parked at the gate. See [`Self::arm_node_done_gate`].
    pub fn release_node_done_gate(&self) {
        self.state.run.release_node_done_gate();
    }

    /// Force a Trigger's next fire into the past so the next
    /// [`Self::run_trigger_tick`] treats it as due. Test seam only — production
    /// next-fire times come from the cron schedule.
    pub async fn force_trigger_due(&self, trigger_id: &str) {
        let _ = self
            .triggers
            .store()
            .set_next_fire(trigger_id, Some("2020-01-01T00:00:00.000Z"))
            .await;
    }

    /// Seed a Trigger row whose `target_repo` is NULL — a legacy record.
    ///
    /// Bypassing the handler is the point, not a shortcut: `POST /triggers`
    /// refuses an unnamed repo (ADR-0033), so the API can no longer produce this
    /// shape, yet the read side must keep resolving it and the fire path must
    /// refuse it before running its guard.
    pub async fn seed_legacy_trigger_without_target_repo(
        &self,
        name: &str,
        pipeline_id: &str,
        cron: &str,
        guard_command: Option<&str>,
    ) -> String {
        let trigger = self
            .triggers
            .store()
            .create(pdo_trigger::domain::trigger::NewTrigger {
                name: name.to_string(),
                pipeline_id: pipeline_id.to_string(),
                pipeline_name: pipeline_id.to_string(),
                target_repo: None,
                target_repos: None,
                source_branch: None,
                input_template: "seeded input".to_string(),
                variables: serde_json::json!({}),
                cron: cron.to_string(),
                guard_command: guard_command.map(|g| g.to_string()),
                overlap_policy: "skip".to_string(),
                max_concurrent: None,
                sandbox: None,
                harness: None,
                agent_choice: None,
                skills: Vec::new(),
                auto_name: true,
                next_fire_at: Some("2030-01-01T00:00:00.000Z".to_string()),
            })
            .await
            .expect("seeding a legacy trigger row must succeed");
        trigger.id
    }

    /// Seed a `run_started` event whose payload carries **no** `target_repo` — a
    /// legacy Run record. Same rationale as
    /// [`Self::seed_legacy_trigger_without_target_repo`]: `POST /runs` refuses
    /// this shape now, but the read side must keep resolving it forever.
    pub async fn seed_legacy_run_without_target_repo(&self, run_id: &str, pipeline_name: &str) {
        let _ = append_event(
            &self.state,
            &event_log::Event {
                id: None,
                run_id: run_id.to_string(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": pipeline_name,
                    "input": "seeded input",
                })),
            },
        )
        .await;
    }
}

/// Boot-time configuration for a daemon instance.
///
/// Carries the knobs that must be decided *per daemon* rather than read from
/// process-global env in the hot path — two daemons in the same test process
/// would otherwise race on a shared env var.
#[derive(Debug, Clone, Default)]
pub struct DaemonConfig {
    /// Replaces the `claude …` tail in spawned tmux scripts when `Some`. Tests
    /// set this (e.g. `Some("exec sleep 600")`) so node sessions run a harmless
    /// command instead of launching a real `claude` process. `None` →
    /// production default (real claude).
    pub tmux_cmd_override: Option<String>,
    /// Debug-only chaos knob: when a due Trigger's name matches, its scheduler
    /// tick panics, so a test can prove the panic is isolated and the next tick
    /// recovers. Armed in production only by `PDO_DEBUG_PANIC_TRIGGER`.
    pub panic_on_trigger_name: Option<String>,
    /// Debug-only chaos knob: the next stale-detection sweep panics (one-shot,
    /// then disarms). Armed in production only by `PDO_DEBUG_PANIC_STALE`.
    pub panic_on_stale_sweep: bool,
    /// Debug-only chaos knob: the next `spawn_node` panics inside its
    /// post-worktree span (one-shot, then disarms), so a test can prove the
    /// `catch_unwind` reaps the orphaned sub-worktree and fails the run loud.
    /// Armed in production only by `PDO_DEBUG_PANIC_SPAWN`.
    pub panic_on_spawn: bool,
    /// Forced service-health observation: `Some` overrides the boot-time
    /// detection so the status-bar `ephemeral`/`persistent` branches are
    /// demonstrable without real systemd. Armed by `PDO_SERVICE_HEALTH`.
    pub service_health_override: Option<ServiceHealthOverride>,
    /// Per-daemon override for the `docker` binary invoked by the sandbox wiring,
    /// so no test needs a real docker daemon. Read once at boot from
    /// [`pdo_kernel::env::DOCKER_CMD_OVERRIDE_ENV`].
    pub docker_cmd_override: Option<String>,
    /// Per-daemon override for the sandbox **home root**. `None` → the real
    /// `$HOME` (staging under `$HOME/.pdo/sandbox`, `.claude` copied from
    /// `$HOME/.claude`). `Some(dir)` points staging, the `.claude` source and the
    /// container-home mount at `dir`, so a test never touches the real home.
    pub sandbox_home_override: Option<PathBuf>,
    /// Price-source URL (ADR-0034); `None` →
    /// [`price_table::PRICE_SOURCE_URL_DEFAULT`]. `Some(url)` points the fetch at
    /// a local server, so the sync is testable without depending on models.dev.
    pub price_source_url: Option<String>,
    /// Refresh the fetched price tier at startup. Even armed it fetches ONLY if
    /// `fetched.json` already exists and is over 24 h old: no egress before the
    /// user has clicked "Sync costs" once (ADR-0034 — the click IS the consent).
    /// `PDO_PRICE_SYNC=off|0|""` disarms it.
    pub price_refresh_at_boot: bool,
    /// Release-source URL for the version check (#697); `None` →
    /// the shipped GitHub Releases endpoint. `Some(url)` points the check at
    /// a local fixture, same seam as [`Self::price_source_url`].
    pub update_source_url: Option<String>,
    /// Run the boot check + 6 h periodic version-check loop. `true` in production;
    /// `false` in the layer-3 literals, which drive
    /// [`DaemonHandle::run_update_check_tick`] deterministically instead of racing
    /// a detached task. The `update_check` instance setting is honoured either way.
    pub run_update_check_loop: bool,
    /// Executor command override for `POST /update/apply` (#699); `None` → `sh <script>`.
    /// Env: `PDO_UPDATE_EXECUTOR`. Tests point it at a fixture script.
    pub update_executor_override: Option<String>,
    /// Forced installation method (#699); `None` → detected. Env: `PDO_INSTALL_METHOD`.
    pub install_method_override: Option<pdo_platform::domain::install::InstallMethod>,
    /// Forced supervision (#699); `None` → detected from `INVOCATION_ID` /
    /// `XPC_SERVICE_NAME`. No env seam: a test sets those variables on a real binary.
    pub supervision_override: Option<pdo_platform::domain::install::Supervision>,
    /// The daemon's own command line for an unsupervised relaunch (#699), argv form.
    /// `daemon_cli_config` fills it; `None` → `<stable exe> daemon --port <bound port>`.
    pub relaunch_command: Option<Vec<String>>,
    /// Run the ~30 s background Trigger-scheduler loop. `true` in production;
    /// `false` in the layer-3 test literals, because `tokio::time::interval` fires
    /// its FIRST tick immediately and that boot tick races the deterministic
    /// [`DaemonHandle::run_trigger_tick`] seam, firing (or not) a Run out from
    /// under a test's `force_trigger_due`. Disabling the loop masks nothing: the
    /// production TOCTOU it exposed is fixed separately in
    /// the trigger context's tick by re-reading the pause kill-switch late.
    pub run_trigger_scheduler_loop: bool,
    /// **Nested (no-cleanup) mode**: suppress the boot orphan sweep, the periodic
    /// reaper, boot recovery and the stale detector, leaving the daemon completely
    /// passive on tmux state. `false` in production unless the env says otherwise
    /// (`PDO_NODE_ID` — a sub-claude context — or `PDO_DAEMON_NO_CLEANUP`), which
    /// [`DaemonConfig::from_env`] resolves once at boot via [`nested_daemon_from`].
    ///
    /// Don't turn this back into an env read at boot: a test binary holds several
    /// daemons, and a sibling's `remove_var("PDO_DAEMON_NO_CLEANUP")` landing
    /// inside another's in-flight `serve_with_config` booted that daemon with
    /// cleanup ARMED, whose reaper then killed an out-of-band session. Per-daemon
    /// config cannot interleave.
    pub nested_daemon: bool,
    /// Extra exact WebSocket Origin allowlist entries, parsed once at boot
    /// from [`pdo_session::adapters::env::ALLOWED_WS_ORIGINS_ENV`] (`PDO_ALLOWED_WS_ORIGINS`).
    ///
    /// Both WS upgrades — `/ws` and `/sessions/{id}/pty` — reject any browser
    /// `Origin` outside `localhost`/`127.0.0.1:<port>` as a DNS-rebinding / CSWSH
    /// guard. Behind a reverse proxy on a public domain that kills the terminal
    /// and event stream with a 403; this list lets an operator name those origins.
    ///
    /// **Additive, never a replacement.** These entries ADD to the four localhost
    /// defaults; the loopback origins must keep working.
    ///
    /// **Exact-match, no wildcards.** Each entry is one origin as the browser
    /// sends it (`scheme://host[:port]`), compared case-insensitively.
    ///
    /// **Env-only, deliberately NOT a UI/instance setting.** The daemon's HTTP
    /// surface is unauthenticated, so an allowlist editable through the UI would
    /// be self-disarmable by the very attacker it blocks — ADR-0015's
    /// `stored → env → default` precedence is knowingly not applied here.
    ///
    /// Don't bake Vite's `:5173` into the defaults to make `make dev` work: that
    /// would let any page on that port attach a PTY in production. It is fixed on
    /// the frontend with `rewriteWsOrigin` in `vite.config.ts`.
    pub allowed_ws_origins: Vec<String>,
    /// Wraps the assembled router before it is served. The composition root
    /// (`pdo::serve`) puts the `identify` middleware there, at the head of every
    /// request; `None` serves the router as built.
    pub router_layer: Option<RouterLayer>,
    /// The Autorisation d'opération every use case of an extracted context asks
    /// before a mutation. The composition root hands in its registered
    /// extensions; the default has none, and allows everything.
    pub extension_host: ExtensionHostHandle,
}

/// The extension host the daemon's use cases ask (see
/// [`DaemonConfig::extension_host`]).
#[derive(Clone)]
pub struct ExtensionHostHandle(pub Arc<dyn pdo_kernel::extension_host::ExtensionHost>);

impl Default for ExtensionHostHandle {
    fn default() -> Self {
        Self(Arc::new(pdo_kernel::extension_host::NoExtensions))
    }
}

impl std::fmt::Debug for ExtensionHostHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExtensionHostHandle(..)")
    }
}

/// What the composition root plugs into the legacy daemon at boot (see
/// [`DaemonConfig::router_layer`]): a transformation of the assembled router,
/// and — for the daemon itself — the wiring of the bounded contexts already
/// extracted from this crate.
#[derive(Clone)]
pub struct RouterLayer {
    wrap: Arc<dyn Fn(Router) -> Router + Send + Sync>,
    contexts: Option<WireContexts>,
}

/// Wire the extracted contexts over the daemon's resources.
pub type WireContexts = Arc<dyn Fn(ContextResources) -> Contexts + Send + Sync>;

impl RouterLayer {
    pub fn new(layer: impl Fn(Router) -> Router + Send + Sync + 'static) -> Self {
        Self {
            wrap: Arc::new(layer),
            contexts: None,
        }
    }

    /// The same layer, wiring the extracted contexts with `wire`.
    pub fn with_contexts(
        mut self,
        wire: impl Fn(ContextResources) -> Contexts + Send + Sync + 'static,
    ) -> Self {
        self.contexts = Some(Arc::new(wire));
        self
    }

    pub fn apply(&self, router: Router) -> Router {
        (self.wrap)(router)
    }

    fn contexts(&self) -> Option<WireContexts> {
        self.contexts.clone()
    }
}

impl std::fmt::Debug for RouterLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouterLayer(..)")
    }
}

/// Resolve the daemon's cleanup posture from the two env signals production
/// carries it on — **pure**, so the truth table is testable without touching
/// process-global env.
///
/// - `PDO_NODE_ID` present at all ⇒ nested. A sub-claude context exports it via
///   `wrap_with_env`, and its mere presence is the signal (an empty value still
///   means "we are inside a NodeRun").
/// - `PDO_DAEMON_NO_CLEANUP` ⇒ nested when set to anything but `""` or `"0"`, the
///   same truthiness the `PDO_DEBUG_PANIC_*` knobs use.
pub(crate) fn nested_daemon_from(node_id: Option<&str>, no_cleanup: Option<&str>) -> bool {
    node_id.is_some() || no_cleanup.is_some_and(|v| !v.is_empty() && v != "0")
}

impl DaemonConfig {
    /// Build config from the environment — the production / CLI daemon path.
    ///
    /// Reads every env seam exactly once, here at boot, keeping those reads out
    /// of the hot paths.
    pub fn from_env() -> Self {
        Self {
            tmux_cmd_override: std::env::var(pdo_kernel::env::TMUX_CMD_OVERRIDE_ENV).ok(),
            panic_on_trigger_name: std::env::var("PDO_DEBUG_PANIC_TRIGGER").ok(),
            panic_on_stale_sweep: std::env::var("PDO_DEBUG_PANIC_STALE")
                .map(|v| !v.is_empty() && v != "0")
                .unwrap_or(false),
            panic_on_spawn: std::env::var("PDO_DEBUG_PANIC_SPAWN")
                .map(|v| !v.is_empty() && v != "0")
                .unwrap_or(false),
            // Invalid values are ignored rather than fatal.
            service_health_override: std::env::var("PDO_SERVICE_HEALTH")
                .ok()
                .and_then(|v| ServiceHealthOverride::parse(&v)),
            docker_cmd_override: std::env::var(pdo_kernel::env::DOCKER_CMD_OVERRIDE_ENV).ok(),
            sandbox_home_override: std::env::var("PDO_SANDBOX_HOME_OVERRIDE")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            price_source_url: std::env::var("PDO_PRICE_SOURCE_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            // The crate's ONE opt-OUT env, deliberately: a feature that must work
            // out of the box cannot be armed by an env var, and the real opt-in is
            // the first click (ADR-0034 — the boot pass only ever REFRESHES an
            // existing cache, it never creates one).
            price_refresh_at_boot: std::env::var("PDO_PRICE_SYNC")
                .map(|v| !v.is_empty() && v != "0" && v != "off")
                .unwrap_or(true),
            update_source_url: std::env::var("PDO_UPDATE_SOURCE_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            // Core daemon behaviour; the `update_check` SETTING is the user's
            // off switch, not an env var. Only the layer-3 literals opt out.
            run_update_check_loop: true,
            update_executor_override: std::env::var(
                pdo_platform::adapters::detached_executor::EXECUTOR_OVERRIDE_ENV,
            )
            .ok()
            .filter(|s| !s.trim().is_empty()),
            install_method_override: std::env::var(
                pdo_platform::adapters::detached_executor::INSTALL_METHOD_OVERRIDE_ENV,
            )
            .ok()
            .and_then(|v| pdo_platform::domain::install::InstallMethod::parse(&v)),
            supervision_override: None,
            relaunch_command: None,
            // No env seam: the loop is core daemon behaviour; only the layer-3
            // test literals opt out.
            run_trigger_scheduler_loop: true,
            // The ONE place either env var is read; `serve_with_config` consumes
            // the resolved boolean, so tests pick the posture per daemon.
            nested_daemon: nested_daemon_from(
                std::env::var("PDO_NODE_ID").ok().as_deref(),
                std::env::var("PDO_DAEMON_NO_CLEANUP").ok().as_deref(),
            ),
            // The ONE read of PDO_ALLOWED_WS_ORIGINS. Unset or blank yields an
            // empty list, i.e. localhost-only.
            allowed_ws_origins: std::env::var(pdo_session::adapters::env::ALLOWED_WS_ORIGINS_ENV)
                .map(|v| pdo_session::domain::pty_protocol::parse_allowed_origins(&v))
                .unwrap_or_default(),

            router_layer: None,
            extension_host: ExtensionHostHandle::default(),
        }
    }
}

/// The URL to advertise for a listener bound at `bound_addr` (#541).
///
/// An unspecified bind (`0.0.0.0` / `::`) is a *listen* address, not a
/// *connect* address: browsers do reach it, but they then send
/// `Origin: http://0.0.0.0:<port>`, which the WS guard refuses. Map it to
/// `localhost`, the one host every default allowlist entry accepts. A concrete
/// bind (`127.0.0.1`, a LAN IP) is advertised as-is.
pub fn advertised_url(bound_addr: std::net::SocketAddr) -> String {
    let ip = bound_addr.ip();
    if ip.is_unspecified() {
        format!("http://localhost:{}", bound_addr.port())
    } else {
        format!("http://{bound_addr}")
    }
}

/// Start the legacy daemon with an explicit [`DaemonConfig`]. Its
/// [`DaemonConfig::router_layer`] must carry the context wiring of the
/// composition root (`pdo::serve`, `pdo::composed_router_layer`). Tests use this
/// to seed a per-daemon `tmux_cmd_override` without mutating process-global env.
pub async fn serve_with_config(
    addr: SocketAddr,
    repo_root: PathBuf,
    config: DaemonConfig,
) -> Result<DaemonHandle> {
    let db_dir = repo_root.join(".pdo");
    std::fs::create_dir_all(&db_dir).context("failed to create .pdo directory")?;
    let db_path = db_dir.join("pdo.db");

    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
        .await
        .context("failed to open SQLite database")?;

    init_db(&db).await?;

    // #722: the seeded skill lands at every startup — created if absent, its
    // content rewritten when the built-in copy (versioned with PDO) differs.
    // A failed seed is logged, never fatal: the daemon must come up even when
    // the bank is in an unexpected state.
    //
    // A dedicated connection, closed as soon as the seed is done: the seed
    // must not leave its statements in the serving pool's caches.
    match sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.display())).await {
        Ok(seed_db) => {
            let seeding = skill_context(
                &seed_db,
                &repo_root,
                Arc::new(pdo_kernel::extension_host::NoExtensions),
            );
            match seeding.seed().await {
                Ok(outcomes) => {
                    for (id, outcome) in &outcomes {
                        match outcome {
                            pdo_skill::domain::seed::SeedOutcome::Skipped { reason } => {
                                warn!("skill seed `{id}` skipped: {reason}")
                            }
                            _ => info!("skill seed `{id}`: {outcome:?}"),
                        }
                    }
                }
                Err(error) => error!("skill seed failed: {error}"),
            }
            seed_db.close().await;
        }
        Err(error) => error!("skill seed connection failed: {error}"),
    }

    // Must sit after the schema is up (the reader PRAGMA-probes the columns) and
    // before anything can consume a setting — the only moment where "one line per
    // boot" is a fact rather than a hope.
    warn_about_retired_sandbox_settings(&db).await;

    // One-shot migration of prompts stranded in the flat
    // `<repo>/.pdo/pipelines/prompts/` dir into each pipeline's canonical
    // `<stem>.prompts/` dir. Must run BEFORE the file watcher attaches, or the
    // moves surface as spurious pipeline-modified events. Not gated on
    // `nested_daemon`: it is a pure filesystem fix-up, unrelated to tmux.
    {
        let repo_pipelines = repo_root.join(".pdo").join("pipelines");
        match pdo_pipeline::adapters::fs_migration::migrate_stranded_flat_prompts(&repo_pipelines) {
            Ok(n) if n > 0 => info!(
                "#231 migration: moved {n} stranded prompt(s) in {}",
                repo_pipelines.display()
            ),
            Ok(_) => {}
            Err(e) => warn!("#231 migration failed: {e}"),
        }
    }

    pdo_pipeline::adapters::registry::unify_legacy_registries(&repo_root)
        .map_err(|error| anyhow::anyhow!("pipeline registry migration failed: {error}"))?;

    // The kernel's event bus (D20): every Run event and every announcement `/ws`
    // relays, as `/ws` relays it. One channel, sized for the two it replaced
    // (256 Run events + 64 announcements).
    let (bus_tx, _) = broadcast::channel::<serde_json::Value>(320);
    let bus: Arc<dyn pdo_kernel::event_bus::EventBus> = Arc::new(
        pdo_kernel::event_bus::BroadcastEventBus::from_sender(bus_tx.clone()),
    );

    let recent_writes = SelfWrites::new();

    let (run_modified_tx, run_modified_rx) =
        tokio::sync::mpsc::unbounded_channel::<pipeline_watcher::RunPipelineModified>();

    let watcher = pipeline_watcher::spawn_watcher(
        repo_root.clone(),
        instance_pipeline_dir(&repo_root),
        bus_tx.clone(),
        recent_writes.clone(),
        run_modified_tx,
    );
    let run_watcher = Arc::new(Mutex::new(watcher));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind")?;
    let bound_addr = listener
        .local_addr()
        .context("failed to read bound local addr")?;

    // Captured before `config` is consumed into `AppState` below.
    let run_trigger_scheduler_loop = config.run_trigger_scheduler_loop;
    let run_update_check_loop = config.run_update_check_loop;
    let nested_daemon = config.nested_daemon;
    let router_layer = config.router_layer.clone();
    let extension_host = config.extension_host.0.clone();
    let workspace = workspace_context(&db, &bus_tx, &repo_root, extension_host.clone());
    let skills = skill_context(&db, &repo_root, extension_host.clone());
    let wire_contexts = router_layer
        .as_ref()
        .and_then(RouterLayer::contexts)
        .context(
            "the daemon runs under the composition root (`pdo::serve`), which wires its \
             bounded contexts: no context wiring was given",
        )?;
    // The contexts are wired over the daemon's resources, and the ports they
    // need from the legacy side (the Runs, the templates, the settings) reach back
    // into the state being built — hence the cyclic construction.
    let mut contexts_router: Option<Router> = None;
    let mut triggers: Option<Arc<pdo_trigger::Triggers>> = None;
    let resources = ContextResourcesSeed {
        db: db.clone(),
        repo_root: repo_root.clone(),
        port: bound_addr.port(),
        home_root: config
            .sandbox_home_override
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from)),
        update_source_url: config.update_source_url.clone(),
        update_executor_override: config.update_executor_override.clone(),
        install_method_override: config.install_method_override,
        supervision_override: config.supervision_override,
        relaunch_command: config.relaunch_command.clone(),
        service_health_override: config.service_health_override,
        tmux_cmd_override: config.tmux_cmd_override.clone(),
        allowed_ws_origins: config.allowed_ws_origins.clone(),
        skills: skills.clone(),
        pipeline_events: bus_tx.clone(),
        bus,
        run_dirs: Arc::new(pdo_run::adapters::run_dir_watch::DebouncedRunDirs(
            run_watcher.clone(),
        )),
        self_writes: recent_writes.clone(),
        workspace: workspace.clone(),
        docker_cmd_override: config.docker_cmd_override.clone(),
        sandbox_home_override: config.sandbox_home_override.clone(),
        panic_on_stale_sweep: config.panic_on_stale_sweep,
        panic_on_spawn: config.panic_on_spawn,
        panic_on_trigger_name: config.panic_on_trigger_name.clone(),
    };

    let state = Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
        let wired = wire_contexts(resources.into_resources(weak.clone()));
        contexts_router = Some(wired.router);
        triggers = Some(wired.triggers);
        AppState {
            workspace,
            skills,
            platform: wired.platform,
            db,
            run: wired.run,
            bus_tx,
            repo_root,
            recent_writes,
            docker_cmd_override: config.docker_cmd_override,
            sandbox_home_override: config.sandbox_home_override,
            docker_probe_cache: Arc::new(tokio::sync::Mutex::new(None)),
            harness_catalogue: Arc::new(
                pdo_harness::adapters::binary_probe::installed_catalogue_reader(),
            ),
            price_sync_lock: tokio::sync::Mutex::new(()),
            price_source_url: config.price_source_url,
            price_refresh_at_boot: config.price_refresh_at_boot,
            allowed_ws_origins: config.allowed_ws_origins,
            session: wired.session,
        }
    });
    state
        .platform
        .init()
        .await
        .map_err(|e| anyhow::anyhow!("failed to create the platform tables: {e}"))?;
    let triggers = triggers.context("the context wiring gave no trigger context")?;

    // A nested daemon suppresses every AUTOMATIC cleanup pathway, so a Tester or
    // Implementer that accidentally runs `pdo daemon` can't trigger reaper-based
    // kills. It still serves HTTP and accepts explicit `cleanup_run` calls.
    if nested_daemon {
        warn!(
            "PDO daemon in nested (no-cleanup) mode — \
             skipping boot-time orphan sweep and periodic reaper. \
             This daemon will not auto-reap any tmux sessions."
        );
    } else {
        if let Err(e) = state.session.sweep().run().await {
            warn!("Orphan sweep at boot failed: {e}");
        }

        // Reconcile persisted run state against the live process world: fail-fast
        // on orphaned Running nodes and surface git/event-log divergence.
        boot_recovery::run_boot_recovery(&state.run).await;
    }

    // The legacy router, and the routes of the contexts already extracted from it:
    // the sandbox one here, the ones the composition root wired (platform) below.
    let mut app = build_router(state.clone()).merge(sandbox_ports::router(&state, extension_host));
    if let Some(router) = contexts_router {
        app = app.merge(router);
    }
    let app = match &router_layer {
        Some(layer) => layer.apply(app),
        None => app,
    };

    // #541: the bind address (`0.0.0.0`) is deliberate (#260) but is NOT a URL a
    // browser can use — the WS Origin guard (#564) only knows `localhost` and
    // `127.0.0.1`, so `http://0.0.0.0:<port>` half-loads the UI and 403s every
    // socket. Advertise the URL that actually works, keep the bind for operators.
    info!(
        "PDO daemon listening on {bound_addr} — open {}",
        advertised_url(bound_addr)
    );

    // Only speak up when the operator configured extras — silence is the
    // localhost-only default. A malformed entry is KEPT, never fail-fast: a
    // remote client must not be able to crash the boot. It is flagged because a
    // typo here is the top cause of a dead terminal, and the 403 is otherwise mute.
    if !state.allowed_ws_origins.is_empty() {
        info!(
            "WS Origin allowlist extended with {} operator origin(s): {} \
             (added on top of the localhost defaults)",
            state.allowed_ws_origins.len(),
            state.allowed_ws_origins.join(", ")
        );
        for entry in &state.allowed_ws_origins {
            // An origin is `scheme://host[:port]` with no path. The trailing slash
            // of a bare origin is already stripped by `parse_allowed_origins`.
            let after_scheme = entry
                .strip_prefix("https://")
                .or_else(|| entry.strip_prefix("http://"));
            let looks_valid = match after_scheme {
                Some(rest) => !rest.is_empty() && !rest.contains('/') && !entry.contains('*'),
                None => false,
            };
            if !looks_valid {
                warn!(
                    "Suspicious PDO_ALLOWED_WS_ORIGINS entry {entry:?}: expected an \
                     exact origin like \"https://pdo.example.tld\" (scheme + host, no \
                     path, no wildcard). Kept as-is; it will simply never match."
                );
            }
        }
    }

    let _reaper_handle = if nested_daemon {
        None
    } else {
        let reaper_state = state.clone();
        Some(tokio::spawn(async move {
            let interval = pdo_session::adapters::env::reaper_interval();
            let mut tick = time::interval(interval);
            loop {
                tick.tick().await;
                // The sweep resolves its TTLs at every pass, not once at boot: a
                // `PUT /settings` must take effect on the next sweep without a
                // restart (ADR-0015, `stored → env → default`).
                // Panic-isolated: a panic in one tick must not silently kill the
                // loop and leave sessions un-reaped for the daemon's life.
                let st = reaper_state.clone();
                run_isolated("reaper", async move {
                    if let Err(e) = st.session.sweep().run().await {
                        warn!("Reaper sweep failed: {e}");
                    }
                })
                .await;
            }
        }))
    };

    // Background task: process run-scoped pipeline modifications
    let mod_state = state.clone();
    let _run_modified_handle = tokio::spawn(async move {
        relay_pipeline_modifications(mod_state.run.clone(), run_modified_rx).await;
    });

    // Background task: stale detection (dead sessions + idle agents)
    let _stale_handle = if nested_daemon {
        None
    } else {
        let stale_state = state.clone();
        Some(tokio::spawn(async move {
            let mut tick = time::interval(Duration::from_secs(
                stale_detector::STALE_TICK_INTERVAL_SECS,
            ));
            loop {
                tick.tick().await;
                run_stale_detection_supervised(&stale_state.run).await;
            }
        }))
    };

    // The Triggers' heartbeat (#927): fires due Triggers on a ~30s tick.
    let _trigger_handle = if nested_daemon || !run_trigger_scheduler_loop {
        None
    } else {
        Some(pdo_trigger::adapters::heartbeat::spawn(triggers.clone()))
    };

    let task = tokio::spawn(async move {
        let _watcher = run_watcher; // keep the file watcher alive for the server's lifetime
        let _reaper = _reaper_handle; // keep the reaper alive
        let _run_modified = _run_modified_handle; // keep the pipeline_modified handler alive
        let _stale = _stale_handle; // keep the stale detector alive
        let _trigger = _trigger_handle; // keep the trigger scheduler alive
        axum::serve(listener, app).await.context("server error")?;
        Ok(())
    });

    // Must stay DETACHED and posted AFTER `tokio::spawn(axum::serve(...))`, never
    // `.await`ed before `build_router`: a `.await` here delays the first
    // `accept()` behind a DNS timeout, and `tests/smoke.sh` polls `/runs` for 30 s
    // so it would go green while masking exactly that.
    {
        let price_state = state.clone();
        tokio::spawn(async move {
            run_isolated("price refresh", async move {
                refresh_prices_at_boot(&price_state).await;
            })
            .await;
        });
    }

    // Version check (#697): load the on-disk cache so `GET /update` answers from
    // it immediately, then boot check + 6 h loop — detached, like the price
    // refresh, so a slow GitHub never delays the first `accept()`.
    state.platform.start_update_checks(run_update_check_loop);

    // Warm the harness-catalogue cache at boot, detached for the same reason as
    // the price refresh: probing every harness's `--help`/`--version` must not
    // delay the first `accept()`.
    {
        let catalogue_state = state.clone();
        tokio::spawn(async move {
            run_isolated("harness catalogue probe", async move {
                probe_harness_catalogues_at_boot(&catalogue_state).await;
            })
            .await;
        });
    }

    Ok(DaemonHandle {
        addr: bound_addr,
        task,
        state,
        triggers,
    })
}

fn daemon_relaunch_command(exe: &Path, bind: Option<IpAddr>, port: u16) -> Vec<String> {
    let mut command = vec![
        exe.display().to_string(),
        "daemon".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];
    if let Some(bind) = bind {
        command.push("--bind".to_string());
        command.push(bind.to_string());
    }
    command
}

/// The address `pdo daemon` listens on: `--bind` (every interface by default)
/// and `--port`.
pub fn daemon_bind_addr(bind: Option<IpAddr>, port: u16) -> SocketAddr {
    SocketAddr::new(bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), port)
}

/// The configuration of `pdo daemon`: the environment, read once, plus the
/// command line an unsupervised update relaunches.
pub fn daemon_cli_config(bind: Option<IpAddr>, port: u16) -> DaemonConfig {
    let mut config = DaemonConfig::from_env();
    // #699: the command line an unsupervised update relaunches — the STABLE binary
    // path (Homebrew's `bin/pdo`, not the Cellar target `brew upgrade` deletes) and
    // the same port, whether it came from `--port` or `PDO_PORT`.
    config.relaunch_command = pdo_platform::adapters::host_installation::invoked_exe_path()
        .map(|exe| daemon_relaunch_command(&exe, bind, port));
    config
}

/// The SHARED existence gate for any `sandbox` reference a client can store (ADR-0031
/// §7). `""` is the *clear* sentinel and `off` means "no sandbox", so both are always
/// valid; anything else must name a profile that resolves.
///
/// A **write-time** gate, deliberately not the authoritative one — the create-run
/// chokepoint is, and it re-resolves at every fire. This one exists so the error lands
/// where the user can fix it. The `env` tier (`PDO_DEFAULT_SANDBOX`) never passes
/// through here, which is why `build_settings_view` also discloses a `reason` on
/// `default_sandbox`.
async fn validate_sandbox_ref(db: &sqlx::SqlitePool, raw: &str) -> Result<(), String> {
    let Some(mode) = event_log::SandboxMode::parse(raw) else {
        return Ok(()); // "" / whitespace = the clear sentinel
    };
    let Some(name) = mode.profile() else {
        return Ok(()); // `off`
    };
    match pdo_sandbox::application::profiles::exists(&sandbox_ports::profile_store(db), name).await
    {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "unknown sandbox profile `{name}`: use `off`, `{}`, `{}`, or a profile you \
             created under Settings → Staging profiles",
            pdo_sandbox::domain::profile::MINIMAL_PROFILE,
            pdo_sandbox::domain::profile::FULL_PROFILE,
        )),
        Err(e) => Err(format!("sandbox profile store error: {e}")),
    }
}

/// ROUTER TRAP: any NEW top-level path prefix (`/nodes`, `/stats`, `/fs`, `/audit`,
/// `/projects`, …) also needs its own entry in the vite dev proxy whitelist
/// (`frontend/vite.config.ts`). Without it a dev-mode request answers 200 with the
/// SPA shell and any smoke test lies. Sub-paths of an already-proxied prefix are
/// free (the proxy key is a prefix), and a static segment beside a `{param}` in the
/// same subtree is fine — axum 0.8 prefers the static one.
fn build_router(state: Arc<AppState>) -> Router {
    let router = Router::new()
        // The review of a Run (#926 moves it out): its diffs and its comments.
        .route("/runs/{run_id}/diff", get(run_diff))
        .route("/runs/{run_id}/diff/structured", get(run_diff_structured))
        .route("/runs/{run_id}/review/comments", get(list_review_comments))
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/reply",
            post(reply_review_comment),
        )
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/resolve",
            post(resolve_review_comment),
        )
        .route(
            "/runs/{run_id}/review/comments/{comment_id}/reopen",
            post(reopen_review_comment),
        )
        .route(
            "/runs/{run_id}/review/comments/send",
            post(send_review_comments),
        )
        // Instance-wide settings (ADR-0015). `GET` returns the per-field
        // {effective, source, stored, env, default} view; `PUT` writes the stored
        // tier, with fail-fast validation.
        .route("/settings", get(get_settings).put(put_settings))
        // Staging profiles (ADR-0031 §2-§7) sit UNDER `/settings` as a routing
        // decision only. It is NOT a claim about the schema: profiles are ROWS, not
        // an {effective, source, stored, env, default} knob, and they must NOT be
        // folded into `build_settings_view` beyond the name list.
        //
        // Price sync (ADR-0034) — the ONLY route in the crate that reaches the network.
        .route("/settings/cost-prices/sync", post(sync_cost_prices))
        .route(
            "/settings/agent-profiles",
            get(list_agent_profiles).post(create_agent_profile),
        )
        .route(
            "/settings/agent-profiles/{id}",
            get(get_agent_profile)
                .put(update_agent_profile)
                .delete(delete_agent_profile),
        )
        .route(
            "/settings/agent-profiles/{id}/referents",
            get(get_agent_profile_referents),
        )
        // Instance stats cockpit (ADR-0029). `overview` is cheap indexed SQL; `cost`
        // fans the memoized per-run cost over the visible period, so it is fetched
        // lazily, only when the cost tab is shown.
        .route("/stats/overview", get(stats::stats_overview))
        .route("/stats/cost", get(stats::stats_cost))
        .route(
            "/stats/performance",
            get(stats_performance::stats_performance),
        )
        // Absorptions (#888, ADR-0077): instance configuration, applied on read by
        // the three `/stats/*` folds above.
        .route(
            "/stats/absorptions",
            get(stats_absorption::list_absorptions).post(stats_absorption::create_absorption),
        )
        .route(
            "/stats/absorptions/member",
            axum::routing::delete(stats_absorption::delete_absorption_member),
        )
        .fallback(static_handler)
        .with_state(state.clone())
        .merge(
            pdo_workspace::adapters::http::router::<LegacyProjectSettings>(state.workspace.clone()),
        )
        .merge(pdo_skill::adapters::http::router(state.skills.clone()))
        // The run context (#923, #924, #925): the Run's surfaces, `/ws`, a Run's
        // detail and events, the node routes, the Pipeline Manager's commands, the
        // stale sweep's heartbeat.
        .merge(pdo_run::router(state.run.clone()));
    // In production the composition root puts `identify` in front of the whole
    // router; this crate's own tests drive `build_router` bare, as the local
    // operator.
    #[cfg(test)]
    let router = router.layer(axum::Extension(pdo_kernel::Actor::LocalOperator));
    router
}

async fn init_db(db: &sqlx::SqlitePool) -> Result<()> {
    // The event log and the tombstones of the forgotten Runs (ADR-0024).
    pdo_run::adapters::sqlite_event_store::init(db).await?;

    // The Triggers' tables belong to the trigger context (#927).
    pdo_trigger::adapters::sqlite::init(db)
        .await
        .context("failed to create trigger tables")?;

    instance_config::init(db)
        .await
        .context("failed to create instance_config table")?;

    // NO seed row: `minimal` and `full` are virtual defaults (ADR-0031 §2), so an
    // untouched install has zero rows here and still resolves both names.
    pdo_sandbox::adapters::sqlite_profiles::init(db)
        .await
        .context("failed to create sandbox_profiles table")?;

    // The audit journal (ADR-0044) and the page mounts belong to the platform
    // context, which creates its own tables (`Platform::init`).

    // Materialised on demand — nothing seeded.
    pdo_workspace::adapters::sqlite::init(db)
        .await
        .context("failed to create project and provisioning rules tables")?;

    // Unlike `project_store`, this table IS seeded: the reserved `Default` profile
    // must exist on every instance, immediately.
    agent_profile::init(db)
        .await
        .context("failed to create agent_profiles table")?;

    // The Banque de skills index (#668, ADR-0062). Content lives on disk under
    // `<repo_root>/.pdo/skills/<id>/`; nothing is seeded.
    pdo_skill::adapters::sqlite::init(db)
        .await
        .context("failed to create skill bank tables")?;

    // Stats absorptions (ADR-0077): instance configuration, nothing seeded.
    stats_absorption::init(db)
        .await
        .context("failed to create stats_absorptions table")?;

    Ok(())
}

// --- API handlers ---

/// One knob's `stored → env → default` disclosure for `GET /settings` (ADR-0015):
/// `effective` is what the daemon actually uses, produced by the same resolver, and
/// each tier is exposed so the UI can *reveal* a shadowed env var instead of ignoring
/// it. Values are in the knob's canonical unit — except the guard's `env`, which is
/// the raw `PDO_GUARD_TIMEOUT_MS` in milliseconds, the env seam's own unit.
fn settings_field(
    effective: i64,
    source: &str,
    stored: Option<i64>,
    env: Option<i64>,
    default: i64,
) -> serde_json::Value {
    serde_json::json!({
        "effective": effective,
        "source": source,
        "stored": stored,
        "env": env,
        "default": default,
    })
}

/// String sibling of [`settings_field`]. Every tier is a string-or-null: there is no
/// baked-in default model, so `default` is always `null` (the account default = no
/// `--model`).
fn settings_field_str(
    effective: Option<&str>,
    source: &str,
    stored: Option<&str>,
    env: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "effective": effective,
        "source": source,
        "stored": stored,
        "env": env,
        "default": serde_json::Value::Null,
    })
}

/// Boolean sibling of [`settings_field`] for a checkbox knob. Unlike
/// [`settings_field_str`], `default` is a real value.
fn settings_field_bool(
    effective: bool,
    source: &str,
    stored: Option<bool>,
    env: Option<bool>,
    default: bool,
) -> serde_json::Value {
    serde_json::json!({
        "effective": effective,
        "source": source,
        "stored": stored,
        "env": env,
        "default": default,
    })
}

/// The cached Docker availability probe (advisory: the run-advance fail-fast stays the
/// authoritative gate).
async fn docker_probe_cached(state: &AppState) -> pdo_sandbox::domain::image::DockerProbe {
    pdo_sandbox::application::docker_probe::probe_cached(
        &state.docker_probe_cache,
        run_sandbox::runtime(&state.run).docker,
    )
    .await
}

/// Probe every installed harness's catalogue once, populating the cache, so the
/// first `/settings` fetch answers warm.
async fn probe_harness_catalogues_at_boot(state: &Arc<AppState>) {
    let home = run_sandbox::sandbox_home_roots(&state.run)
        .ok()
        .map(|(h, _)| h);
    let registry = pdo_harness::adapters::descriptor_file::load_or_builtin(home.as_deref());
    pdo_harness::application::harness_offers::warm_catalogues(&state.harness_catalogue, &registry)
        .await;
}

/// Build the `GET /settings` view: per knob, the effective value, the winning tier,
/// and every tier's raw value (ADR-0015). Each `effective` MUST be computed by that
/// knob's own resolver, or the page drifts from what the daemon uses at spawn /
/// sweep / tick time.
async fn build_settings_view(state: &AppState) -> Result<serde_json::Value, sqlx::Error> {
    let db = &state.db;
    let cfg = instance_config::get(db).await?;

    let cap_default = admission::DEFAULT_SESSION_CAP as i64;
    let cap_stored_wins = cfg.session_cap.filter(|&n| n >= 1);
    let cap_env = pdo_run::adapters::env::env_cap().map(|n| n as i64);
    let cap_effective =
        pdo_run::adapters::env::configured_cap_with(cfg.session_cap.map(|n| n as usize)) as i64;
    let cap_source = if cap_stored_wins.is_some() {
        "stored"
    } else if cap_env.is_some() {
        "env"
    } else {
        "default"
    };

    let ttl_default = pdo_session::domain::settings::DEFAULT_REAPER_TTL.as_secs() as i64;
    let ttl_stored_wins = cfg.reaper_ttl_secs.filter(|&n| n >= 1);
    let ttl_env = pdo_session::adapters::env::env_reaper_ttl_secs().map(|n| n as i64);
    let ttl_effective = pdo_session::adapters::env::reaper_ttl_with(
        cfg.reaper_ttl_secs.map(|n| n as u64),
    )
    .as_secs() as i64;
    let ttl_source = if ttl_stored_wins.is_some() {
        "stored"
    } else if ttl_env.is_some() {
        "env"
    } else {
        "default"
    };

    let lat_default = pdo_session::domain::settings::DEFAULT_LIBASSIST_IDLE_TTL.as_secs() as i64;
    let lat_stored_wins = cfg.libassist_idle_ttl_secs.filter(|&n| n >= 1);
    let lat_env = pdo_session::adapters::env::env_libassist_idle_ttl_secs().map(|n| n as i64);
    let lat_effective = pdo_session::adapters::env::libassist_idle_ttl_with(
        cfg.libassist_idle_ttl_secs.map(|n| n as u64),
    )
    .as_secs() as i64;
    let lat_source = if lat_stored_wins.is_some() {
        "stored"
    } else if lat_env.is_some() {
        "env"
    } else {
        "default"
    };

    // The env seam is milliseconds; everything else here is seconds.
    let guard_default = pdo_trigger::domain::guard::GUARD_TIMEOUT_SECS as i64;
    let guard_stored_wins = cfg.guard_timeout_secs.filter(|&n| (1..=600).contains(&n));
    let guard_env_ms = pdo_trigger::adapters::env::env_guard_timeout_ms().map(|n| n as i64);
    let guard_effective =
        pdo_trigger::adapters::env::guard_timeout_with(cfg.guard_timeout_secs.map(|n| n as u64))
            .as_secs() as i64;
    let guard_source = if guard_stored_wins.is_some() {
        "stored"
    } else if guard_env_ms.is_some() {
        "env"
    } else {
        "default"
    };

    // The empty-string filter mirrors the resolver: a `""` stored/env value is
    // treated as unset, so it never wins the tier or discloses a bogus value.
    let dm_stored = cfg.default_model.as_deref().filter(|s| !s.is_empty());
    let dm_env = pdo_session::adapters::env::env_default_model();
    let dm_effective = pdo_session::adapters::env::default_model_with(cfg.default_model.clone());
    let dm_source = if dm_stored.is_some() {
        "stored"
    } else if dm_env.is_some() {
        "env"
    } else {
        // Effective=null, default=null ⇒ the account default (no --model).
        "default"
    };

    // `effective`=null ⇒ the `claude` floor applies at resolve time.
    let dh_stored = cfg.default_harness.as_deref().filter(|s| !s.is_empty());
    let dh_env = pdo_session::adapters::env::env_default_harness();
    let dh_effective =
        pdo_session::adapters::env::default_harness_with(cfg.default_harness.clone());
    let dh_source = if dh_stored.is_some() {
        "stored"
    } else if dh_env.is_some() {
        "env"
    } else {
        "default"
    };
    // The stored map plus the legacy `PDO_DEFAULT_MODEL` env folded under `claude`,
    // so the screen shows exactly what the resolver's `instance` tier will read.
    let dhm_stored = cfg.default_harness_model.clone();
    let mut dhm_effective = dhm_stored.clone();
    if !dhm_effective.contains_key(pdo_harness::registry::CLAUDE) {
        if let Some(m) = dm_effective.clone() {
            dhm_effective.insert(pdo_harness::registry::CLAUDE.to_string(), m);
        }
    }

    // `effective` is computed by the SAME resolver the create-run chokepoint consumes,
    // so the disclosed value cannot drift. This is the ONLY sandbox knob on this
    // screen: what a sandboxed Run *is* — image, home content, env — belongs to the
    // staging profile.
    let sbx_stored = cfg.default_sandbox.as_deref().filter(|s| !s.is_empty());
    let sbx_env = pdo_sandbox::adapters::env::DEFAULT_SANDBOX_ENV;
    let sbx_env_val = std::env::var(sbx_env)
        .ok()
        .as_deref()
        .and_then(event_log::SandboxMode::parse)
        .map(|m| m.as_str().to_string());
    let sbx_effective =
        pdo_sandbox::adapters::env::default_sandbox_with(cfg.default_sandbox.clone());
    let sbx_source = if sbx_stored.is_some() {
        "stored"
    } else if sbx_env_val.is_some() {
        "env"
    } else {
        "default"
    };
    // The winning tier may name a staging profile that does not exist, and every Run
    // it produces dies with a 400. `put_settings` gates the *stored* tier, but the
    // **env** tier passes through no validator by construction — so the only honest
    // place to surface it is here, as an advisory `reason` beside the value.
    let sbx_reason = match sbx_effective.profile() {
        None => None,
        Some(name) => {
            match pdo_sandbox::application::profiles::exists(
                &sandbox_ports::profile_store(db),
                name,
            )
            .await
            {
                Ok(true) => None,
                Ok(false) => Some(format!(
                    "no staging profile named `{name}` — every Run that falls back to this \
                 default will fail at launch (tier: {sbx_source})"
                )),
                Err(e) => Some(format!("cannot check staging profile `{name}`: {e}")),
            }
        }
    };

    // `effective` comes from the SAME resolver the sweep consumes. Stored is `0`/`1`;
    // both are a stored *decision* and both win over the env.
    let ate_stored = cfg.autocomplete_turn_end.map(|v| v != 0);
    let ate_env = stale_detector::env_autocomplete_turn_end();
    let ate_effective = stale_detector::autocomplete_turn_end_with(cfg.autocomplete_turn_end);
    let ate_source = if ate_stored.is_some() {
        "stored"
    } else if ate_env.is_some() {
        "env"
    } else {
        "default"
    };

    // Same discipline as `autocomplete_turn_end`, against the create chokepoint's
    // resolver.
    let dan_stored = cfg.default_auto_name.map(|v| v != 0);
    let dan_env = prompt_augmenter::env_default_auto_name();
    let dan_effective = prompt_augmenter::default_auto_name_with(cfg.default_auto_name);
    let dan_source = if dan_stored.is_some() {
        "stored"
    } else if dan_env.is_some() {
        "env"
    } else {
        "default"
    };

    // Same discipline, against `resolve_run_auto_fail`'s instance tier.
    let af_stored = cfg.auto_fail.map(|v| v != 0);
    let af_env = std::env::var(instance_config::AUTO_FAIL_ENV).ok().map(|s| {
        let t = s.trim();
        t.eq_ignore_ascii_case("1")
            || t.eq_ignore_ascii_case("true")
            || t.eq_ignore_ascii_case("yes")
            || t.eq_ignore_ascii_case("on")
    });
    let af_effective = instance_config::resolve_instance_auto_fail(cfg.auto_fail);
    let af_source = if af_stored.is_some() {
        "stored"
    } else if af_env.is_some() {
        "env"
    } else {
        "default"
    };

    // #697: the version-check switch, same `0`/`1` stored discipline.
    // #751: `review_agent_can_resolve`, against the reply endpoint's resolver.
    let racr_stored = cfg.review_agent_can_resolve.map(|v| v != 0);
    let racr_env = review_comments::env_review_agent_can_resolve();
    let racr_effective =
        review_comments::review_agent_can_resolve_with(cfg.review_agent_can_resolve);
    let racr_source = if racr_stored.is_some() {
        "stored"
    } else if racr_env.is_some() {
        "env"
    } else {
        "default"
    };

    let uc_stored = cfg.update_check.map(|v| v != 0);
    let uc_env = pdo_platform::adapters::env::env_update_check();
    let uc_effective =
        pdo_platform::domain::update_check::resolve_update_check(cfg.update_check, uc_env);
    let uc_source = if uc_stored.is_some() {
        "stored"
    } else if uc_env.is_some() {
        "env"
    } else {
        "default"
    };

    // Manager on demand: the auto-start flag, `stored → env → default(false)` —
    // the SAME resolver the create-run chokepoint consumes, so the disclosed
    // value cannot drift.
    let me_stored = cfg.manager_enabled.map(|v| v != 0);
    let me_env = std::env::var(instance_config::MANAGER_ENABLED_ENV)
        .ok()
        .map(|s| {
            let t = s.trim();
            t.eq_ignore_ascii_case("1")
                || t.eq_ignore_ascii_case("true")
                || t.eq_ignore_ascii_case("yes")
                || t.eq_ignore_ascii_case("on")
        });
    let me_effective = instance_config::resolve_manager_enabled(cfg.manager_enabled);
    let me_source = if me_stored.is_some() {
        "stored"
    } else if me_env.is_some() {
        "env"
    } else {
        "default"
    };
    // #779: the per-run attachment budget, `stored → env → default(50 MB)` —
    // the SAME resolver the `POST /runs` multipart branch consumes.
    let att_stored = cfg.max_attachments_mb.filter(|&n| n >= 1);
    let att_env = env_max_attachments_mb();
    let att_effective = resolve_max_attachments_mb(cfg.max_attachments_mb);
    let att_source = if att_stored.is_some() {
        "stored"
    } else if att_env.is_some() {
        "env"
    } else {
        "default"
    };
    // The profile pin: a stored agent-profile NAME, or unset (« Follow the
    // Run »). No env tier — unlike the flag, a pin is a pure stored decision.
    let mp_stored = cfg.manager_profile.as_deref().filter(|s| !s.is_empty());
    let mp_source = if mp_stored.is_some() {
        "stored"
    } else {
        "default"
    };

    // Not a `settings_field` — it has no stored/env/default tier. Folded in here so
    // the modal learns the default AND whether Docker can run a sandbox in ONE fetch.
    // Advisory: it grays out `full`/`minimal`, but the run-advance fail-fast
    // (ADR-0030 pt 4) stays the authoritative gate.
    let docker_probe = docker_probe_cached(state).await;

    // NAMES ONLY is part of the contract, not an oversight: this payload is on the hot
    // path of the launch dialog, which fetches settings on every open. Serving resolved
    // entry lists here would make a `<select>` that needs three strings pay a
    // per-profile fold. The editor reads `GET /settings/sandbox-profiles` for the rest.
    //
    // `home` is an observed FACT, not a tier: the filesystem explorer yields ABSOLUTE
    // paths while a profile entry is RELATIVE to `$HOME`. It MUST honour
    // `sandbox_home_override`, or the layer-3 harness and the daemon disagree about
    // what "under $HOME" means.
    let profile_names =
        pdo_sandbox::application::profiles::names(&sandbox_ports::profile_store(db))
            .await
            .map_err(|e| sqlx::Error::Protocol(e.0))?;
    let sandbox_profiles: Vec<serde_json::Value> = profile_names
        .iter()
        .map(|(name, is_virtual)| serde_json::json!({ "name": name, "virtual": is_virtual }))
        .collect();
    let host_home_path = run_sandbox::sandbox_home_roots(&state.run)
        .ok()
        .map(|(h, _)| h);
    let host_home = host_home_path
        .as_ref()
        .map(|h| h.to_string_lossy().into_owned());

    // An observed STATE, not a settings knob: no stored/env/default tier, only which
    // of the three price tiers won. A hand-edited file passes through NO validator by
    // construction, so the only honest place to surface a refused row is here, as an
    // advisory `reason`.
    //
    // Both paths are ALWAYS reported, even when neither file exists — nothing is ever
    // seeded (ADR-0031 §2), so naming the paths IS the entire discoverability story.
    //
    // The resolved read view lives on `/stats/cost` instead, beside the "Sync costs"
    // action, so pressing sync and reading what PDO can price happen in one place.
    let price_table_view = match &host_home_path {
        Some(home) => {
            let table = price_table::PriceTable::load(home);
            let (manual_path, fetched_path) = price_table::PriceTable::paths(home);
            serde_json::json!({
                "manual_path": manual_path.to_string_lossy(),
                "fetched_path": fetched_path.to_string_lossy(),
                "source": table.source(),
                "fetched_at": table.fetched_at(),
                "fetched_rows": table.fetched_rows(),
                "manual_keys": table.manual_keys(),
                // The SAME string the loader logs, so the page and the journal cannot
                // disagree.
                "reason": table.diagnostic(),
            })
        }
        None => serde_json::json!({
            "manual_path": null,
            "fetched_path": null,
            "source": null,
            "fetched_at": null,
            "fetched_rows": 0,
            "manual_keys": [],
            "reason": "HOME is unset, so the price files have no resolvable path — \
                       only the prices compiled into the binary apply",
        }),
    };

    // The disk descriptor tier (ADR-0045), same posture as `price_table` above: a
    // hand-edited descriptor file passes through NO validator, so the only honest
    // place to say a descriptor is inert or refused is here. The harness context
    // builds the whole block: each harness's offer, the refusals, the path.
    let registry =
        pdo_harness::adapters::descriptor_file::load_or_builtin(host_home_path.as_deref());
    let offers =
        pdo_harness::application::harness_offers::list_offers(&state.harness_catalogue, &registry)
            .await;
    let harness_descriptors_view = pdo_harness::adapters::http::descriptors_view::descriptors_view(
        host_home_path.as_deref(),
        &registry,
        &offers,
    );

    Ok(serde_json::json!({
        "session_cap": settings_field(cap_effective, cap_source, cfg.session_cap, cap_env, cap_default),
        "reaper_ttl_secs": settings_field(ttl_effective, ttl_source, cfg.reaper_ttl_secs, ttl_env, ttl_default),
        // Its own knob, deliberately not a reuse of `reaper_ttl_secs`: that one bounds
        // a completed node's session (1 h), this one a REPL nobody is using (120 s).
        "libassist_idle_ttl_secs": settings_field(lat_effective, lat_source, cfg.libassist_idle_ttl_secs, lat_env, lat_default),
        "guard_timeout_secs": settings_field(guard_effective, guard_source, cfg.guard_timeout_secs, guard_env_ms, guard_default),
        "default_model": settings_field_str(dm_effective.as_deref(), dm_source, dm_stored, dm_env.as_deref()),
        // `default_harness_model` is a per-harness map, disclosed as
        // {effective, stored} so the screen shows both what is set and what the
        // resolver will read.
        "default_harness": settings_field_str(dh_effective.as_deref(), dh_source, dh_stored, dh_env.as_deref()),
        "default_harness_model": {
            "effective": dhm_effective,
            "stored": dhm_stored,
        },
        "agent_choice": cfg.agent_choice,
        // #669: the Instance tier of the skills selection, as stored (ids + labels).
        "skills": cfg.skills,
        // The settings surface is where a broken descriptor is ALWAYS said.
        "harness_descriptors": harness_descriptors_view,
        "default_sandbox": {
            "effective": sbx_effective.as_str(),
            "source": sbx_source,
            "stored": sbx_stored,
            "env": sbx_env_val,
            "default": event_log::SandboxMode::OFF_WIRE,
            // `null` when the winning tier resolves. Only `default_sandbox` carries a
            // `reason`: it is the one enum knob whose value space is open (a profile
            // name), hence the one that can point at nothing.
            "reason": sbx_reason,
        },
        "sandbox_profiles": sandbox_profiles,
        "home": host_home,
        "price_table": price_table_view,
        "sandbox_docker": {
            "available": docker_probe.available,
            "reason": docker_probe.reason,
            "checked_at": pdo_kernel::clock::now_iso(),
        },
        "autocomplete_turn_end": settings_field_bool(
            ate_effective,
            ate_source,
            ate_stored,
            ate_env,
            stale_detector::AUTOCOMPLETE_TURN_END_DEFAULT,
        ),
        "default_auto_name": settings_field_bool(
            dan_effective,
            dan_source,
            dan_stored,
            dan_env,
            prompt_augmenter::DEFAULT_AUTO_NAME_DEFAULT,
        ),
        // The global tier of `auto_fail`. Default `false`: an agent `pdo fail` parks
        // the run for a human to confirm.
        "auto_fail": settings_field_bool(af_effective, af_source, af_stored, af_env, false),
        // #697: may the daemon ask the release source at all. Default ON.
        "update_check": settings_field_bool(
            uc_effective,
            uc_source,
            uc_stored,
            uc_env,
            pdo_platform::domain::update_check::UPDATE_CHECK_DEFAULT,
        ),
        // #751: may an agent's `pdo review reply --resolved` resolve the comment
        // directly? Default OFF: the human resolves, the agent proposes (ADR-0067 §4).
        "review_agent_can_resolve": settings_field_bool(
            racr_effective,
            racr_source,
            racr_stored,
            racr_env,
            review_comments::REVIEW_AGENT_CAN_RESOLVE_DEFAULT,
        ),
        // Manager on demand: off by default — a Run starts managerless and the
        // Manager tab's Start button (or the Settings toggle for future Runs)
        // brings one up.
        "manager_enabled": settings_field_bool(
            me_effective,
            me_source,
            me_stored,
            me_env,
            instance_config::MANAGER_ENABLED_DEFAULT,
        ),
        // The agent-profile NAME the manager is pinned to, or null (« Follow the
        // Run » — the manager mirrors the Run's harness/model/effort). A stored
        // name whose profile was deleted dangles: the UI renders the #432
        // tombstone, and the spawn falls back to « Follow the Run ».
        "manager_profile": settings_field_str(mp_stored, mp_source, mp_stored, None),
        // #779: images + files budget per Run, in MB. Bounds the multipart body
        // of `POST /runs` and drives the modal's client-side gate.
        "max_attachments_mb": settings_field(
            att_effective,
            att_source,
            att_stored,
            att_env,
            MAX_ATTACHMENTS_MB_DEFAULT,
        ),
        "updated_at": cfg.updated_at,
    }))
}

/// Resolve the price-source URL for this daemon: the per-daemon override (the
/// layer-3/5 seam) else the shipped default.
fn price_source_url(state: &AppState) -> String {
    state
        .price_source_url
        .clone()
        .unwrap_or_else(|| price_table::PRICE_SOURCE_URL_DEFAULT.to_string())
}

/// The host home the price files live under, or a message explaining why there is
/// none. Prices are an **instance** concept: even for a sandboxed Run this is the
/// HOST home — the sandbox seam moves the transcript root, not this one.
///
/// Returns the *message*, not a `Response`: `Result<_, Response>` trips
/// `clippy::result_large_err` and CI is `-D warnings`.
fn price_home_root(state: &AppState) -> Result<PathBuf, String> {
    run_sandbox::sandbox_home_roots(&state.run)
        .map(|(home, _)| home)
        .map_err(|e| format!("cannot resolve the home root for the price files: {e}"))
}

/// Outcome of one price sync, for the report. Split out so the pure diff is
/// testable without an HTTP round-trip.
struct PriceSyncOutcome {
    rows: usize,
    added: Vec<String>,
    updated: Vec<String>,
    unchanged: usize,
    shadowed_by_manual: Vec<String>,
    changed: bool,
}

/// Diff a freshly normalised fetched tier against the table currently in force.
/// PURE. `changed` is keyed on the RAW fetched rows, NOT on effective prices: a
/// shadowed key whose source price moved is a real change to the fetched tier even
/// though nothing a user sees moves.
fn diff_price_sync(
    before: &price_table::PriceTable,
    fresh: &std::collections::BTreeMap<String, price_table::Price>,
) -> PriceSyncOutcome {
    let mut out = PriceSyncOutcome {
        rows: fresh.len(),
        added: Vec::new(),
        updated: Vec::new(),
        unchanged: 0,
        shadowed_by_manual: Vec::new(),
        changed: fresh != before.fetched_prices(),
    };
    for (key, fresh_price) in fresh {
        let shadowed = before.tier_of(key) == Some(price_table::PriceTier::Manual);
        if shadowed {
            out.shadowed_by_manual.push(key.clone());
        }
        let effective_before = before.price_for(key);
        let effective_after = if shadowed {
            effective_before
        } else {
            Some(*fresh_price)
        };
        match effective_before {
            None => out.added.push(key.clone()),
            Some(_) if effective_before != effective_after => out.updated.push(key.clone()),
            Some(_) => out.unchanged += 1,
        }
    }
    out
}

/// `POST /settings/cost-prices/sync` — fetch the remote price source and rewrite
/// the fetched tier (ADR-0034).
///
/// NOT a `POST /runs/:id/commands` kind: that dispatcher is run-scoped, and a price
/// sync touches neither the event log nor a Run projection. NOT under `/stats`
/// either — ADR-0029 makes that a read-derivation surface.
///
/// Codes: **409** a sync is already in flight · **502** source unreachable / broken
/// JSON / empty harvest, and **nothing is written** · **500** a disk write failed ·
/// **200** otherwise, with `noop: true` + `reason` when the table did not move
/// (ADR-0025 forbids a blind `{ok:true}`).
///
/// The 502 is deliberate: classing a network cut as a 500 would file it as a daemon
/// defect. An explicitly requested effect that fails is a HARD error that NAMES the
/// source, never a silent fallback (ADR-0030).
async fn sync_cost_prices(State(state): State<Arc<AppState>>) -> Response {
    let home_root = match price_home_root(&state) {
        Ok(h) => h,
        Err(msg) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };
    // `try_lock`, not `.lock()`: a second click brings nothing, and ADR-0025 wants it
    // said rather than queued silently.
    let Ok(_guard) = state.price_sync_lock.try_lock() else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "a price sync is already in flight" })),
        )
            .into_response();
    };

    let url = price_source_url(&state);
    let before = price_table::PriceTable::load(&home_root);

    let body = match price_table::fetch_source(&url).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("price source unreachable: {url}: {e}")
                })),
            )
                .into_response();
        }
    };
    let (fresh, rejected) = match price_table::normalize_models_dev(&body) {
        Ok(v) => v,
        Err(e) => {
            // Includes the empty-harvest guard: an upstream schema drift must NOT be
            // able to write an empty file over the last known table.
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": format!("price source unusable: {url}: {e}")
                })),
            )
                .into_response();
        }
    };

    let outcome = diff_price_sync(&before, &fresh);
    let (_, fetched_path) = price_table::PriceTable::paths(&home_root);
    let rejected_msgs: Vec<String> = rejected
        .iter()
        .map(|r| format!("{}: {}", r.key, r.why))
        .collect();

    if !outcome.changed {
        // Nothing to write, so nothing is written — which leaves the table's
        // fingerprint alone and keeps the cost-breakdown memo warm.
        return Json(serde_json::json!({
            "ok": true,
            "noop": true,
            "reason": format!(
                "table already up to date — {} row(s) from the source, none changed",
                outcome.rows
            ),
            "source": url,
            "fetched_at": before.fetched_at(),
            "rows": outcome.rows,
            "added": Vec::<String>::new(),
            "updated": Vec::<String>::new(),
            "unchanged": outcome.unchanged,
            "rejected": rejected_msgs,
            "shadowed_by_manual": outcome.shadowed_by_manual,
        }))
        .into_response();
    }

    let fetched_at = pdo_kernel::clock::now_iso();
    if let Err(e) = price_table::write_fetched(&fetched_path, &url, &fetched_at, &fresh) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("cannot write {}: {e}", fetched_path.display())
            })),
        )
            .into_response();
    }

    // Tell every connected client the table moved, so an open Stats modal is not the
    // only thing that knows.
    let _ = state.bus_tx.send(serde_json::json!({
        "type": "cost_prices_synced",
        "rows": outcome.rows,
    }));

    Json(serde_json::json!({
        "ok": true,
        "source": url,
        "fetched_at": fetched_at,
        "rows": outcome.rows,
        "added": outcome.added,
        "updated": outcome.updated,
        "unchanged": outcome.unchanged,
        "rejected": rejected_msgs,
        "shadowed_by_manual": outcome.shadowed_by_manual,
    }))
    .into_response()
}

/// Refresh the fetched price tier if — and only if — it already exists and is stale
/// (ADR-0034).
///
/// Three gates, in this order, each load-bearing:
/// 1. the config flag;
/// 2. **`fetched.json` must already exist** — the boot pass REFRESHES, it never
///    seeds, so there is no egress before the user's first explicit click (which
///    also keeps the integration daemons, booting from a fresh `HOME`, offline);
/// 3. it must be older than [`price_table::PRICE_REFRESH_MAX_AGE`].
///
/// A failure is a `warn!`, never fatal: a daemon restarted by systemd can come up
/// before DNS is ready, and a boot must not fail on that.
async fn refresh_prices_at_boot(state: &Arc<AppState>) {
    if !state.price_refresh_at_boot {
        return;
    }
    let Ok(home_root) = run_sandbox::sandbox_home_roots(&state.run).map(|(h, _)| h) else {
        return; // no HOME → no path → nothing to refresh, silently
    };
    let (_, fetched_path) = price_table::PriceTable::paths(&home_root);
    if !fetched_path.exists() {
        // The user has never synced; asking the network now would be an egress they
        // did not consent to.
        return;
    }
    let table = price_table::PriceTable::load(&home_root);
    if let Some(fetched_at) = table.fetched_at() {
        if let Ok(then) = chrono::DateTime::parse_from_rfc3339(fetched_at) {
            let age = chrono::Utc::now().signed_duration_since(then.with_timezone(&chrono::Utc));
            if age
                < chrono::Duration::from_std(price_table::PRICE_REFRESH_MAX_AGE)
                    .unwrap_or_else(|_| chrono::Duration::hours(24))
            {
                return; // fresh → noop
            }
        }
        // An unparseable `fetched_at` falls through and refreshes: a cache whose
        // vintage cannot be read is one worth re-fetching.
    }

    // Same lock as the button. Losing the race means a manual sync is running, which
    // does strictly better — give up without a word.
    let Ok(_guard) = state.price_sync_lock.try_lock() else {
        return;
    };
    let url = price_source_url(state);
    let body = match price_table::fetch_source(&url).await {
        Ok(b) => b,
        Err(e) => {
            warn!("price refresh at boot: {url} unreachable ({e}) — keeping the current table");
            return;
        }
    };
    let fresh = match price_table::normalize_models_dev(&body) {
        Ok((rows, _)) => rows,
        Err(e) => {
            warn!("price refresh at boot: {url} unusable ({e}) — keeping the current table");
            return;
        }
    };
    if fresh == *table.fetched_prices() {
        return; // nothing moved: leave the file, and the memo, alone
    }
    match price_table::write_fetched(&fetched_path, &url, &pdo_kernel::clock::now_iso(), &fresh) {
        Ok(()) => info!("price refresh at boot: {} row(s) from {url}", fresh.len()),
        Err(e) => warn!(
            "price refresh at boot: cannot write {}: {e}",
            fetched_path.display()
        ),
    }
}

/// `GET /settings` — the instance-wide config, per knob, as
/// `{effective, source, stored, env, default}` (ADR-0015). `GET /sessions` stays the
/// lean status-bar view.
async fn get_settings(State(state): State<Arc<AppState>>) -> Response {
    match build_settings_view(&state).await {
        Ok(view) => Json(view).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// The ONE boot line owed to a user whose database still holds `image_source` or
/// `dockerfile_path`: those values are now **inert**, and a value that stops
/// mattering must never do so in silence.
///
/// Returns `Option` rather than logging, so "nothing stored ⇒ nothing said" is a
/// property of the value, not of a branch at the call site. ONE line for both
/// columns: two lines for one obsolete decision would read like two problems.
///
/// `profile_hint` is the profile a Run currently falls back to — the concrete thing
/// to go and edit. `None` degrades to naming the screen instead.
fn retired_sandbox_settings_warning(
    stored: &[(&'static str, String)],
    profile_hint: Option<&str>,
) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let values = stored
        .iter()
        .map(|(field, value)| format!("`{field}` = `{value}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let where_to_go = match profile_hint {
        Some(name) => format!("the staging profile `{name}`, which is this instance's default"),
        None => "the staging profile your Runs launch with".to_string(),
    };
    let env_vars = instance_config::RETIRED_SANDBOX_IMAGE_COLUMNS
        .iter()
        .map(|(_, env)| format!("`{env}`"))
        .collect::<Vec<_>>()
        .join(" / ");
    Some(format!(
        "instance_config still stores {values}, and #471 made those values IGNORED — a staging \
         profile's own image source decides now. Set the image on {where_to_go} (Settings → \
         Manage staging profiles…), or set {env_vars} in the daemon's environment to move the \
         built-in default. No Run is affected either way."
    ))
}

/// Say the above ONCE per boot, right after the schema is up. Best-effort: a DB read
/// failure here must not stop a daemon from starting over a message about a setting
/// that is already inert.
async fn warn_about_retired_sandbox_settings(db: &sqlx::SqlitePool) {
    let stored = match instance_config::retired_sandbox_image_values(db).await {
        Ok(stored) => stored,
        Err(e) => {
            warn!("#471: cannot check for retired sandbox image settings: {e}");
            return;
        }
    };
    let hint = instance_config::get(db).await.ok().and_then(|cfg| {
        pdo_sandbox::adapters::env::default_sandbox_with(cfg.default_sandbox)
            .profile()
            .map(String::from)
    });
    if let Some(msg) = retired_sandbox_settings_warning(&stored, hint.as_deref()) {
        warn!("{msg}");
    }
}

/// The message a `PUT /settings` gets for a retired field. It has to answer the only
/// question the caller has — *where did it go?* — hence naming the profile editor AND
/// the surviving env var; a bare "unknown field" would be true and useless.
fn retired_setting_message(field: &str, env_var: &str) -> String {
    format!(
        "`{field}` is no longer an instance-wide setting: a staging profile's own image source \
         replaced it (#471). Set the image on the profile itself (Settings → Manage staging \
         profiles…), or set `{env_var}` in the daemon's environment to move the built-in default."
    )
}

/// `PUT /settings` — persist the stored tier of one or more knobs, then return the
/// recomputed [`build_settings_view`] so the UI shows the new effective/source
/// without a second round-trip (ADR-0015). Validation is fail-fast: an out-of-range
/// value is a `400` before anything is persisted — never a silent default like the
/// env parser.
///
/// The body is taken as raw JSON rather than straight into
/// [`instance_config::UpdateInstanceConfig`] for ONE reason: a *retired* field must
/// 400 **naming itself**, not be silently ignored by serde and answered 200 as though
/// applied. It also turns axum's opaque 422 on a type error into a 400 with a message.
async fn put_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let bad = |msg: &str| -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response()
    };
    // Checked BEFORE parsing, so the answer is about the field the caller sent rather
    // than about the shape of the struct that no longer has it.
    for (field, env_var) in instance_config::RETIRED_SANDBOX_IMAGE_COLUMNS {
        if body.get(field).is_some() {
            return bad(&retired_setting_message(field, env_var));
        }
    }
    let req: instance_config::UpdateInstanceConfig = match serde_json::from_value(body) {
        Ok(req) => req,
        Err(e) => return bad(&format!("invalid settings payload: {e}")),
    };
    if let Some(c) = req.session_cap {
        if c < 1 {
            return bad("session_cap must be >= 1");
        }
    }
    if let Some(t) = req.reaper_ttl_secs {
        if t < 1 {
            return bad("reaper_ttl_secs must be >= 1");
        }
    }
    if let Some(t) = req.libassist_idle_ttl_secs {
        if t < 1 {
            return bad("libassist_idle_ttl_secs must be >= 1");
        }
    }
    if let Some(g) = req.guard_timeout_secs {
        if !(1..=600).contains(&g) {
            return bad("guard_timeout_secs must be between 1 and 600 seconds");
        }
    }
    if let Some(m) = req.max_attachments_mb {
        // #779: at least 1 MB, and bounded so the budget stays a sane
        // in-memory multipart body (the parts are buffered before the write).
        if !(1..=4096).contains(&m) {
            return bad("max_attachments_mb must be between 1 and 4096");
        }
    }
    if let Some(s) = req.default_sandbox.as_deref() {
        // "" = clear sentinel; anything else must be `off` or a staging profile that
        // RESOLVES. Same shared gate as the Trigger surfaces.
        if let Err(msg) = validate_sandbox_ref(&state.db, s).await {
            return bad(&msg);
        }
    }
    if let Some(p) = req.manager_profile.as_deref() {
        // "" = clear sentinel (back to « Follow the Run »); anything else must
        // name an EXISTING agent profile. A pin to nothing would silently fall
        // back at spawn time, so it is refused at registration — the #432
        // tombstone then only ever arises from a LATER profile deletion/rename,
        // which no PUT can prevent.
        if !p.is_empty() {
            match agent_profile::find_by_name_ci(&state.db, p).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let names = agent_profile::list(&state.db)
                        .await
                        .map(|profiles| {
                            profiles
                                .iter()
                                .map(|profile| profile.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    return bad(&format!(
                        "unknown manager profile `{p}`: it names no agent profile — a pin to \
                         nothing would silently fall back to « Follow the Run ». Known profiles: {names}"
                    ));
                }
                Err(e) => {
                    return bad(&format!("cannot check agent profile `{p}`: {e}"));
                }
            }
        }
    }
    if let Some(h) = req.default_harness.as_deref() {
        // "" = clear sentinel; anything else must RESOLVE against the registry. A
        // default harness that resolves to nothing would break EVERY spawn that falls
        // through to it, so it is refused at registration.
        if !h.is_empty() {
            let home_root = run_sandbox::sandbox_home_roots(&state.run)
                .map(|(home, _)| home)
                .unwrap_or_default();
            let registry = pdo_harness::adapters::descriptor_file::load(&home_root);
            if registry.resolve(h).is_none() {
                return bad(&format!(
                    "unknown default harness `{h}`: it resolves to no embedded or declared \
                     harness — a default that does not resolve would break every spawn that \
                     falls through to it. Known harnesses: {}",
                    registry.names().join(", ")
                ));
            }
        }
    }

    let update_check_edit = req.update_check;
    match instance_config::update(&state.db, req).await {
        Ok(_) => {
            // #697: re-enabling the check runs one at once, detached — the section
            // reads "Not checked yet since re-enabling." until it lands. Turning it
            // off needs nothing: the read path already answers `null` when off.
            if update_check_edit == Some(true) {
                state.platform.check_for_update_in_background();
            }
            match build_settings_view(&state).await {
                Ok(view) => Json(view).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "AgentProfileInput")]
struct WriteAgentProfileRequest {
    name: String,
    harness: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

fn agent_profile_error_response(error: agent_profile::AgentProfileError) -> Response {
    let status = match error {
        agent_profile::AgentProfileError::NotFound => StatusCode::NOT_FOUND,
        agent_profile::AgentProfileError::DuplicateName { .. } => StatusCode::CONFLICT,
        agent_profile::AgentProfileError::EmptyName
        | agent_profile::AgentProfileError::EmptyHarness
        | agent_profile::AgentProfileError::DefaultUndeletable => StatusCode::BAD_REQUEST,
        agent_profile::AgentProfileError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

async fn list_agent_profiles(State(state): State<Arc<AppState>>) -> Response {
    match agent_profile::list(&state.db).await {
        Ok(profiles) => Json(serde_json::json!({ "profiles": profiles })).into_response(),
        Err(error) => {
            error!("failed to list agent profiles: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    }
}

async fn get_agent_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match agent_profile::get(&state.db, &id).await {
        Ok(Some(profile)) => Json(profile).into_response(),
        Ok(None) => agent_profile_error_response(agent_profile::AgentProfileError::NotFound),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn create_agent_profile(
    State(state): State<Arc<AppState>>,
    Json(req): Json<WriteAgentProfileRequest>,
) -> Response {
    match agent_profile::create(
        &state.db,
        &req.name,
        &req.harness,
        req.model.as_deref(),
        req.effort.as_deref(),
    )
    .await
    {
        Ok(profile) => (StatusCode::CREATED, Json(profile)).into_response(),
        Err(error) => agent_profile_error_response(error),
    }
}

async fn update_agent_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<WriteAgentProfileRequest>,
) -> Response {
    match agent_profile::update(
        &state.db,
        &id,
        &req.name,
        &req.harness,
        req.model.as_deref(),
        req.effort.as_deref(),
    )
    .await
    {
        Ok(profile) => Json(profile).into_response(),
        Err(error) => agent_profile_error_response(error),
    }
}

async fn delete_agent_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match agent_profile::delete(&state.db, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => agent_profile_error_response(agent_profile::AgentProfileError::NotFound),
        Err(error) => agent_profile_error_response(error),
    }
}

async fn get_agent_profile_referents(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !matches!(agent_profile::get(&state.db, &id).await, Ok(Some(_))) {
        return agent_profile_error_response(agent_profile::AgentProfileError::NotFound);
    }

    let instance = instance_config::get(&state.db)
        .await
        .ok()
        .and_then(|config| config.agent_choice)
        .is_some_and(|choice| {
            matches!(choice, pdo_harness::agent_choice::AgentChoice::Profile { profile_id } if profile_id == id)
        });

    let projects = pdo_workspace::adapters::sqlite::list(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|project| {
            matches!(
                project_agent_choice(project),
                Some(pdo_harness::agent_choice::AgentChoice::Profile { profile_id }) if profile_id == id
            )
        })
        .map(|project| serde_json::json!({ "id": project.id, "name": project.name }))
        .collect::<Vec<_>>();

    let triggers = pdo_trigger::adapters::sqlite::list(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|trigger| {
            matches!(
                trigger.agent_choice.as_ref(),
                Some(pdo_harness::agent_choice::AgentChoice::Profile { profile_id }) if profile_id == &id
            )
        })
        .map(|trigger| {
            serde_json::json!({
                "id": trigger.id,
                "name": trigger.name,
                "pipeline_id": trigger.pipeline_id
            })
        })
        .collect::<Vec<_>>();

    let pipeline_entries = instance_pipeline_dir(&state.repo_root)
        .map(|directory| pdo_pipeline::adapters::registry::pipeline_files(&directory))
        .unwrap_or_default();
    let pipelines = pipeline_entries
        .into_iter()
        .flat_map(|entry| {
            let Ok(yaml) = std::fs::read_to_string(&entry.path) else {
                return Vec::new();
            };
            let Ok(parsed) = pipeline::parse_pipeline(&yaml) else {
                return Vec::new();
            };
            let name = parsed.pipeline.name.clone();
            parsed
                .pipeline
                .nodes
                .into_iter()
                .filter(|node| {
                    matches!(
                        node.agent_choice.as_ref(),
                        Some(pdo_harness::agent_choice::AgentChoice::Profile { profile_id }) if profile_id == &id
                    )
                })
                .map(|node| {
                    serde_json::json!({
                        "id": entry.id,
                        "name": name,
                        "node_id": node.id,
                        "scope": "instance"
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    Json(serde_json::json!({
        "profile_id": id,
        "instance": instance,
        "projects": projects,
        "triggers": triggers,
        "pipelines": pipelines,
        "runs": []
    }))
    .into_response()
}

async fn run_diff(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => return (StatusCode::NOT_FOUND, "run not found").into_response(),
    };

    let pipeline_branch = format!("pdo/run-{run_id}");
    // Base on the Run's frozen fork point, NOT the shared checkout's wandering HEAD:
    // a checkout parked on a divergent branch would sweep in phantom files. Three-dot
    // (merge-base = fork point) so main's advance after the fork does not surface as
    // phantom deletions; `:(exclude).pdo/` drops the blackboard artefacts. Mirrors
    // `compute_run_loc`.
    let base = run_diff_base(&run_state);
    let range = format!("{base}...{pipeline_branch}");
    // #748: the Run's branch lives in its EFFECTIVE repository (ADR-0033), never in
    // the daemon's cwd — computing here in `state.repo_root` is why the diff never
    // rendered for a Run targeting another repo.
    let repo = effective_repo_root(&state, &run_state);
    let output = match std::process::Command::new("git")
        .args(["diff", &range, "--", ".", ":(exclude).pdo/"])
        .current_dir(&repo)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("git diff failed: {e}"),
            )
                .into_response();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unknown revision") || stderr.contains("not a git repository") {
            return (StatusCode::NOT_FOUND, "run branch not found").into_response();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("git diff failed: {stderr}"),
        )
            .into_response();
    }

    let diff = String::from_utf8_lossy(&output.stdout);
    (StatusCode::OK, diff.into_owned()).into_response()
}

#[derive(Deserialize)]
struct StructuredDiffQuery {
    /// Source ref. Defaults to the Run's fork point (`run_diff_base`).
    from: Option<String>,
    /// Destination ref. Defaults to the Run's working tree while it exists
    /// (#835), else to the Run's tip (`pdo/run-<id>`).
    to: Option<String>,
}

/// `GET /runs/<id>/diff/structured[?from=<ref>&to=<ref>]` (#748, ADR-0067).
///
/// Files → hunks → lines for a pair of Run refs, computed in the Run's
/// **effective** repository. With no query the pair is fork → worktree while
/// the Run's worktree exists (#835: commits *and* uncommitted edits, so a
/// running node needs no commit to be reviewed), fork → tip once it is gone;
/// either way the range is three-dot with `.pdo/` excluded — the bounds of the
/// LOC stat, so "counted" and "shown" agree (a clean tree makes both defaults
/// coincide). An explicit pair is compared two-dot (two arbitrary Run refs: a
/// node's `before`/`after`, …). The raw-patch endpoints stay as they are.
async fn run_diff_structured(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(q): Query<StructuredDiffQuery>,
) -> Response {
    let (events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    // #749: a side is a stable Run ref id (`fork`, `tip`, `worktree`,
    // `node:<id>:<iter>:before`, `live:<id>`) resolved against the log, or —
    // compatibility — a raw git ref.
    let worktree = run_worktree_dir(&state.run, &run_state, &run_id);
    let from = match resolve_ref_param(
        &run_id,
        &run_state,
        &events,
        q.from.as_deref(),
        run_refs::FORK_ID,
    ) {
        Ok(RefTarget::Git(r)) => r,
        Ok(RefTarget::Worktree) => {
            return (
                StatusCode::BAD_REQUEST,
                "the working tree can only be the destination of a diff",
            )
                .into_response();
        }
        Err(resp) => return *resp,
    };
    let to = match resolve_ref_param(
        &run_id,
        &run_state,
        &events,
        q.to.as_deref(),
        default_to_id(worktree.as_ref()),
    ) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    // Three-dot only for the fork → tip/worktree default (the LOC bounds); any
    // other pair is two arbitrary Run refs compared two-dot.
    let three_dot = run_refs::is_default_pair(q.from.as_deref(), q.to.as_deref());
    let result = match to {
        RefTarget::Git(to) => {
            let repo = effective_repo_root(&state, &run_state);
            structured_diff::compute(&repo, &from, &to, three_dot)
        }
        RefTarget::Worktree => {
            let Some(dir) = worktree else {
                return (StatusCode::NOT_FOUND, "run worktree not found").into_response();
            };
            structured_diff::compute_worktree(&dir, &from, run_refs::WORKTREE_ID, three_dot)
        }
    };
    match result {
        Ok(d) => Json(d).into_response(),
        Err(e) if e.is_unknown_revision() => {
            (StatusCode::NOT_FOUND, "run branch not found").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct ListReviewCommentsQuery {
    /// `open` (default for the CLI) | `resolved` | `all` (default here: the UI
    /// wants everything, resolved ones collapse client-side).
    #[serde(default)]
    state: Option<String>,
    /// #752: the displayed pair to **re-map** every comment onto (stable Run ref
    /// ids, defaults `fork` / `tip` when only one side is given). Absent both:
    /// no mapping, the comments come back as written.
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

/// `GET /runs/<id>/review/comments[?state=open|resolved|all]` (#750, #751): the
/// Run's **sent** review comments as projected from the event log — the same
/// list `GET /runs/<id>` carries under `review_comments`, with each comment's
/// thread (`replies`), exposed on its own for `pdo review list` and the tests.
/// Also answers `review_agent_can_resolve` so a CLI can say what `--resolved`
/// will do. With `?from=&to=` (#752) every comment is re-mapped onto that pair:
/// `outdated` / `mapped_line` / `moved` are added, computed on read.
async fn list_review_comments(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(q): Query<ListReviewCommentsQuery>,
) -> Response {
    let filter = match q.state.as_deref() {
        None => review_comments::StateFilter::All,
        Some(raw) => match review_comments::StateFilter::parse(raw) {
            Some(f) => f,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("state must be `open`, `resolved` or `all`, got {raw:?}")
                    })),
                )
                    .into_response();
            }
        },
    };
    let (events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let comments: Vec<&review_comments::ReviewComment> = run_state
        .review_comments
        .iter()
        .filter(|c| filter.keeps(c))
        .collect();
    let can_resolve = review_agent_can_resolve(&state).await;

    // #752 (ADR-0067 §5): re-map each anchor onto the requested pair, on read.
    // A comment whose anchored side is unchanged between the SHA it was written
    // against and the displayed one is *reported* (`mapped_line`, possibly
    // shifted); otherwise it is `outdated`. Nothing is written back.
    let wants_map = q.from.is_some() || q.to.is_some();
    if !wants_map {
        return Json(serde_json::json!({
            "comments": comments,
            "agent_can_resolve": can_resolve,
        }))
        .into_response();
    }
    let worktree = run_worktree_dir(&state.run, &run_state, &run_id);
    let from = match resolve_ref_param(
        &run_id,
        &run_state,
        &events,
        q.from.as_deref(),
        run_refs::FORK_ID,
    ) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let to = match resolve_ref_param(
        &run_id,
        &run_state,
        &events,
        q.to.as_deref(),
        default_to_id(worktree.as_ref()),
    ) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let repo = effective_repo_root(&state, &run_state);
    // #835: the working tree's "SHA" is its snapshot tree id, a real object of
    // the repository — the two-dot re-map below diffs against it like any ref.
    let sha_of_target = |t: &RefTarget| match t {
        RefTarget::Git(r) => structured_diff::rev_parse_object(&repo, r),
        RefTarget::Worktree => worktree
            .as_ref()
            .and_then(|dir| structured_diff::snapshot_worktree(dir).ok()),
    };
    let from_sha = sha_of_target(&from);
    let to_sha = sha_of_target(&to);
    let diff_path = |orig: &str, target: &str, path: &str| {
        if !structured_diff::is_safe_path(path)
            || !structured_diff::is_safe_ref(orig)
            || !structured_diff::is_safe_ref(target)
        {
            return None;
        }
        structured_diff::compute_path(&repo, orig, target, path).ok()
    };
    let mapped: Vec<serde_json::Value> = comments
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).expect("ReviewComment serializes");
            if let Some(m) =
                review_comments::map_comment(c, from_sha.as_deref(), to_sha.as_deref(), &diff_path)
            {
                if let (Some(obj), serde_json::Value::Object(extra)) = (
                    v.as_object_mut(),
                    serde_json::to_value(m).expect("AnchorMapping serializes"),
                ) {
                    obj.extend(extra);
                }
            }
            v
        })
        .collect();
    Json(serde_json::json!({
        "comments": mapped,
        "agent_can_resolve": can_resolve,
        "from_sha": from_sha,
        "to_sha": to_sha,
    }))
    .into_response()
}

/// The instance's `review_agent_can_resolve`, read FRESH at the edge (never
/// cached at boot) through `stored → env → default(false)` — the same chokepoint
/// discipline as `default_auto_name`.
async fn review_agent_can_resolve(state: &AppState) -> bool {
    let stored = instance_config::get(&state.db)
        .await
        .ok()
        .and_then(|c| c.review_agent_can_resolve);
    review_comments::review_agent_can_resolve_with(stored)
}

#[derive(Deserialize)]
struct ReplyReviewCommentRequest {
    text: String,
    /// The agent considers the comment addressed: a **proposal** unless the
    /// instance setting lets agents resolve directly.
    #[serde(default)]
    resolved: bool,
    /// Fallback author when the call carries no session headers (the CLI
    /// always sends them from a node or manager session).
    #[serde(default)]
    author: Option<String>,
}

/// Find a sent comment by id, or answer the 404 the three verbs share.
fn find_review_comment<'a>(
    run_state: &'a event_log::RunState,
    comment_id: &str,
) -> Result<&'a review_comments::ReviewComment, Box<Response>> {
    run_state
        .review_comments
        .iter()
        .find(|c| c.id == comment_id)
        .ok_or_else(|| {
            Box::new(
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": format!("unknown review comment {comment_id:?} on this Run (ids read rc-001, rc-002, …; `pdo review list` shows them)")
                    })),
                )
                    .into_response(),
            )
        })
}

/// Append one review event (`replied` / `resolved` / `reopened`) and hand back the
/// freshly projected comment. Every append broadcasts, so the Review page sees
/// the reply inline without polling.
async fn append_review_event(
    state: &AppState,
    run_id: &str,
    kind: event_log::EventKind,
    payload: serde_json::Value,
) -> Result<(), Box<Response>> {
    let event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind,
        node_id: None,
        iter: None,
        payload: Some(payload),
    };
    append_event(state, &event).await.map_err(|e| {
        error!("run {run_id}: failed to append {:?}: {e}", event.kind);
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("failed to record the event: {e}") })),
            )
                .into_response(),
        )
    })
}

/// The projected comment after the appends, for the response body.
async fn projected_review_comment(
    state: &AppState,
    run_id: &str,
    comment_id: &str,
) -> Result<review_comments::ReviewComment, Box<Response>> {
    let (_, run_state) = load_projected(state, run_id).await?;
    find_review_comment(&run_state, comment_id).cloned()
}

/// `POST /runs/<id>/review/comments/<rc-id>/reply` (#751, ADR-0067 §4; CONTEXT.md
/// « Réponse de review »): an agent — the manager or any node of the Run — answers
/// a sent comment. The author is **deduced from the session** that calls (the
/// `X-PDO-Session-*` headers `pdo review reply` forwards: the manager session
/// reads `manager`, a node session its node id). `resolved: true` is a
/// **proposal** by default (`review_comment_replied` with
/// `proposes_resolution`); under `review_agent_can_resolve` it also appends
/// `review_comment_resolved` by the same author — the human keeps Reopen either
/// way. Replies are welcome on a resolved comment too (the thread stays open to
/// clarification); `resolved` is then just a reply.
async fn reply_review_comment(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, comment_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<ReplyReviewCommentRequest>,
) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let text = req.text.trim_end().to_string();
    if text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "empty reply text" })),
        )
            .into_response();
    }
    let comment = match find_review_comment(&run_state, &comment_id) {
        Ok(c) => c.clone(),
        Err(resp) => return *resp,
    };
    let session_node = session_claim_from_headers(&headers).map(|c| c.node_id);
    let author = review_comments::reply_author(session_node.as_deref(), req.author.as_deref());

    let can_resolve = req.resolved && review_agent_can_resolve(&state).await;
    let outcome =
        if !req.resolved || comment.status == review_comments::ReviewCommentStatus::Resolved {
            review_comments::ReplyOutcome::Replied
        } else if can_resolve {
            review_comments::ReplyOutcome::Resolved
        } else {
            review_comments::ReplyOutcome::Proposed
        };

    if let Err(resp) = append_review_event(
        &state,
        &run_id,
        event_log::EventKind::ReviewCommentReplied,
        serde_json::json!({
            "id": comment_id,
            "author": author,
            "text": text,
            "proposes_resolution": req.resolved && comment.status != review_comments::ReviewCommentStatus::Resolved,
        }),
    )
    .await
    {
        return *resp;
    }
    if outcome == review_comments::ReplyOutcome::Resolved {
        if let Err(resp) = append_review_event(
            &state,
            &run_id,
            event_log::EventKind::ReviewCommentResolved,
            serde_json::json!({ "id": comment_id, "by": author }),
        )
        .await
        {
            return *resp;
        }
    }
    info!(
        "run {run_id}: {author} replied to review comment {comment_id} ({})",
        match outcome {
            review_comments::ReplyOutcome::Replied => "reply",
            review_comments::ReplyOutcome::Proposed => "resolution proposed",
            review_comments::ReplyOutcome::Resolved => "resolved directly, setting on",
        }
    );
    match projected_review_comment(&state, &run_id, &comment_id).await {
        Ok(c) => Json(serde_json::json!({
            "comment": c,
            "author": author,
            "outcome": outcome,
        }))
        .into_response(),
        Err(resp) => *resp,
    }
}

#[derive(Deserialize, Default)]
struct ReviewDecisionRequest {
    /// Who decides — `user` (the Review page, default) or an agent id for a
    /// scripted resolution.
    #[serde(default)]
    by: Option<String>,
}

/// `POST /runs/<id>/review/comments/<rc-id>/resolve` (#751): the human resolves
/// (accepts a proposal, or closes a comment outright). Already resolved ⇒ `200`
/// with `changed: false` and no event — two browsers clicking is not an error.
async fn resolve_review_comment(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, comment_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Option<Json<ReviewDecisionRequest>>,
) -> Response {
    review_decision(
        &state,
        &run_id,
        &comment_id,
        &headers,
        body.map(|Json(b)| b).unwrap_or_default(),
        review_comments::ReviewCommentStatus::Resolved,
    )
    .await
}

/// `POST /runs/<id>/review/comments/<rc-id>/reopen` (#751): back to `sent`. On a
/// resolved comment it reopens it (whoever resolved it — the human keeps the
/// last word, ADR-0067 §4); on a `sent` comment carrying a pending proposal it
/// **declines** the proposal and keeps the comment open for the agent — the
/// same event, `review_comment_reopened`. A `sent` comment with nothing pending
/// ⇒ `200`, `changed: false`.
async fn reopen_review_comment(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, comment_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Option<Json<ReviewDecisionRequest>>,
) -> Response {
    review_decision(
        &state,
        &run_id,
        &comment_id,
        &headers,
        body.map(|Json(b)| b).unwrap_or_default(),
        review_comments::ReviewCommentStatus::Sent,
    )
    .await
}

async fn review_decision(
    state: &AppState,
    run_id: &str,
    comment_id: &str,
    headers: &HeaderMap,
    req: ReviewDecisionRequest,
    target: review_comments::ReviewCommentStatus,
) -> Response {
    let (_, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let comment = match find_review_comment(&run_state, comment_id) {
        Ok(c) => c.clone(),
        Err(resp) => return *resp,
    };
    // A session (agent) decides as itself; the Review page decides as `user`.
    let by = match session_claim_from_headers(headers).map(|c| c.node_id) {
        Some(node) => review_comments::reply_author(Some(&node), None),
        None => req
            .by
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "user".to_string()),
    };
    let (kind, changes) = match target {
        review_comments::ReviewCommentStatus::Resolved => (
            event_log::EventKind::ReviewCommentResolved,
            comment.status != review_comments::ReviewCommentStatus::Resolved,
        ),
        review_comments::ReviewCommentStatus::Sent => (
            event_log::EventKind::ReviewCommentReopened,
            comment.status == review_comments::ReviewCommentStatus::Resolved
                || comment.proposal_pending,
        ),
    };
    if !changes {
        return Json(serde_json::json!({ "comment": comment, "changed": false })).into_response();
    }
    let kind_str = format!("{kind:?}");
    if let Err(resp) = append_review_event(
        state,
        run_id,
        kind,
        serde_json::json!({ "id": comment_id, "by": by }),
    )
    .await
    {
        return *resp;
    }
    info!("run {run_id}: review comment {comment_id} {kind_str} by {by}");
    match projected_review_comment(state, run_id, comment_id).await {
        Ok(c) => Json(serde_json::json!({ "comment": c, "changed": true })).into_response(),
        Err(resp) => *resp,
    }
}

/// One draft the browser hands over for sending (#750). `from`/`to` are stable
/// Run ref ids (default `fork` → `tip`), never SHAs.
#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
struct SendReviewCommentInput {
    path: String,
    side: String,
    line: i64,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct SendReviewCommentsRequest {
    comments: Vec<SendReviewCommentInput>,
}

/// Context lines kept on each side of the anchored line in the excerpt.
const REVIEW_EXCERPT_CONTEXT: usize = 2;
/// How long a manager spawned by the send waits before the batch is pasted —
/// the harness has to be reading its input, or the paste lands in a shell.
const REVIEW_PASTE_DELAY_AFTER_SPAWN: Duration = Duration::from_secs(8);

/// `POST /runs/<id>/review/comments/send` (#750, ADR-0067 §2–3; CONTEXT.md
/// « Envoi au manager »): turn a batch of browser drafts into `sent` comments.
///
/// Order of operations, each step a reason the drafts **stay drafts**:
/// 1. the Run branch must still exist — an archived Run, or a `pdo/run-<id>`
///    branch that is gone, answers `409 run_branch_gone` with the reason the UI
///    shows on its disabled buttons (nothing for the manager to act on);
/// 2. every anchor is validated (safe path, `old|new`, line ≥ 1, non-empty
///    text, known ref ids) — `400` names the offending index;
/// 3. the Pipeline Manager is started **on demand** if it is not running
///    (`ensure_run_manager`; its refusals pass through);
/// 4. one `review_comment_sent` event per comment lands in the log (ids
///    `rc-NNN` continue the Run's sequence; the batch shares a `batch_id`);
/// 5. **one** message for the whole batch is pasted into the manager's tmux
///    session, detached — after a short grace when this call spawned it.
///
/// The response carries the sent comments so the browser can drop its drafts
/// without waiting for the WebSocket round-trip (which also arrives: every
/// append broadcasts).
async fn send_review_comments(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<SendReviewCommentsRequest>,
) -> Response {
    let (events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    if req.comments.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no comments to send" })),
        )
            .into_response();
    }

    // 1. Branch gone ⇒ sending is disabled, with the reason.
    let tip = run_refs::tip_branch(&run_id);
    let repo = effective_repo_root(&state, &run_state);
    let branch_gone = run_state.status == event_log::RunStatus::Archived
        || structured_diff::rev_parse(&repo, &tip).is_none();
    if branch_gone {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "run_branch_gone",
                "recoverable": false,
                "message": format!(
                    "Run branch {tip} no longer exists — nothing for the manager to act on. Comments stay readable; sending is disabled."
                ),
            })),
        )
            .into_response();
    }

    // 2. Validate + resolve every anchor before touching anything.
    let sha_of = |r: &str| structured_diff::rev_parse_object(&repo, r);
    // #835: an anchor on the working tree freezes its snapshot tree id — the
    // stable object the re-map (#752) diffs from once the tree moves on.
    let worktree = run_worktree_dir(&state.run, &run_state, &run_id);
    let snapshot = worktree
        .as_ref()
        .map(|dir| structured_diff::snapshot_worktree(dir).ok());
    let refs = run_refs::collect(
        &run_id,
        &run_state,
        &events,
        &sha_of,
        snapshot.as_ref().map(Option::as_deref),
    );
    let label_of = |id: &str| -> String {
        refs.refs
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.label.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let bad = |i: usize, why: &str| -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("comment #{}: {why}", i + 1) })),
        )
            .into_response()
    };
    let now = pdo_kernel::clock::now_iso();
    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut prepared: Vec<review_comments::ReviewComment> = Vec::with_capacity(req.comments.len());
    for (i, c) in req.comments.iter().enumerate() {
        if !structured_diff::is_safe_path(&c.path) {
            return bad(i, &format!("invalid path {:?}", c.path));
        }
        let Some(side) = review_comments::ReviewSide::parse(&c.side) else {
            return bad(i, &format!("side must be `old` or `new`, got {:?}", c.side));
        };
        if c.line < 1 {
            return bad(i, "line must be ≥ 1");
        }
        if c.text.trim().is_empty() {
            return bad(i, "empty text");
        }
        let from_id = c.from.as_deref().unwrap_or(run_refs::FORK_ID);
        let to_id = c.to.as_deref().unwrap_or(default_to_id(worktree.as_ref()));
        let resolve = |id: &str| -> Option<String> {
            match run_refs::resolve_id(&run_id, &run_state, &events, id) {
                run_refs::Resolved::Git(r) => Some(r),
                run_refs::Resolved::Worktree => snapshot.clone().flatten(),
                _ => None,
            }
        };
        let Some(from_git) = resolve(from_id) else {
            return bad(i, &format!("unknown run ref {from_id:?}"));
        };
        let Some(to_git) = resolve(to_id) else {
            return bad(i, &format!("unknown run ref {to_id:?}"));
        };
        // The excerpt reads the anchored side's file at its ref. Best-effort: a
        // path that no longer resolves sends with an empty excerpt.
        let anchored_ref = match side {
            review_comments::ReviewSide::Old => &from_git,
            review_comments::ReviewSide::New => &to_git,
        };
        let excerpt = structured_diff::file_at_ref(&repo, anchored_ref, &c.path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .map(|content| review_comments::excerpt(&content, c.line, REVIEW_EXCERPT_CONTEXT))
            .unwrap_or_default();
        prepared.push(review_comments::ReviewComment {
            id: String::new(), // assigned below, once the manager is up
            path: c.path.clone(),
            side,
            line: c.line,
            from_ref: from_id.to_string(),
            to_ref: to_id.to_string(),
            from_sha: sha_of(&from_git),
            to_sha: sha_of(&to_git),
            text: c.text.trim_end().to_string(),
            excerpt,
            author: "user".to_string(),
            sent_at: now.clone(),
            batch_id: Some(batch_id.clone()),
            status: review_comments::ReviewCommentStatus::Sent,
            replies: Vec::new(),
            proposal_pending: false,
            resolved_by: None,
            resolved_at: None,
            reopened_by: None,
            reopened_at: None,
            proposal_declined: false,
        });
    }

    // 3. Manager on demand — before any event, so a refusal leaves drafts intact.
    let created = match ensure_run_manager(&state.run, &run_id, &run_state).await {
        Ok(created) => created,
        Err((status, body)) => return (status, Json(body)).into_response(),
    };

    // 4. Durable first: one event per comment, ids continuing the Run's sequence.
    let base = run_state.review_comments.len();
    for (i, c) in prepared.iter_mut().enumerate() {
        c.id = review_comments::comment_id(base + i + 1);
        let event = event_log::Event {
            id: None,
            run_id: run_id.clone(),
            ts: now.clone(),
            kind: event_log::EventKind::ReviewCommentSent,
            node_id: None,
            iter: None,
            payload: Some(serde_json::to_value(&*c).expect("ReviewComment serializes")),
        };
        if let Err(e) = append_event(&state, &event).await {
            error!(
                "run {run_id}: failed to append review_comment_sent {}: {e}",
                c.id
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to record comment {} ({} of the batch recorded): {e}", c.id, i)
                })),
            )
                .into_response();
        }
    }

    // 5. One message for the batch, handed over off the request.
    let (from_label, to_label) = {
        let first = &prepared[0];
        (label_of(&first.from_ref), label_of(&first.to_ref))
    };
    let message = review_comments::batch_message(&run_id, &from_label, &to_label, &prepared);
    let tmux = state.session.tmux().clone();
    let session = pdo_session::domain::names::manager_session_name(&run_id);
    let delay = if created {
        REVIEW_PASTE_DELAY_AFTER_SPAWN
    } else {
        Duration::from_millis(300)
    };
    tokio::spawn(async move {
        time::sleep(delay).await;
        pdo_session::application::messages::paste_message(tmux.as_ref(), &session, &message);
    });
    info!(
        "run {run_id}: {} review comment(s) sent to the manager as one message (batch {batch_id}{})",
        prepared.len(),
        if created { ", manager started" } else { "" }
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sent": prepared,
            "batch_id": batch_id,
            "manager_started": created,
        })),
    )
        .into_response()
}

pub use pdo_kernel::env::TMUX_CMD_OVERRIDE_ENV;

// --- Static file serving ---

async fn static_handler(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return serve_index();
    }

    match FrontendAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => serve_index(),
    }
}

fn serve_index() -> Response {
    match FrontendAssets::get("index.html") {
        Some(content) => Html(content.data.into_owned()).into_response(),
        None => {
            if cfg!(debug_assertions) {
                Html(DEV_PLACEHOLDER).into_response()
            } else {
                (StatusCode::NOT_FOUND, "frontend assets not found").into_response()
            }
        }
    }
}

const DEV_PLACEHOLDER: &str = r#"<!DOCTYPE html>
<html>
<head><title>PDO (dev)</title></head>
<body style="background:#0f1115;color:#e6e8eb;font-family:sans-serif;display:grid;place-items:center;height:100vh;margin:0">
<div style="text-align:center">
<h1>PDO daemon running</h1>
<p>In dev mode, run the Vite frontend separately:<br><code>cd frontend && pnpm run dev</code></p>
</div>
</body>
</html>"#;

// ─────────────────────────────────────────────────────────────────────────────
// The workspace context (crates/pdo-workspace), wired into this daemon
// ─────────────────────────────────────────────────────────────────────────────

/// The settings a Projet carries for the contexts still in this crate: the
/// agentic profile (ADR-0057) and the skills selection (ADR-0062).
/// The agentic profile a Projet carries, as this crate types it.
fn project_agent_choice(
    project: &pdo_workspace::domain::project::Project,
) -> Option<pdo_harness::agent_choice::AgentChoice> {
    use pdo_workspace::adapters::http::ProjectSettings;
    project
        .agent_choice
        .as_ref()
        .and_then(|stored| LegacyProjectSettings::decode_agent_choice(stored.as_str()))
}

/// The skills a Projet selects, as this crate types them.
fn project_skills(
    project: &pdo_workspace::domain::project::Project,
) -> Vec<skill_selection::SkillRef> {
    use pdo_workspace::adapters::http::ProjectSettings;
    LegacyProjectSettings::decode_skills(project.skills.as_ref().map(|s| s.as_str()))
}

pub(crate) struct LegacyProjectSettings;

impl pdo_workspace::adapters::http::ProjectSettings for LegacyProjectSettings {
    type AgentChoice = pdo_harness::agent_choice::AgentChoice;
    type Skill = skill_selection::SkillRef;

    fn encode_skills(skills: Vec<Self::Skill>) -> Option<String> {
        skill_selection::to_stored_json(&skill_selection::normalise(skills))
    }

    fn decode_skills(stored: Option<&str>) -> Vec<Self::Skill> {
        skill_selection::from_stored_json(stored.map(str::to_string))
    }
}

/// A Projet change, announced on the live pipeline stream the UI listens to.
struct PipelineChannelEvents(broadcast::Sender<serde_json::Value>);

impl pdo_workspace::application::ports::ProjectEvents for PipelineChannelEvents {
    fn project_changed(&self, project_id: &str) {
        let _ = self.0.send(serde_json::json!({
            "type": "project_changed",
            "project_id": project_id,
        }));
    }
}

/// The workspace context over this daemon's database, live stream and
/// repository root, with `host` as the Autorisation d'opération.
fn workspace_context(
    db: &sqlx::SqlitePool,
    bus_tx: &broadcast::Sender<serde_json::Value>,
    repo_root: &Path,
    host: Arc<dyn pdo_kernel::extension_host::ExtensionHost>,
) -> Arc<pdo_workspace::application::Workspace> {
    use pdo_workspace::adapters::{fs, git, sqlite};
    Arc::new(pdo_workspace::application::Workspace::new(
        pdo_workspace::application::WorkspacePorts {
            projects: Arc::new(sqlite::SqliteProjects::new(db.clone())),
            provisioning: Arc::new(sqlite::SqliteProvisioningRules::new(db.clone())),
            recent: Arc::new(sqlite::SqliteRecentRepositories::new(db.clone())),
            events: Arc::new(PipelineChannelEvents(bus_tx.clone())),
            git: Arc::new(git::GitCli),
            source_tree: Arc::new(fs::FsSourceTree),
            files: Arc::new(fs::FsBrowser::new(repo_root)),
        },
        host,
    ))
}

/// The skill context over this daemon's database and repository root (where
/// the bank keeps its content), with `host` as the Autorisation d'opération.
fn skill_context(
    db: &sqlx::SqlitePool,
    repo_root: &Path,
    host: Arc<dyn pdo_kernel::extension_host::ExtensionHost>,
) -> Arc<pdo_skill::Skills> {
    use pdo_skill::adapters::{delivery, fs, source, sqlite};
    Arc::new(pdo_skill::Skills::new(
        pdo_skill::SkillsPorts {
            skills: Arc::new(sqlite::SqliteSkillIndex::new(db.clone())),
            folders: Arc::new(sqlite::SqliteFolderIndex::new(db.clone())),
            sources: Arc::new(sqlite::SqliteSourceHistory::new(db.clone())),
            content: Arc::new(fs::FsSkillContent::new(repo_root)),
            scanner: Arc::new(source::GitSourceScanner::new()),
            delivery: Arc::new(delivery::WorktreeDelivery::new(repo_root)),
            referents: Arc::new(LegacySkillReferents {
                db: db.clone(),
                repo_root: repo_root.to_path_buf(),
            }),
        },
        host,
    ))
}

/// Who selects a skill, read from the tiers that still live in this crate: the
/// instance configuration, the Projets, the Triggers, the instance pipelines'
/// nodes, and the Runs not started yet (#669).
struct LegacySkillReferents {
    db: sqlx::SqlitePool,
    repo_root: PathBuf,
}

impl pdo_skill::application::ports::SkillReferents for LegacySkillReferents {
    fn referents<'a>(
        &'a self,
        id: &'a str,
    ) -> pdo_skill::application::ports::BoxFuture<'a, pdo_skill::application::ports::Referents>
    {
        Box::pin(skill_referents(&self.db, &self.repo_root, id))
    }
}

async fn skill_referents(
    db: &sqlx::SqlitePool,
    repo_root: &Path,
    id: &str,
) -> pdo_skill::application::ports::Referents {
    use pdo_skill::application::ports::{
        NamedRef, PipelineReferent, Referents, RunReferent, TriggerReferent,
    };
    let instance = instance_config::get(db)
        .await
        .map(|config| skill_selection::selects(&config.skills, id))
        .unwrap_or(false);

    let projects = pdo_workspace::adapters::sqlite::list(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|project| skill_selection::selects(&project_skills(project), id))
        .map(|project| NamedRef {
            id: project.id,
            name: project.name,
        })
        .collect();

    let triggers = pdo_trigger::adapters::sqlite::list(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|trigger| skill_selection::selects(&trigger.skills, id))
        .map(|trigger| TriggerReferent {
            id: trigger.id,
            name: trigger.name,
            pipeline_id: trigger.pipeline_id,
        })
        .collect();

    let pipeline_entries = instance_pipeline_dir(repo_root)
        .map(|directory| pdo_pipeline::adapters::registry::pipeline_files(&directory))
        .unwrap_or_default();
    let pipelines = pipeline_entries
        .into_iter()
        .flat_map(|entry| {
            let Ok(yaml) = std::fs::read_to_string(&entry.path) else {
                return Vec::new();
            };
            let Ok(parsed) = pipeline::parse_pipeline(&yaml) else {
                return Vec::new();
            };
            let name = parsed.pipeline.name.clone();
            parsed
                .pipeline
                .nodes
                .into_iter()
                .filter(|node| skill_selection::selects(&node.skills, id))
                .map(|node| PipelineReferent {
                    id: entry.id.clone(),
                    name: name.clone(),
                    node_id: node.id,
                    scope: "instance".to_string(),
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // One indexed pass over `run_started` payloads: non-terminal Runs that have not
    // spawned a single node yet (same terminality test as the sandbox-profile
    // referents, plus the "no node_started" clause that makes them *not started*).
    let rows: Result<Vec<(String, String)>, _> = sqlx::query_as(
        "SELECT e.run_id, e.payload FROM events e \
         WHERE e.kind = 'run_started' AND e.payload IS NOT NULL \
           AND NOT EXISTS ( \
             SELECT 1 FROM events t WHERE t.run_id = e.run_id AND t.kind IN \
               ('run_completed', 'run_failed', 'run_skipped', 'run_halted', 'run_archived', \
                'node_started') \
           ) \
         ORDER BY e.ts DESC",
    )
    .fetch_all(db)
    .await;
    let runs = match rows {
        Ok(rows) => rows
            .iter()
            .filter_map(|(run_id, payload)| {
                let val = serde_json::from_str::<serde_json::Value>(payload).ok()?;
                let skills: Vec<skill_selection::SkillRef> =
                    serde_json::from_value(val.get("skills")?.clone()).ok()?;
                if !skill_selection::selects(&skills, id) {
                    return None;
                }
                let text = |key: &str| val.get(key).and_then(|v| v.as_str()).map(str::to_string);
                Some(RunReferent {
                    run_id: run_id.clone(),
                    pipeline_name: text("pipeline_name"),
                    name: text("name"),
                })
            })
            .collect(),
        Err(e) => {
            error!("failed to query runs for skill referents: {e}");
            Vec::new()
        }
    };

    Referents {
        instance,
        projects,
        triggers,
        pipelines,
        runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_run::adapters::disk_live_facts::compute_run_loc;
    use pdo_run::application::complete_node::{
        complete_node_iteration, CompletionAttempt, CompletionSource,
    };
    use pdo_run::application::release_completion::release_node_completion;
    use pdo_run::application::reopen::embed_reopen_for_targeted_command;
    use pdo_run::application::scheduler::advance as run_advance;
    use pdo_run::domain::completion_refusal;

    use axum::body::Body;
    use axum::http::Request;
    use pdo_platform::domain::service_health::ServiceHealth;
    use tower::util::ServiceExt;

    /// #541 — the startup banner must hand the user a URL the WS Origin guard
    /// accepts. `0.0.0.0` (and `::`) are listen-only and map to `localhost`;
    /// a concrete bind is advertised verbatim.
    #[test]
    fn advertised_url_maps_unspecified_bind_to_localhost() {
        let v4: SocketAddr = "0.0.0.0:5172".parse().unwrap();
        assert_eq!(advertised_url(v4), "http://localhost:5172");
        let v6: SocketAddr = "[::]:5172".parse().unwrap();
        assert_eq!(advertised_url(v6), "http://localhost:5172");
    }

    #[test]
    fn advertised_url_keeps_concrete_bind() {
        let lo: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(advertised_url(lo), "http://127.0.0.1:0");
        let lan: SocketAddr = "192.168.1.20:5172".parse().unwrap();
        assert_eq!(advertised_url(lan), "http://192.168.1.20:5172");
    }

    #[test]
    fn daemon_relaunch_preserves_only_an_explicit_bind() {
        let exe = Path::new("/usr/local/bin/pdo");
        assert_eq!(
            daemon_relaunch_command(exe, None, 5172),
            vec!["/usr/local/bin/pdo", "daemon", "--port", "5172"]
        );
        assert_eq!(
            daemon_relaunch_command(exe, Some("127.0.0.1".parse().unwrap()), 5172),
            vec![
                "/usr/local/bin/pdo",
                "daemon",
                "--port",
                "5172",
                "--bind",
                "127.0.0.1"
            ]
        );
    }

    /// A unique fake daemon port per test state, so the derived tmux socket
    /// (`tmux_socket_name(port)` = `pdo-<port>`) is distinct per test. Tests spawn
    /// stub sessions that exit immediately (`exec true`); on a *shared* socket
    /// (every state used `port: 0` → `pdo-0`) one test's session-exit tears the
    /// tmux server down just as a concurrent test's `new-session` runs, failing
    /// that spawn — an intermittent 400 in CI under load (the flaky
    /// `start_node_command_force_spawns_pending_node`). Isolating the socket per
    /// test removes the cross-test race. This is a socket-name seed only, never a
    /// bound port: oneshot tests never start an HTTP listener.
    ///
    /// The counter is per PROCESS, and `make test` runs nextest — one process per
    /// test — so a counter starting at a constant gave every test the same first
    /// port, hence the same socket, and brought the race back (the flaky
    /// `adr25_case_d_spawned_lists_what_actually_launched`: « tmux new-session
    /// failed: server exited unexpectedly »). The start is therefore spread by
    /// process id: 4 000 slots of 10 sockets across 20 000..60 000. `cargo test`
    /// (one process) still counts up from a single start, so it never collides.
    fn next_test_daemon_port() -> u16 {
        use std::sync::atomic::{AtomicU16, Ordering};
        static NEXT: std::sync::OnceLock<AtomicU16> = std::sync::OnceLock::new();
        NEXT.get_or_init(|| AtomicU16::new(20000 + (std::process::id() % 4000) as u16 * 10))
            .fetch_add(1, Ordering::Relaxed)
    }

    // The nested-daemon (no-cleanup) posture resolver
    //
    // The truth table used to be inlined in `serve_with_config` as two
    // `std::env::var` reads, which made it untestable except by mutating
    // process-global env — the very thing that raced the `pty_bridge` tests into
    // a red CI. Pure fn, so each case is a plain assertion.

    #[test]
    fn nested_daemon_is_off_when_neither_signal_is_present() {
        assert!(!nested_daemon_from(None, None));
    }

    #[test]
    fn a_sub_claude_context_is_nested_on_the_mere_presence_of_a_node_id() {
        // `wrap_with_env` exports PDO_NODE_ID; presence is the signal, so even an
        // empty value means "inside a NodeRun" (the pre-existing `.is_ok()` shape).
        assert!(nested_daemon_from(Some("implementer"), None));
        assert!(nested_daemon_from(Some(""), None));
    }

    #[test]
    fn the_operator_knob_is_truthy_but_not_merely_set() {
        assert!(nested_daemon_from(None, Some("1")));
        assert!(nested_daemon_from(None, Some("yes")));
        // Same truthiness as the `PDO_DEBUG_PANIC_*` knobs: "" and "0" are OFF, so
        // `PDO_DAEMON_NO_CLEANUP=0` disarms rather than arms.
        assert!(!nested_daemon_from(None, Some("")));
        assert!(!nested_daemon_from(None, Some("0")));
    }

    #[test]
    fn either_signal_alone_is_enough() {
        assert!(nested_daemon_from(Some("worker"), Some("0")));
        assert!(nested_daemon_from(None, Some("1")));
    }

    /// The platform context of a test state: its real SQLite and filesystem
    /// adapters, no Run to verify a session against, no home to journal an
    /// update in.
    async fn test_platform(
        db: &sqlx::SqlitePool,
        health: Option<pdo_platform::domain::service_health::ServiceHealth>,
    ) -> Arc<pdo_platform::Platform> {
        use pdo_platform::adapters as a;
        use pdo_platform::application::ports::{BoxFuture, Runs};

        struct NoRuns;
        impl Runs for NoRuns {
            fn verify<'a>(
                &'a self,
                run_id: &'a str,
                _: &'a str,
            ) -> BoxFuture<'a, Result<(), String>> {
                Box::pin(async move { Err(format!("no run `{run_id}` in a test state")) })
            }
            fn record_command<'a>(
                &'a self,
                _: &'a str,
                _: &'a str,
                _: &'a str,
                _: serde_json::Value,
            ) -> BoxFuture<'a, Result<(), String>> {
                Box::pin(async { Ok(()) })
            }
            fn count_live(&self) -> BoxFuture<'_, usize> {
                Box::pin(async { 0 })
            }
        }

        let platform = pdo_platform::Platform::new(
            pdo_platform::PlatformPorts {
                page_mounts: Arc::new(a::sqlite_page_mounts::SqlitePageMounts::new(db.clone())),
                page_files: Arc::new(a::page_files::LocalPageFiles),
                audit: Arc::new(a::sqlite_audit_log::SqliteAuditLog::new(db.clone())),
                runs: Arc::new(NoRuns),
                update_setting: Arc::new(contexts::LegacyUpdateSetting { db: db.clone() }),
                release_source: Arc::new(a::github_releases::GithubReleases),
                update_journal: Arc::new(a::fs_update_journal::FsUpdateJournal::new(None)),
                installation: Arc::new(a::host_installation::HostInstallation::default()),
                update_executor: Arc::new(a::detached_executor::DetachedExecutor::new(None)),
                extensions: Arc::new(pdo_kernel::extension_host::NoExtensions),
            },
            pdo_platform::application::update_context::UpdateOptions::default(),
            health.unwrap_or(pdo_platform::domain::service_health::ServiceHealth {
                supervisor: "none".to_string(),
                persistent: None,
            }),
        );
        platform.init().await.unwrap();
        platform
    }

    async fn test_state() -> Arc<AppState> {
        test_state_with_tmux(None).await
    }

    /// A tmux for the unit tests of the run side: every session "exists", nothing
    /// runs. The declared wait asks tmux whether the node's session is up, and the
    /// runs these tests seed have no real session behind them.
    struct AliveTmux;

    impl pdo_session::application::ports::Tmux for AliveTmux {
        fn socket(&self) -> &str {
            "pdo-test-alive"
        }
        fn session_path(&self) -> String {
            String::new()
        }
        fn new_session(
            &self,
            _name: &str,
            _working_dir: &Path,
            _script: &str,
            _what: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill(&self, _name: &str) {}
        fn session_exists(&self, _name: &str) -> bool {
            true
        }
        fn server_alive(&self) -> Option<bool> {
            Some(true)
        }
        fn list_pdo_sessions(&self) -> std::collections::HashSet<String> {
            Default::default()
        }
        fn is_attached(&self, _name: &str) -> bool {
            false
        }
        fn capture(&self, _name: &str) -> Option<String> {
            None
        }
        fn send_keys(&self, _name: &str, _text: &str) {}
        fn paste(&self, _name: &str, _text: &str) {}
        fn attach_argv(&self, _name: &str, _ignore_size: bool) -> Vec<String> {
            Vec::new()
        }
        fn attach_command(&self, _name: &str) -> String {
            String::new()
        }
        fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
            false
        }
    }

    /// The same unit-test state with a sandbox home override. The session
    /// context's ports hold a `Weak` on the state, so it is rebuilt around them
    /// rather than mutated through `Arc::get_mut`.
    fn with_home_override(state: Arc<AppState>, home: PathBuf) -> Arc<AppState> {
        let mut inner = Arc::try_unwrap(state).ok().expect("an unshared test state");
        inner.sandbox_home_override = Some(home.clone());
        Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
            inner.session = contexts::test_session(
                weak.clone(),
                inner.repo_root.clone(),
                next_test_daemon_port(),
                None,
            );
            inner.run = contexts::test_run(contexts::TestRun {
                state: weak.clone(),
                db: &inner.db,
                bus_tx: &inner.bus_tx,
                session: &inner.session,
                workspace: inner.workspace.clone(),
                skills: inner.skills.clone(),
                repo_root: inner.repo_root.clone(),
                port: next_test_daemon_port(),
                docker_cmd_override: inner.docker_cmd_override.clone(),
                sandbox_home_override: Some(home),
            });
            inner
        })
    }

    async fn test_state_with_alive_sessions() -> Arc<AppState> {
        test_state_with_tmux(Some(Arc::new(AliveTmux))).await
    }

    async fn test_state_with_tmux(
        tmux: Option<Arc<dyn pdo_session::application::ports::Tmux>>,
    ) -> Arc<AppState> {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (bus_tx, _) = broadcast::channel(80);
        let workspace = workspace_context(
            &db,
            &bus_tx,
            &std::env::current_dir().unwrap(),
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        );
        let skills = skill_context(
            &db,
            &std::env::current_dir().unwrap(),
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        );
        let platform = test_platform(&db, None).await;
        let test_repo_root: PathBuf = std::env::current_dir().unwrap();
        let test_port: u16 = next_test_daemon_port();
        Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
            let session =
                contexts::test_session(weak.clone(), test_repo_root.clone(), test_port, tmux);
            let run = contexts::test_run(contexts::TestRun {
                state: weak.clone(),
                db: &db,
                bus_tx: &bus_tx,
                session: &session,
                workspace: workspace.clone(),
                skills: skills.clone(),
                repo_root: test_repo_root.clone(),
                port: test_port,
                docker_cmd_override: None,
                sandbox_home_override: None,
            });
            AppState {
                workspace,
                skills,
                platform,
                db,
                run,
                bus_tx,
                repo_root: test_repo_root.clone(),
                recent_writes: SelfWrites::new(),
                docker_cmd_override: None,
                sandbox_home_override: None,
                docker_probe_cache: Arc::new(tokio::sync::Mutex::new(None)),
                harness_catalogue: Arc::new(
                    pdo_harness::adapters::binary_probe::installed_catalogue_reader(),
                ),
                price_sync_lock: tokio::sync::Mutex::new(()),
                price_source_url: None,
                price_refresh_at_boot: false,
                allowed_ws_origins: Vec::new(),
                session,
            }
        })
    }

    #[tokio::test]
    async fn stats_overview_sessions_use_run_cohorts_and_dynamic_harness_hierarchies() {
        let state = test_state().await;
        let insert = |run: &str,
                      ts: &str,
                      kind: &str,
                      node: Option<&str>,
                      iter: Option<i64>,
                      payload: Option<serde_json::Value>| {
            let db = state.db.clone();
            let run = run.to_string();
            let ts = ts.to_string();
            let kind = kind.to_string();
            let node = node.map(str::to_string);
            async move {
                sqlx::query(
                    "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(run)
                .bind(ts)
                .bind(kind)
                .bind(node)
                .bind(iter)
                .bind(payload.map(|value| value.to_string()))
                .execute(&db)
                .await
                .unwrap();
            }
        };

        insert(
            "before",
            "2026-07-14T09:00:00Z",
            "run_started",
            None,
            None,
            Some(serde_json::json!({
                "pipeline_id": "p-out",
                "pipeline_name": "Outside",
                "node_defs": [{"id":"worker","name":"Outside worker","node_type":"agent", "isolated_worktree":false}]
            })),
        )
        .await;
        insert(
            "before",
            "2026-07-15T09:00:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            Some(serde_json::json!({"node_type":"agent", "isolated_worktree":false,"harness":"claude"})),
        )
        .await;

        insert(
            "r1",
            "2026-07-15T09:00:00Z",
            "run_started",
            None,
            None,
            Some(serde_json::json!({
                "pipeline_id": "p1",
                "pipeline_name": "Pipeline old",
                "target_repo": "/repos/product",
                "node_defs": [
                    {"id":"worker","name":"Worker old","node_type":"agent", "isolated_worktree":false},
                    {"id":"build","name":"Build","node_type":"script"}
                ]
            })),
        )
        .await;
        for (harness, node_type, session_id) in [
            (None, "agent", None),
            (Some("copilot"), "agent", Some("copilot-resurrected")),
            (Some("copilot"), "agent", Some("copilot-resurrected")),
            (Some("copilot"), "agent", Some("copilot-restart")),
            (None, "script", None),
        ] {
            insert(
                "r1",
                "2026-07-20T09:00:00Z",
                "node_started",
                Some(if node_type == "script" {
                    "build"
                } else {
                    "worker"
                }),
                Some(1),
                Some(serde_json::json!({
                    "node_type":node_type,
                    "harness":harness,
                    "session_id":session_id
                })),
            )
            .await;
        }

        insert(
            "r2",
            "2026-07-16T09:00:00Z",
            "run_started",
            None,
            None,
            Some(serde_json::json!({
                "pipeline_id": "p1",
                "pipeline_name": "Pipeline latest",
                "target_repo": "/repos/product",
                "node_defs": [{"id":"worker","name":"Worker latest","node_type":"agent", "isolated_worktree":false}]
            })),
        )
        .await;
        for (iter, session_id) in [(1, "future-loop-1"), (2, "future-loop-2")] {
            insert(
                "r2",
                "2026-07-16T10:00:00Z",
                "node_started",
                Some("worker"),
                Some(iter),
                Some(serde_json::json!({
                    "node_type":"agent", "isolated_worktree":false,
                    "harness":"future-harness",
                    "session_id":session_id
                })),
            )
            .await;
        }

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri(
                        "/stats/overview?from=2026-07-15T00:00:00Z&to=2026-07-17T00:00:00Z&bucket=day",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        assert_eq!(
            body["session_harnesses"],
            serde_json::json!(["claude", "copilot", "future-harness"])
        );
        assert_eq!(
            body["sessions_by_period"],
            serde_json::json!([
                {
                    "bucket":"2026-07-15",
                    "harnesses":[
                        {"harness":"claude","executions":1},
                        {"harness":"copilot","executions":2}
                    ]
                },
                {
                    "bucket":"2026-07-16",
                    "harnesses":[
                        {"harness":"future-harness","executions":2}
                    ]
                }
            ])
        );
        let pipeline = &body["sessions_by_pipeline"][0];
        assert_eq!(pipeline["id"], "p1");
        assert_eq!(pipeline["name"], "Pipeline latest");
        let worker = pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == "worker")
            .unwrap();
        assert_eq!(worker["name"], "Worker latest");
        assert_eq!(worker["executions"], 5);
        assert_eq!(
            worker["harnesses"],
            serde_json::json!([
                {"harness":"claude","executions":1},
                {"harness":"copilot","executions":2},
                {"harness":"future-harness","executions":2}
            ])
        );
        assert!(
            pipeline["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["id"] != "build"),
            "script starts are excluded"
        );
        assert_eq!(pipeline["executions"], 5);
    }

    #[tokio::test]
    async fn stats_cost_returns_harness_coverage_and_nested_contribution_hierarchies() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-cost")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();
        sqlx::query(
            "INSERT INTO projects (id, name, harness, created_at) VALUES \
             ('prj-product', 'Product', NULL, '2026-07-01T00:00:00Z')",
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_members (path, project_id, created_at) VALUES \
             (?, 'prj-product', '2026-07-01T00:00:00Z')",
        )
        .bind(&repo)
        .execute(&state.db)
        .await
        .unwrap();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        let run_ids = ["stats-cost-r1", "stats-cost-r2", "stats-cost-r3"];
        for (index, run_id) in run_ids.iter().enumerate() {
            insert_event(
                &state.db,
                run_id,
                &format!("2026-07-{}T09:00:00Z", 15 + index),
                "run_started",
                None,
                None,
                serde_json::json!({
                    "pipeline_id":"p1",
                    "pipeline_name":format!("Pipeline {}", index + 1),
                    "target_repo":repo,
                    "harness":"claude",
                    "node_defs":[{"id":"worker","name":format!("Worker {}", index + 1),"node_type":"agent", "isolated_worktree":true}]
                }),
            )
            .await;
        }
        insert_event(
            &state.db,
            run_ids[0],
            "2026-07-20T09:00:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"}),
        )
        .await;
        insert_event(
            &state.db,
            run_ids[0],
            "2026-07-20T10:00:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-1"}),
        )
        .await;
        insert_event(
            &state.db,
            run_ids[0],
            "2026-07-20T11:00:00Z",
            "node_started",
            Some("worker"),
            Some(2),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-1"}),
        )
        .await;
        insert_event(
            &state.db,
            run_ids[1],
            "2026-07-21T09:00:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-2"}),
        )
        .await;
        insert_event(
            &state.db,
            run_ids[2],
            "2026-07-22T09:00:00Z",
            "node_started",
            Some("worker"),
            Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":false}),
        )
        .await;

        let claude_dir = home.join(".claude").join("projects").join(
            pdo_harness::claude_transcript::encode_working_dir(&worktrees::sub_worktree_path(
                &state.repo_root,
                run_ids[0],
                "worker",
                1,
            )),
        );
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("claude-1.jsonl"),
            r#"{"type":"assistant","requestId":"r1","message":{"id":"m1","model":"claude-opus-4-8","usage":{"input_tokens":1000000,"output_tokens":0}}}"#,
        )
        .unwrap();
        let legacy_dir = home.join(".claude").join("projects").join(
            pdo_harness::claude_transcript::encode_working_dir(&worktrees::worktree_dir_for_run(
                &state.repo_root,
                run_ids[2],
            )),
        );
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join("legacy.jsonl"),
            r#"{"type":"assistant","requestId":"r3","message":{"id":"m3","model":"claude-opus-4-8","usage":{"input_tokens":1000000,"output_tokens":0}}}"#,
        )
        .unwrap();
        let copilot_dir = home
            .join(".copilot")
            .join("session-state")
            .join("copilot-1");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(
            copilot_dir.join("events.jsonl"),
            r#"{"type":"session.shutdown","data":{"totalNanoAiu":100000000000}}"#,
        )
        .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/cost?from=2026-07-15T00:00:00Z&to=2026-07-18T00:00:00Z&bucket=day")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        assert_eq!(
            body["harnesses"],
            serde_json::json!(["claude", "copilot", "future-harness"])
        );
        assert_eq!(body["total"]["usd"], 11.0);
        assert_eq!(body["total"]["executions"], 3);
        assert_eq!(body["total"]["readable"], 0);
        assert_eq!(body["total"]["unknown"], 3);
        assert!(
            body["total"]["unknown"].as_i64().unwrap()
                <= body["total"]["executions"].as_i64().unwrap()
        );
        let claude = body["total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["harness"] == "claude")
            .unwrap();
        assert_eq!(claude["usd"], 10.0);
        assert!(claude["average_usd"].is_null());
        // #811: `median_usd` travels beside `average_usd` on the same readable
        // population — no execution's cost is readable here, so both stay null
        // rather than collapsing to $0.
        assert!(claude["median_usd"].is_null());
        assert_eq!(claude["estimated"], true);
        assert_eq!(claude["executions"], 3);
        assert_eq!(claude["readable"], 0);
        assert_eq!(claude["unknown"], 3);
        let future = body["total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["harness"] == "future-harness")
            .unwrap();
        assert!(future["usd"].is_null());
        assert_eq!(future["executions"], 2);
        assert_eq!(future["unknown"], 2);
        assert!(future["missing_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "harness has no cost source"));
        let copilot = body["total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["harness"] == "copilot")
            .unwrap();
        assert_eq!(copilot["estimated"], false);
        assert_eq!(copilot["executions"], 1);
        assert_eq!(copilot["readable"], 1);
        assert_eq!(copilot["unknown"], 0);
        assert_eq!(copilot["average_usd"], 1.0);
        assert_eq!(copilot["median_usd"], 1.0);
        for metric in body["total"]["harnesses"].as_array().unwrap() {
            assert!(metric["unknown"].as_i64().unwrap() <= metric["executions"].as_i64().unwrap());
        }

        assert_eq!(body["by_period"][0]["bucket"], "2026-07-15");
        assert_eq!(body["by_period"][0]["usd"], 6.0);
        let pipeline = &body["by_pipeline"][0];
        assert_eq!(pipeline["id"], "p1");
        assert_eq!(pipeline["name"], "Pipeline 3");
        assert_eq!(pipeline["executions"], 3);
        assert_eq!(pipeline["readable"], 0);
        assert_eq!(pipeline["unknown"], 3);
        let worker = pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "worker")
            .unwrap();
        assert_eq!(worker["name"], "Worker 3");
        assert_eq!(worker["executions"], 5);
        assert_eq!(worker["readable"], 2);
        assert_eq!(worker["unknown"], 3);
        // #811, mixed readable/unreadable: the three unreadable executions enter
        // neither figure, and the two readable ones leave a median (the midpoint
        // of a pair, so it coincides with the average here — same population,
        // two readings).
        assert!(!worker["median_usd"].is_null(), "got: {worker:#?}");
        assert_eq!(worker["median_usd"], worker["average_usd"]);
        let infrastructure = pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "Infrastructure")
            .unwrap();
        assert_eq!(infrastructure["executions"], 3);
        assert_eq!(infrastructure["unknown"], 3);
        let unassigned = pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "Unassigned")
            .unwrap();
        assert_eq!(unassigned["usd"], 5.0);
        assert_eq!(unassigned["readable"], 0);
        assert_eq!(body["by_project"][0]["id"], "prj-product");
        assert_eq!(body["by_project"][0]["name"], "Product");
        assert_eq!(body["by_project"][0]["executions"], 3);
        assert_eq!(body["by_project"][0]["readable"], 0);
        assert_eq!(body["by_project"][0]["unknown"], 3);
        assert_eq!(body["by_project"][0]["pipelines"][0]["id"], "p1");
        assert!(!body["resolved"].as_array().unwrap().is_empty());
    }

    /// #890 / ADR-0077 — the HTTP contract of an absorption: posed through the
    /// API, it folds Pipeline A into Pipeline B in Sessions, Cost (all three
    /// trees), Performance (both trees) and the Triggers fires; `uncombined=true`
    /// reads the raw rows; the Performance memo recomputes on every write; and
    /// removing the last member brings the original rows back.
    #[tokio::test]
    async fn stats_absorption_folds_pipelines_in_every_tab_and_undoes_cleanly() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-absorption")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // Two versions of one Pipeline: A (`alpha-old`, older) and B
        // (`alpha-new`). Both have a `worker` Node (same id → one Node row under
        // the absorbent); B's worker ran on a model nobody prices (a lower
        // bound, `partial`), and B has a Node on a harness with no cost source.
        let runs = [
            (
                "abs-a1",
                "alpha-old",
                "Alpha",
                "2032-03-15",
                "claude-opus-4-8",
                10,
            ),
            (
                "abs-b1",
                "alpha-new",
                "Alpha",
                "2032-03-16",
                "claude-unpriced-9",
                20,
            ),
        ];
        for (run, pipeline_id, name, day, model, minutes) in runs {
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:00:00Z"),
                "run_started",
                None,
                None,
                serde_json::json!({
                    "pipeline_id": pipeline_id,
                    "pipeline_name": name,
                    "target_repo": repo,
                    "harness": "claude",
                    "node_defs": [
                        {"id":"worker","name":"Worker","node_type":"agent","isolated_worktree":true},
                        {"id":"extra","name":"Extra","node_type":"agent","isolated_worktree":true}
                    ]
                }),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:01:00Z"),
                "node_started",
                Some("worker"),
                Some(1),
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","session_id":format!("{run}-s")}),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:{:02}:00Z", 1 + minutes),
                "node_completed",
                Some("worker"),
                Some(1),
                serde_json::json!({}),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:40:00Z"),
                "run_completed",
                None,
                None,
                serde_json::json!({}),
            )
            .await;
            let dir =
                home.join(".claude").join("projects").join(
                    pdo_harness::claude_transcript::encode_working_dir(
                        &worktrees::sub_worktree_path(&state.repo_root, run, "worker", 1),
                    ),
                );
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{run}-s.jsonl")),
                format!(
                    r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000000,"output_tokens":0}}}}}}"#
                ),
            )
            .unwrap();
        }
        insert_event(
            &state.db,
            "abs-b1",
            "2032-03-16T09:30:00Z",
            "node_started",
            Some("extra"),
            Some(1),
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"future-harness","session_id":"abs-b1-extra"}),
        )
        .await;
        // Trigger fires: 2 for A, 1 for B.
        for (id, pipeline) in [("abs-ta", "alpha-old"), ("abs-tb", "alpha-new")] {
            sqlx::query(
                "INSERT INTO triggers (id, name, pipeline_id, cron, enabled, created_at) \
                 VALUES (?, ?, ?, '0 2 * * *', 1, '2032-03-01T00:00:00Z')",
            )
            .bind(id)
            .bind(id)
            .bind(pipeline)
            .execute(&state.db)
            .await
            .unwrap();
        }
        for (trigger, ts) in [
            ("abs-ta", "2032-03-15T02:00:00Z"),
            ("abs-ta", "2032-03-16T02:00:00Z"),
            ("abs-tb", "2032-03-16T03:00:00Z"),
        ] {
            sqlx::query(
                "INSERT INTO trigger_fires (trigger_id, ts, outcome) VALUES (?, ?, 'fired')",
            )
            .bind(trigger)
            .bind(ts)
            .execute(&state.db)
            .await
            .unwrap();
        }

        async fn send(
            state: &Arc<AppState>,
            method: &str,
            uri: &str,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = Request::builder().method(method).uri(uri);
            if body.is_some() {
                request = request.header("content-type", "application/json");
            }
            let response = build_router(state.clone())
                .oneshot(
                    request
                        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            )
        }
        let window = "from=2032-03-15T00:00:00Z&to=2032-03-17T00:00:00Z";
        let (from, to) = ("2032-03-15T00:00:00Z", "2032-03-17T00:00:00Z");
        let overview_uri = format!("/stats/overview?{window}&bucket=day");
        let cost_uri = format!("/stats/cost?{window}&bucket=day");
        let performance_uri = format!("/stats/performance?{window}");
        let ids = |rows: &serde_json::Value| -> Vec<String> {
            let mut ids: Vec<String> = rows
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].as_str().unwrap().to_string())
                .collect();
            ids.sort();
            ids
        };
        let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .cloned()
                .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
        };

        // --- Before: two Pipeline rows everywhere, and the raw costs to add up.
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(
            ids(&overview["sessions_by_pipeline"]),
            vec!["alpha-new", "alpha-old"]
        );
        let (_, raw_cost) = send(&state, "GET", &cost_uri, None).await;
        let usd_old = find(&raw_cost["by_pipeline"], "alpha-old")["usd"]
            .as_f64()
            .unwrap();
        let usd_new = find(&raw_cost["by_pipeline"], "alpha-new")["usd"]
            .as_f64()
            .unwrap_or(0.0);
        assert!(!find(&raw_cost["by_pipeline"], "alpha-old")["partial"]
            .as_bool()
            .unwrap());
        let (_, performance_before) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            ids(&performance_before["by_pipeline"]),
            vec!["alpha-new", "alpha-old"]
        );
        let recomputes = stats_performance::recompute_count_for_test(from, to);

        // --- Combine: B absorbs A.
        let (status, list) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "pipeline",
                "absorbent": {"key": "alpha-new", "name": "Alpha"},
                "members": [{"key": "alpha-old", "name": "Alpha"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["absorptions"][0]["absorbent"]["key"], "alpha-new");
        assert_eq!(list["absorptions"][0]["members"][0]["key"], "alpha-old");
        assert_eq!(list["absorptions"][0]["members"][0]["origin"], "manual");

        // Sessions: one row, cumulated executions, one `worker` Node, members.
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(ids(&overview["sessions_by_pipeline"]), vec!["alpha-new"]);
        let row = find(&overview["sessions_by_pipeline"], "alpha-new");
        assert_eq!(row["name"], "Alpha");
        assert_eq!(row["executions"], 3);
        assert_eq!(row["runs"], 2);
        assert_eq!(row["last_run"], "2032-03-16T09:00:00Z");
        assert_eq!(find(&row["nodes"], "worker")["executions"], 2);
        assert_eq!(row["absorbed"][0]["key"], "alpha-old");
        assert_eq!(row["absorbed"][0]["runs"], 1);
        assert_eq!(row["absorbed"][0]["executions"], 1);
        assert_eq!(row["absorbed"][0]["last_run"], "2032-03-15T09:00:00Z");
        // Triggers: the fires follow the absorption.
        assert_eq!(
            overview["fires_by_pipeline"],
            serde_json::json!([{"pipeline_id": "alpha-new", "name": "Alpha", "count": 3}])
        );

        // Cost: one row per tree, the costs added up, the honesty marks kept.
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(ids(&cost["by_pipeline"]), vec!["alpha-new"]);
        let pipeline = find(&cost["by_pipeline"], "alpha-new");
        assert_eq!(pipeline["executions"], 2);
        assert_eq!(pipeline["usd"].as_f64().unwrap(), usd_old + usd_new);
        assert_eq!(pipeline["partial"], true, "B's lower bound shows through");
        assert_eq!(pipeline["absorbed"][0]["key"], "alpha-old");
        assert_eq!(find(&pipeline["nodes"], "worker")["executions"], 2);
        let infrastructure: Vec<_> = pipeline["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["name"] == "Infrastructure")
            .collect();
        assert_eq!(
            infrastructure.len(),
            1,
            "one Infrastructure row per absorbent"
        );
        let extra = find(&pipeline["nodes"], "extra");
        assert!(
            extra["usd"].is_null(),
            "an unknown cost stays unknown: {extra}"
        );
        let project_pipelines = &cost["by_project"][0]["pipelines"];
        assert_eq!(ids(project_pipelines), vec!["alpha-new"]);
        for model in cost["by_model"].as_array().unwrap() {
            for effort in model["efforts"].as_array().unwrap() {
                assert_eq!(ids(&effort["pipelines"]), vec!["alpha-new"]);
            }
        }

        // Performance: one row per tree, the medians recomputed over both Runs.
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            recomputes + 1,
            "a Combine must recompute the Performance memo"
        );
        assert_eq!(ids(&performance["by_pipeline"]), vec!["alpha-new"]);
        let pipeline = find(&performance["by_pipeline"], "alpha-new");
        assert_eq!(pipeline["absorbed"][0]["key"], "alpha-old");
        let claude = pipeline["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap()
            .clone();
        assert_eq!(claude["duration"]["measured"], 2);
        assert_eq!(claude["duration"]["stats"]["median"], 900000.0);
        assert_eq!(
            find(&pipeline["nodes"], "worker")["harnesses"][0]["duration"]["measured"],
            2
        );
        for model in performance["by_model"].as_array().unwrap() {
            for effort in model["efforts"].as_array().unwrap() {
                assert_eq!(ids(&effort["pipelines"]), vec!["alpha-new"]);
            }
        }

        // Uncombined: the raw rows, absorption kept.
        let (_, overview) = send(
            &state,
            "GET",
            &format!("{overview_uri}&uncombined=true"),
            None,
        )
        .await;
        assert_eq!(
            ids(&overview["sessions_by_pipeline"]),
            vec!["alpha-new", "alpha-old"]
        );
        assert!(find(&overview["sessions_by_pipeline"], "alpha-new")["absorbed"].is_null());
        assert_eq!(overview["fires_by_pipeline"].as_array().unwrap().len(), 2);
        let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
        assert_eq!(ids(&cost["by_pipeline"]), vec!["alpha-new", "alpha-old"]);
        let (_, performance) = send(
            &state,
            "GET",
            &format!("{performance_uri}&uncombined=true"),
            None,
        )
        .await;
        assert_eq!(
            ids(&performance["by_pipeline"]),
            vec!["alpha-new", "alpha-old"]
        );

        // Uncombine the last member: the absorption is gone, the rows are back.
        // The store is then exactly what it was before the Combine, so the
        // Performance memo answers from the entry that state computed — never
        // from the combined one.
        let recomputes = stats_performance::recompute_count_for_test(from, to);
        let (status, list) = send(
            &state,
            "DELETE",
            "/stats/absorptions/member?dimension=pipeline&member=alpha-old",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["absorptions"], serde_json::json!([]));
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(
            ids(&overview["sessions_by_pipeline"]),
            vec!["alpha-new", "alpha-old"]
        );
        let (_, after) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            after, performance_before,
            "the uncombined state reads as before"
        );
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            recomputes,
            "the pre-Combine memo entry serves the pre-Combine state"
        );
        let (status, _) = send(
            &state,
            "DELETE",
            "/stats/absorptions/member?dimension=pipeline&member=alpha-old",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// #892 / ADR-0077 — the HTTP contract of the Node and Model absorptions: a
    /// Node absorption folds two Nodes of one Pipeline row in the three
    /// `/stats/*` endpoints and every tree they show in (`uncombined=true`
    /// separates them); Nodes of two Pipelines are refused and write nothing;
    /// taking a Pipeline out of its absorption takes its Nodes out of the Node
    /// absorption of that row; a Model absorption folds two ids on the « By
    /// model » axis of Cost and Performance, their efforts meeting by effort,
    /// and leaves the Pipeline axis alone.
    #[tokio::test]
    async fn stats_absorption_folds_nodes_and_models_and_cascades_a_pipeline_removal() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-absorption-892")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(node.map(|_| 1_i64))
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // Pipeline `beta`: its reviewer Node was renamed between two versions
        // (`review` → `code-review`), and the model it ran on reads once as the
        // pinned alias `opus`, once as the observed id. Pipeline `gamma` runs a
        // `triage` Node, and a `review` Node of its own.
        let runs = [
            (
                "n-b1",
                "beta",
                "Beta",
                "2033-04-15",
                "review",
                "Review",
                "opus",
            ),
            (
                "n-b2",
                "beta",
                "Beta",
                "2033-04-16",
                "code-review",
                "Code review",
                "claude-opus-4-8",
            ),
            (
                "n-g1",
                "gamma",
                "Gamma",
                "2033-04-16",
                "triage",
                "Triage",
                "claude-opus-4-8",
            ),
            (
                "n-g2",
                "gamma",
                "Gamma",
                "2033-04-16",
                "review",
                "Review",
                "claude-sonnet-5",
            ),
        ];
        for (run, pipeline_id, name, day, node, node_name, model) in runs {
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:00:00Z"),
                "run_started",
                None,
                serde_json::json!({
                    "pipeline_id": pipeline_id,
                    "pipeline_name": name,
                    "target_repo": repo,
                    "harness": "claude",
                    "node_defs": [{"id": node, "name": node_name, "node_type": "agent", "isolated_worktree": true}]
                }),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:01:00Z"),
                "node_started",
                Some(node),
                serde_json::json!({
                    "node_type": "agent", "isolated_worktree": true, "harness": "claude",
                    "session_id": format!("{run}-s"), "model": model, "effort": "high"
                }),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:11:00Z"),
                "node_completed",
                Some(node),
                serde_json::json!({}),
            )
            .await;
            insert_event(
                &state.db,
                run,
                &format!("{day}T09:20:00Z"),
                "run_completed",
                None,
                serde_json::json!({}),
            )
            .await;
            let dir =
                home.join(".claude").join("projects").join(
                    pdo_harness::claude_transcript::encode_working_dir(
                        &worktrees::sub_worktree_path(&state.repo_root, run, node, 1),
                    ),
                );
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{run}-s.jsonl")),
                format!(
                    r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":0}}}}}}"#
                ),
            )
            .unwrap();
        }

        async fn send(
            state: &Arc<AppState>,
            method: &str,
            uri: &str,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = Request::builder().method(method).uri(uri);
            if body.is_some() {
                request = request.header("content-type", "application/json");
            }
            let response = build_router(state.clone())
                .oneshot(
                    request
                        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            )
        }
        let window = "from=2033-04-15T00:00:00Z&to=2033-04-17T00:00:00Z";
        let overview_uri = format!("/stats/overview?{window}&bucket=day");
        let cost_uri = format!("/stats/cost?{window}&bucket=day");
        let performance_uri = format!("/stats/performance?{window}");
        // Node rows only: the Infrastructure and Unassigned rows are no Node.
        let ids = |rows: &serde_json::Value| -> Vec<String> {
            let mut ids: Vec<String> = rows
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].as_str().unwrap().to_string())
                .filter(|id| !id.contains(':'))
                .collect();
            ids.sort();
            ids
        };
        let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .cloned()
                .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
        };
        let strs =
            |values: &[&str]| -> Vec<String> { values.iter().map(|v| v.to_string()).collect() };
        // Every tree a `beta` Node shows in, with the Node ids each one reads.
        let beta_nodes = |cost: &serde_json::Value, performance: &serde_json::Value| {
            let mut trees = vec![
                ids(&find(&cost["by_pipeline"], "beta")["nodes"]),
                ids(&find(&cost["by_project"][0]["pipelines"], "beta")["nodes"]),
                ids(&find(&performance["by_pipeline"], "beta")["nodes"]),
            ];
            for axis in [&cost["by_model"], &performance["by_model"]] {
                let mut nodes = std::collections::BTreeSet::new();
                for model in axis.as_array().unwrap() {
                    for effort in model["efforts"].as_array().unwrap() {
                        for pipeline in effort["pipelines"].as_array().unwrap() {
                            if pipeline["id"] == "beta" {
                                nodes.extend(ids(&pipeline["nodes"]));
                            }
                        }
                    }
                }
                trees.push(nodes.into_iter().collect());
            }
            trees
        };

        // --- Before: two Node rows under `beta`, everywhere.
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
            strs(&["code-review", "review"])
        );
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        for tree in beta_nodes(&cost, &performance) {
            assert_eq!(tree, strs(&["code-review", "review"]));
        }

        // --- Nodes of two Pipelines are refused, and nothing is written.
        let (status, refusal) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "node",
                "scope": "beta",
                "scope_name": "Beta",
                "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
                "members": [{"key": "triage", "name": "Triage", "scope": "gamma"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
        let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
        assert_eq!(list["absorptions"], serde_json::json!([]));
        let (status, _) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "node",
                "absorbent": {"key": "code-review", "name": "Code review"},
                "members": [{"key": "review", "name": "Review"}],
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a Node absorption needs its Pipeline"
        );

        // --- Combine the two Nodes of `beta`.
        let (status, list) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "node",
                "scope": "beta",
                "scope_name": "Beta",
                "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
                "members": [{"key": "review", "name": "Review", "scope": "beta"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["absorptions"][0]["dimension"], "node");
        assert_eq!(list["absorptions"][0]["scope_name"], "Beta");

        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        let beta = find(&overview["sessions_by_pipeline"], "beta");
        assert_eq!(ids(&beta["nodes"]), strs(&["code-review"]));
        let node = find(&beta["nodes"], "code-review");
        assert_eq!(node["name"], "Code review");
        assert_eq!(node["executions"], 2);
        assert_eq!(node["runs"], 2);
        assert_eq!(node["absorbed"][0]["key"], "review");
        assert_eq!(node["absorbed"][0]["name"], "Review");
        assert_eq!(node["absorbed"][0]["executions"], 1);
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "gamma")["nodes"]),
            strs(&["review", "triage"]),
            "another Pipeline's `review` stays itself"
        );
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        for tree in beta_nodes(&cost, &performance) {
            assert_eq!(tree, strs(&["code-review"]));
        }
        let cost_node = find(&find(&cost["by_pipeline"], "beta")["nodes"], "code-review");
        assert_eq!(cost_node["executions"], 2);
        assert_eq!(cost_node["absorbed"][0]["key"], "review");
        let performance_node = find(
            &find(&performance["by_pipeline"], "beta")["nodes"],
            "code-review",
        );
        assert_eq!(performance_node["harnesses"][0]["duration"]["measured"], 2);
        assert_eq!(performance_node["absorbed"][0]["key"], "review");

        // Uncombined: two Node rows again.
        let (_, overview) = send(
            &state,
            "GET",
            &format!("{overview_uri}&uncombined=true"),
            None,
        )
        .await;
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
            strs(&["code-review", "review"])
        );
        let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
        let (_, performance) = send(
            &state,
            "GET",
            &format!("{performance_uri}&uncombined=true"),
            None,
        )
        .await;
        for tree in beta_nodes(&cost, &performance) {
            assert_eq!(tree, strs(&["code-review", "review"]));
        }

        // --- Models: the alias and the observed id, one row on the model axis.
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let opus_executions = find(&cost["by_model"], "claude-opus-4-8")["executions"]
            .as_i64()
            .unwrap();
        assert_eq!(find(&cost["by_model"], "opus")["executions"], 1);
        let (status, list) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "model",
                "absorbent": {"key": "claude-opus-4-8", "name": "claude-opus-4-8"},
                "members": [{"key": "opus", "name": "opus"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            ids(&cost["by_model"]),
            strs(&["claude-opus-4-8", "claude-sonnet-5"])
        );
        let opus = find(&cost["by_model"], "claude-opus-4-8");
        assert_eq!(opus["executions"], opus_executions + 1);
        assert_eq!(
            ids(&opus["efforts"]),
            strs(&["high"]),
            "the efforts meet by effort"
        );
        assert_eq!(opus["efforts"][0]["executions"], opus_executions + 1);
        assert_eq!(opus["absorbed"][0]["key"], "opus");
        assert_eq!(opus["absorbed"][0]["runs"], 1);
        assert!(opus["provenance"].is_string());
        // #906 (ADR-0078 amending ADR-0077): the model absorption reaches the
        // Node's own couples too — one couple, marked as globally absorbed.
        let node_pairs =
            find(&find(&cost["by_pipeline"], "beta")["nodes"], "code-review")["models"].clone();
        let pairs: Vec<String> = node_pairs
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| pair["model"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(pairs, strs(&["claude-opus-4-8"]), "{node_pairs:#}");
        assert_eq!(node_pairs[0]["global_absorbed"][0]["key"], "opus|high");
        assert_eq!(node_pairs[0]["global_absorbed"][0]["name"], "opus · high");
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            ids(&performance["by_model"]),
            strs(&["claude-opus-4-8", "claude-sonnet-5"])
        );
        let opus = find(&performance["by_model"], "claude-opus-4-8");
        assert_eq!(ids(&opus["efforts"]), strs(&["high"]));
        assert_eq!(opus["absorbed"][0]["key"], "opus");
        let (_, cost) = send(&state, "GET", &format!("{cost_uri}&uncombined=true"), None).await;
        assert!(ids(&cost["by_model"]).contains(&"opus".to_string()));

        // --- Cascade: `beta` absorbs `gamma`, then `triage` joins `review`'s
        // absorbent under `beta`; taking `gamma` out takes `triage` out too.
        let (status, _) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "pipeline",
                "absorbent": {"key": "beta", "name": "Beta"},
                "members": [{"key": "gamma", "name": "Gamma"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "node",
                "scope": "gamma",
                "absorbent": {"key": "code-review", "name": "Code review"},
                "members": [{"key": "triage", "name": "Triage"}],
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "an absorbed Pipeline's Nodes combine under its absorbent"
        );
        let (status, _) = send(
            &state,
            "POST",
            "/stats/absorptions",
            Some(serde_json::json!({
                "dimension": "node",
                "scope": "beta",
                "scope_name": "Beta",
                "absorbent": {"key": "code-review", "name": "Code review", "scope": "beta"},
                "members": [{"key": "triage", "name": "Triage", "scope": "beta"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        let beta = find(&overview["sessions_by_pipeline"], "beta");
        // gamma's `review` meets beta's under `code-review` too (same id).
        assert_eq!(ids(&beta["nodes"]), strs(&["code-review"]));
        assert_eq!(find(&beta["nodes"], "code-review")["executions"], 4);

        let (status, list) = send(
            &state,
            "DELETE",
            "/stats/absorptions/member?dimension=pipeline&member=gamma",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let node_members: Vec<String> = list["absorptions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|absorption| absorption["dimension"] == "node")
            .flat_map(|absorption| absorption["members"].as_array().unwrap().clone())
            .map(|member| member["key"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            node_members,
            strs(&["review"]),
            "`triage` only ran under gamma: it left; `review` also runs under beta: it stays"
        );
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "gamma")["nodes"]),
            strs(&["review", "triage"]),
            "gamma's Nodes find their rows under gamma again"
        );
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
            strs(&["code-review"])
        );

        // Uncombine the Node from its icon: the two rows come back.
        let (status, _) = send(
            &state,
            "DELETE",
            "/stats/absorptions/member?dimension=node&scope=beta&member=review",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, overview) = send(&state, "GET", &overview_uri, None).await;
        assert_eq!(
            ids(&find(&overview["sessions_by_pipeline"], "beta")["nodes"]),
            strs(&["code-review", "review"])
        );
    }

    /// #906 (ADR-0078) — the effort absorption of one model and the couple
    /// absorption of one Node, both with a frozen scope, and the global
    /// absorptions reaching the couples of every Node. The figures are ADR-0078's:
    /// claude-opus-5 has *not set* 10 and high 20, claude-opus-5-5 has *not set* 5
    /// and high 8, claude-opus-4-8 has *not set* 3.
    #[tokio::test]
    async fn stats_absorption_folds_efforts_and_couples_with_a_frozen_scope() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-absorption-906")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(node.map(|_| 1_i64))
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // Pipeline `delta`: `coder` ran on claude-opus-5 and claude-opus-4-8,
        // `implementer` on claude-opus-5-5. One execution per Run.
        let groups: [(&str, &str, Option<&str>, usize); 5] = [
            ("coder", "claude-opus-5", None, 10),
            ("coder", "claude-opus-5", Some("high"), 20),
            ("implementer", "claude-opus-5-5", None, 5),
            ("implementer", "claude-opus-5-5", Some("high"), 8),
            ("coder", "claude-opus-4-8", None, 3),
        ];
        let mut index = 0;
        for (node, model, effort, count) in groups {
            for _ in 0..count {
                index += 1;
                let run = format!("e-{index:02}");
                let minute = format!("{:02}", index % 60);
                let hour = 8 + index / 60;
                let at = |second: &str| format!("2034-05-15T{hour:02}:{minute}:{second}Z");
                insert_event(
                    &state.db,
                    &run,
                    &at("00"),
                    "run_started",
                    None,
                    serde_json::json!({
                        "pipeline_id": "delta",
                        "pipeline_name": "Delta",
                        "target_repo": repo,
                        "harness": "claude",
                        "node_defs": [
                            {"id": "coder", "name": "Coder", "node_type": "agent", "isolated_worktree": true},
                            {"id": "implementer", "name": "Implementer", "node_type": "agent", "isolated_worktree": true}
                        ]
                    }),
                )
                .await;
                let mut started = serde_json::json!({
                    "node_type": "agent", "isolated_worktree": true, "harness": "claude",
                    "session_id": format!("{run}-s"), "model": model,
                });
                if let Some(effort) = effort {
                    started["effort"] = serde_json::json!(effort);
                }
                insert_event(
                    &state.db,
                    &run,
                    &at("10"),
                    "node_started",
                    Some(node),
                    started,
                )
                .await;
                insert_event(
                    &state.db,
                    &run,
                    &at("40"),
                    "node_completed",
                    Some(node),
                    serde_json::json!({}),
                )
                .await;
                insert_event(
                    &state.db,
                    &run,
                    &at("50"),
                    "run_completed",
                    None,
                    serde_json::json!({}),
                )
                .await;
                let dir = home.join(".claude").join("projects").join(
                    pdo_harness::claude_transcript::encode_working_dir(
                        &worktrees::sub_worktree_path(&state.repo_root, &run, node, 1),
                    ),
                );
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    dir.join(format!("{run}-s.jsonl")),
                    format!(
                        r#"{{"type":"assistant","requestId":"{run}","message":{{"id":"{run}-m","model":"{model}","usage":{{"input_tokens":1000,"output_tokens":0}}}}}}"#
                    ),
                )
                .unwrap();
            }
        }

        async fn send(
            state: &Arc<AppState>,
            method: &str,
            uri: &str,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = Request::builder().method(method).uri(uri);
            if body.is_some() {
                request = request.header("content-type", "application/json");
            }
            let response = build_router(state.clone())
                .oneshot(
                    request
                        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            )
        }
        let (from, to) = ("2034-05-15T00:00:00Z", "2034-05-16T00:00:00Z");
        let window = format!("from={from}&to={to}");
        let cost_uri = format!("/stats/cost?{window}&bucket=day");
        let performance_uri = format!("/stats/performance?{window}");
        let uncombined = "&uncombined=true";
        let find = |rows: &serde_json::Value, id: &str| -> serde_json::Value {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .cloned()
                .unwrap_or_else(|| panic!("no row {id} in {rows:#}"))
        };
        // effort id → executions (Cost) of one model row of the « By model » axis.
        let cost_efforts = |cost: &serde_json::Value, model: &str| -> Vec<(String, i64)> {
            find(&cost["by_model"], model)["efforts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["id"].as_str().unwrap().to_string(),
                        e["executions"].as_i64().unwrap(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect()
        };
        // effort id → measured durations (Performance), the same way.
        let performance_efforts = |perf: &serde_json::Value, model: &str| -> Vec<(String, i64)> {
            find(&perf["by_model"], model)["efforts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["id"].as_str().unwrap().to_string(),
                        e["harnesses"][0]["duration"]["measured"].as_i64().unwrap(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect()
        };
        let counts = |pairs: &[(&str, i64)]| -> Vec<(String, i64)> {
            pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
        };
        // couple key → executions of one Node row, in every tree it shows in:
        // Cost by pipeline, Cost by project, Performance by pipeline.
        let node_couples = |cost: &serde_json::Value,
                            perf: &serde_json::Value,
                            node: &str|
         -> Vec<Vec<(String, i64)>> {
            let cost_rows = [
                find(&find(&cost["by_pipeline"], "delta")["nodes"], node),
                find(
                    &find(&cost["by_project"][0]["pipelines"], "delta")["nodes"],
                    node,
                ),
            ];
            let mut trees: Vec<Vec<(String, i64)>> = cost_rows
                .iter()
                .map(|row| {
                    let mut pairs: Vec<(String, i64)> = row["models"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            (
                                p["key"].as_str().unwrap().to_string(),
                                p["executions"].as_i64().unwrap(),
                            )
                        })
                        .collect();
                    pairs.sort();
                    pairs
                })
                .collect();
            let perf_row = find(&find(&perf["by_pipeline"], "delta")["nodes"], node);
            let mut pairs: Vec<(String, i64)> = perf_row["models"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        p["key"].as_str().unwrap().to_string(),
                        p["harnesses"][0]["duration"]["measured"].as_i64().unwrap(),
                    )
                })
                .collect();
            pairs.sort();
            trees.push(pairs);
            trees
        };
        let couple_of = |tree: &serde_json::Value, node: &str, key: &str| -> serde_json::Value {
            find(&find(tree, "delta")["nodes"], node)["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["key"] == key)
                .cloned()
                .unwrap_or_else(|| panic!("no couple {key} under {node}"))
        };
        let post = |body: serde_json::Value| {
            let state = state.clone();
            async move { send(&state, "POST", "/stats/absorptions", Some(body)).await }
        };
        let delete = |query: String| {
            let state = state.clone();
            async move {
                send(
                    &state,
                    "DELETE",
                    &format!("/stats/absorptions/member?{query}"),
                    None,
                )
                .await
            }
        };
        let enc = |value: &str| -> String {
            value
                .bytes()
                .map(|byte| {
                    if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                        (byte as char).to_string()
                    } else {
                        format!("%{byte:02X}")
                    }
                })
                .collect()
        };
        let coder_scope = stats_absorption::couple_scope("delta", "coder");

        // --- Before: two efforts under claude-opus-5.
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5"),
            counts(&[("", 10), ("high", 20)])
        );
        let before_high = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "high");
        let before_not_set = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "");
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            performance_efforts(&performance, "claude-opus-5"),
            counts(&[("", 10), ("high", 20)])
        );
        let recomputes = stats_performance::recompute_count_for_test(from, to);

        // --- Efforts of two models are refused, and nothing is written.
        let (status, refusal) = post(serde_json::json!({
            "dimension": "effort",
            "scope": "claude-opus-5",
            "absorbent": {"key": "high", "name": "high", "scope": "claude-opus-5"},
            "members": [{"key": "", "name": "not set", "scope": "claude-opus-5-5"}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
        let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
        assert_eq!(list["absorptions"], serde_json::json!([]));

        // --- Effort absorption: *not set* → high on claude-opus-5.
        let (status, list) = post(serde_json::json!({
            "dimension": "effort",
            "scope": "claude-opus-5",
            "scope_name": "claude-opus-5",
            "absorbent": {"key": "high", "name": "high", "scope": "claude-opus-5"},
            "members": [{"key": "", "name": "not set", "scope": "claude-opus-5"}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["absorptions"][0]["dimension"], "effort");
        assert_eq!(list["absorptions"][0]["scope"], "claude-opus-5");
        assert_eq!(list["absorptions"][0]["scope_name"], "claude-opus-5");
        assert_eq!(list["absorptions"][0]["members"][0]["key"], "");

        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5"),
            counts(&[("high", 30)])
        );
        let high = find(&find(&cost["by_model"], "claude-opus-5")["efforts"], "high");
        assert_eq!(high["runs"], 30);
        assert_eq!(high["absorbed"][0]["key"], "");
        assert_eq!(high["absorbed"][0]["name"], "not set");
        assert_eq!(high["absorbed"][0]["runs"], 10);
        assert_eq!(
            high["absorbed"][0]["scopes"],
            serde_json::json!(["claude-opus-5"])
        );
        // The honesty marks add up, they are never smoothed away.
        for field in ["readable", "unknown"] {
            assert_eq!(
                high[field].as_i64().unwrap(),
                before_high[field].as_i64().unwrap() + before_not_set[field].as_i64().unwrap(),
                "{field}"
            );
        }
        assert_eq!(
            high["partial"].as_bool().unwrap(),
            before_high["partial"].as_bool().unwrap()
                || before_not_set["partial"].as_bool().unwrap()
        );
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            recomputes + 1,
            "an effort absorption must recompute the Performance memo"
        );
        assert_eq!(
            performance_efforts(&performance, "claude-opus-5"),
            counts(&[("high", 30)])
        );
        assert_eq!(
            find(
                &find(&performance["by_model"], "claude-opus-5")["efforts"],
                "high"
            )["absorbed"][0]["key"],
            ""
        );
        // It reaches the couples of `coder`, in every tree, marked as global.
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(
                tree,
                counts(&[("claude-opus-4-8|", 3), ("claude-opus-5|high", 30)])
            );
        }
        for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
            let couple = couple_of(tree, "coder", "claude-opus-5|high");
            assert_eq!(couple["global_absorbed"][0]["key"], "claude-opus-5|");
            assert_eq!(
                couple["global_absorbed"][0]["name"],
                "claude-opus-5 · not set"
            );
            assert_eq!(couple["global_absorbed"][0]["runs"], 10);
            assert!(couple_of(tree, "coder", "claude-opus-4-8|")["global_absorbed"].is_null());
        }
        // Uncombined: both efforts and both couples again.
        let (_, raw_cost) = send(&state, "GET", &format!("{cost_uri}{uncombined}"), None).await;
        let (_, raw_performance) = send(
            &state,
            "GET",
            &format!("{performance_uri}{uncombined}"),
            None,
        )
        .await;
        assert_eq!(
            cost_efforts(&raw_cost, "claude-opus-5"),
            counts(&[("", 10), ("high", 20)])
        );
        assert_eq!(
            performance_efforts(&raw_performance, "claude-opus-5"),
            counts(&[("", 10), ("high", 20)])
        );
        for tree in node_couples(&raw_cost, &raw_performance, "coder") {
            assert_eq!(
                tree,
                counts(&[
                    ("claude-opus-4-8|", 3),
                    ("claude-opus-5|", 10),
                    ("claude-opus-5|high", 20)
                ])
            );
        }

        // --- ADR-0078, case 2: claude-opus-5 absorbs claude-opus-4-8 after the
        // pose — the newcomer's *not set* stays apart.
        let (status, _) = post(serde_json::json!({
            "dimension": "model",
            "absorbent": {"key": "claude-opus-5", "name": "claude-opus-5"},
            "members": [{"key": "claude-opus-4-8", "name": "claude-opus-4-8"}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5"),
            counts(&[("", 3), ("high", 30)])
        );
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            performance_efforts(&performance, "claude-opus-5"),
            counts(&[("", 3), ("high", 30)])
        );
        // The model absorption reaches `coder`'s couples too.
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(
                tree,
                counts(&[("claude-opus-5|", 3), ("claude-opus-5|high", 30)])
            );
        }
        for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
            let couple = couple_of(tree, "coder", "claude-opus-5|");
            assert_eq!(couple["global_absorbed"][0]["key"], "claude-opus-4-8|");
        }
        let (status, _) = delete("dimension=model&member=claude-opus-4-8".to_string()).await;
        assert_eq!(status, StatusCode::OK);

        // --- ADR-0078, case 1: claude-opus-5-5 absorbs claude-opus-5 — the
        // absorption keeps to claude-opus-5's Runs: high 38, *not set* 5.
        let (status, _) = post(serde_json::json!({
            "dimension": "model",
            "absorbent": {"key": "claude-opus-5-5", "name": "claude-opus-5-5"},
            "members": [{"key": "claude-opus-5", "name": "claude-opus-5"}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5-5"),
            counts(&[("", 5), ("high", 38)])
        );
        let high = find(
            &find(&cost["by_model"], "claude-opus-5-5")["efforts"],
            "high",
        );
        assert_eq!(high["absorbed"][0]["key"], "", "the member still shows");
        assert_eq!(
            high["absorbed"][0]["scopes"],
            serde_json::json!(["claude-opus-5"])
        );
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            performance_efforts(&performance, "claude-opus-5-5"),
            counts(&[("", 5), ("high", 38)])
        );
        // An effort absorption on a model row counted under another is refused.
        let (status, _) = post(serde_json::json!({
            "dimension": "effort",
            "scope": "claude-opus-5",
            "absorbent": {"key": "high", "name": "high"},
            "members": [{"key": "low", "name": "low"}],
        }))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // --- ADR-0078, case 3: taking claude-opus-5 out — it comes back with
        // high 30 and no *not set*.
        let (status, _) = delete("dimension=model&member=claude-opus-5".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5"),
            counts(&[("high", 30)])
        );
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5-5"),
            counts(&[("", 5), ("high", 8)])
        );

        // --- Couple absorption in `coder`: claude-opus-4-8 · not set into
        // claude-opus-5 · high. One couple row, never on the « By model » axis.
        let (status, refusal) = post(serde_json::json!({
            "dimension": "couple",
            "scope": coder_scope,
            "absorbent": {"key": "claude-opus-5|high", "name": "claude-opus-5 · high", "scope": coder_scope},
            "members": [{"key": "claude-opus-5-5|high", "name": "claude-opus-5-5 · high",
                         "scope": stats_absorption::couple_scope("delta", "implementer")}],
        }))
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "couples of two Nodes: {refusal}"
        );
        let recomputes = stats_performance::recompute_count_for_test(from, to);
        let (status, list) = post(serde_json::json!({
            "dimension": "couple",
            "scope": coder_scope,
            "scope_name": "Coder (Delta)",
            "absorbent": {"key": "claude-opus-5|high", "name": "claude-opus-5 · high", "scope": coder_scope},
            "members": [{"key": "claude-opus-4-8|", "name": "claude-opus-4-8 · not set", "scope": coder_scope}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        let couple_view = list["absorptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["dimension"] == "couple")
            .cloned()
            .unwrap();
        assert_eq!(couple_view["scope_name"], "Coder (Delta)");
        assert_eq!(couple_view["absorbent"]["name"], "claude-opus-5 · high");

        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            recomputes + 1,
            "a couple absorption must recompute the Performance memo"
        );
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(tree, counts(&[("claude-opus-5|high", 33)]));
        }
        for tree in [&cost["by_pipeline"], &performance["by_pipeline"]] {
            let couple = couple_of(tree, "coder", "claude-opus-5|high");
            assert_eq!(couple["absorbed"][0]["key"], "claude-opus-4-8|");
            assert_eq!(couple["absorbed"][0]["runs"], 3);
            assert_eq!(
                couple["absorbed"][0]["scopes"],
                serde_json::json!([coder_scope])
            );
        }
        // The « By model » axis never sees it.
        assert_eq!(cost_efforts(&cost, "claude-opus-4-8"), counts(&[("", 3)]));
        assert_eq!(
            performance_efforts(&performance, "claude-opus-4-8"),
            counts(&[("", 3)])
        );
        let (_, raw_cost) = send(&state, "GET", &format!("{cost_uri}{uncombined}"), None).await;
        let (_, raw_performance) = send(
            &state,
            "GET",
            &format!("{performance_uri}{uncombined}"),
            None,
        )
        .await;
        for tree in node_couples(&raw_cost, &raw_performance, "coder") {
            assert_eq!(tree.len(), 3, "Uncombined shows every raw couple: {tree:?}");
        }

        // --- Global before local: claude-opus-5-5 absorbs claude-opus-5, the
        // local absorbent's model — it carries the local absorption along.
        let (status, _) = post(serde_json::json!({
            "dimension": "model",
            "absorbent": {"key": "claude-opus-5-5", "name": "claude-opus-5-5"},
            "members": [{"key": "claude-opus-5", "name": "claude-opus-5"}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(tree, counts(&[("claude-opus-5-5|high", 33)]));
        }
        let (status, _) = delete("dimension=model&member=claude-opus-5".to_string()).await;
        assert_eq!(status, StatusCode::OK);

        // --- Frozen scope of couples: `implementer` absorbs `coder` — its own
        // couples are not combined; `coder`'s stay combined.
        let (status, _) = post(serde_json::json!({
            "dimension": "node",
            "scope": "delta",
            "scope_name": "Delta",
            "absorbent": {"key": "implementer", "name": "Implementer", "scope": "delta"},
            "members": [{"key": "coder", "name": "Coder", "scope": "delta"}],
        }))
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        for tree in node_couples(&cost, &performance, "implementer") {
            assert_eq!(
                tree,
                counts(&[
                    ("claude-opus-5-5|", 5),
                    ("claude-opus-5-5|high", 8),
                    ("claude-opus-5|high", 33)
                ])
            );
        }
        let (status, _) = delete("dimension=node&scope=delta&member=coder".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(
                tree,
                counts(&[("claude-opus-5|high", 33)]),
                "coder comes back with its couple combined"
            );
        }

        // --- Uncombine from the members list / Settings: the rows come back.
        // (A state the memo has already seen is served from it: take the effort
        // out first, leaving a couple-only state it never computed.)
        let recomputes = stats_performance::recompute_count_for_test(from, to);
        let (status, _) = delete("dimension=effort&scope=claude-opus-5&member=".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            recomputes + 1,
            "an Uncombine must recompute the Performance memo"
        );
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(
                tree,
                counts(&[("claude-opus-5|", 10), ("claude-opus-5|high", 23)]),
                "the couple absorption stays, the effort one is gone"
            );
        }
        let (status, list) = delete(format!(
            "dimension=couple&scope={}&member={}",
            enc(&coder_scope),
            enc("claude-opus-4-8|")
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["absorptions"], serde_json::json!([]));
        let (_, cost) = send(&state, "GET", &cost_uri, None).await;
        let (_, performance) = send(&state, "GET", &performance_uri, None).await;
        assert_eq!(
            cost_efforts(&cost, "claude-opus-5"),
            counts(&[("", 10), ("high", 20)])
        );
        for tree in node_couples(&cost, &performance, "coder") {
            assert_eq!(tree.len(), 3);
        }
    }

    /// `GET /stats/performance` — the whole shape in one cohort: success-only
    /// filtering (a loop lap, a stopped-then-restarted attempt, a script node, a
    /// failed Run, a failed merge-resolver attempt all excluded/retained per the
    /// issue's rules), stable Pipeline/Node identity with "latest name wins",
    /// Pipeline aggregation pooling from raw main sessions (not from the Nodes'
    /// own means), the whole-cohort `total`, Claude subagent grouping (declared
    /// name + the "Unidentified subagent" fallback for an opaque id), a harness
    /// with no context-usage source still keeping its Duration, and both
    /// Infrastructure roles (Pipeline Manager, Merge resolver).
    #[tokio::test]
    async fn stats_performance_returns_pipeline_node_subagent_and_infrastructure_metrics() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // --- run_started: two runs of pipeline p1 (latest name "Loop V2" wins),
        // one run of pipeline p2, one run of p1 that will fail outright (so its
        // Pipeline Manager attempt is excluded, never even "expected").
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"p1","pipeline_name":"Loop V1","target_repo":repo,"harness":"claude",
                "node_defs":[
                    {"id":"worker","name":"Worker V1","node_type":"agent", "isolated_worktree":true},
                    {"id":"build","name":"Build","node_type":"script"}
                ]
            }),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r2",
            "2026-07-16T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"p1","pipeline_name":"Loop V2","target_repo":repo,"harness":"copilot",
                "node_defs":[{"id":"worker","name":"Worker V2","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r3",
            "2026-07-17T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"p2","pipeline_name":"Other pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"solo","name":"Solo","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r4",
            "2026-07-18T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"p1","pipeline_name":"Loop V2","target_repo":repo,"harness":"claude",
                "node_defs":[]
            }),
        )
        .await;

        // --- perf-r1: worker's loop lap (iter 1, 2), a stopped attempt on iter 3
        // that is then restarted and succeeds, a fourth iter on a harness with no
        // context source, plus a script node that must never appear at all.
        insert_event(
            &state.db, "perf-r1", "2026-07-15T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db, "perf-r1", "2026-07-15T09:30:00Z", "node_started",
            Some("worker"), Some(2),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-2"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:35:00Z",
            "node_completed",
            Some("worker"),
            Some(2),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db, "perf-r1", "2026-07-15T09:40:00Z", "node_started",
            Some("worker"), Some(3),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-3"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:41:00Z",
            "node_stopped",
            Some("worker"),
            Some(3),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db, "perf-r1", "2026-07-15T09:45:00Z", "node_started",
            Some("worker"), Some(3),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-3b"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:50:00Z",
            "node_completed",
            Some("worker"),
            Some(3),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db, "perf-r1", "2026-07-15T10:00:00Z", "node_started",
            Some("worker"), Some(4),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"future-harness","session_id":"future-1"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T10:05:00Z",
            "node_completed",
            Some("worker"),
            Some(4),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:10:00Z",
            "node_started",
            Some("build"),
            Some(1),
            serde_json::json!({"node_type":"script","harness":"claude","session_id":"build-1"}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:11:00Z",
            "node_completed",
            Some("build"),
            Some(1),
            serde_json::json!({}),
        )
        .await;

        // Pipeline Manager: one successful Run.
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:55:00Z",
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
        // Merge resolver: one failed attempt (excluded), then one successful
        // pairing scoped to worker's iter-3 restart (its own worktree).
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:51:00Z",
            "merge_resolver_started",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:51:30Z",
            "merge_resolver_failed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:52:00Z",
            "merge_resolver_started",
            None,
            None,
            serde_json::json!({"conflicting_node_id":"worker","iter":3}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r1",
            "2026-07-15T09:53:00Z",
            "merge_resolver_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // --- perf-r2: worker's one Copilot execution.
        insert_event(
            &state.db, "perf-r2", "2026-07-16T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-1"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r2",
            "2026-07-16T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;

        // --- perf-r3: p2's solo node, one Claude execution. Also a second
        // successful Run, pooled into the same Pipeline Manager role.
        insert_event(
            &state.db, "perf-r3", "2026-07-17T09:10:00Z", "node_started",
            Some("solo"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-solo"}),
        ).await;
        insert_event(
            &state.db,
            "perf-r3",
            "2026-07-17T09:16:00Z",
            "node_completed",
            Some("solo"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-r3",
            "2026-07-17T09:20:00Z",
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // --- perf-r4: the whole Run fails outright — never an "expected"
        // Pipeline Manager attempt at all.
        insert_event(
            &state.db,
            "perf-r4",
            "2026-07-18T09:05:00Z",
            "run_failed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // --- Claude transcripts (main sessions + one Node's two subagents).
        let claude_root = home.join(".claude").join("projects");
        let write_claude_main = |run_id: &str,
                                 node_id: &str,
                                 iter: i64,
                                 session: &str,
                                 tokens: u64| {
            let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
                &worktrees::sub_worktree_path(&state.repo_root, run_id, node_id, iter),
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{session}.jsonl")),
                format!(
                    r#"{{"type":"assistant","message":{{"model":"claude-opus-4-8","usage":{{"input_tokens":{tokens},"output_tokens":0}}}}}}"#
                ),
            )
            .unwrap();
            dir
        };
        let worker1_dir = write_claude_main("perf-r1", "worker", 1, "claude-1", 20000);
        write_claude_main("perf-r1", "worker", 2, "claude-2", 25000);
        let worker3_dir = write_claude_main("perf-r1", "worker", 3, "claude-3b", 30000);
        write_claude_main("perf-r3", "solo", 1, "claude-solo", 45000);

        // --- Infrastructure subagents (#585 user story #36): a Pipeline
        // Manager session left in the Run's own top-level worktree (never
        // colliding with an isolated Node's disjoint sub-worktree) and a
        // Merge resolver session left in the conflicting Node's own
        // iter-3 worktree, alongside (and excluded from) that Node's own
        // "claude-3b" session — resolved by exclusion, never by path alone.
        let pm_project_dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::worktree_dir_for_run(&state.repo_root, "perf-r1"),
        ));
        std::fs::create_dir_all(&pm_project_dir).unwrap();
        std::fs::write(
            pm_project_dir.join("pm-session.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":12000,"output_tokens":0}}}"#,
        )
        .unwrap();
        let pm_subagents_dir = pm_project_dir.join("pm-session").join("subagents");
        std::fs::create_dir_all(&pm_subagents_dir).unwrap();
        std::fs::write(
            pm_subagents_dir.join("reviewer.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:50:10Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":8000,\"output_tokens\":0}}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:50:40Z\"}\n",
        )
        .unwrap();

        std::fs::write(
            worker3_dir.join("mr-session.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":9000,"output_tokens":0}}}"#,
        )
        .unwrap();
        let mr_subagents_dir = worker3_dir.join("mr-session").join("subagents");
        std::fs::create_dir_all(&mr_subagents_dir).unwrap();
        std::fs::write(
            mr_subagents_dir.join("patch-reviewer.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:52:10Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":7000,\"output_tokens\":0}}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:52:40Z\"}\n",
        )
        .unwrap();

        let subagents_dir = worker1_dir.join("claude-1").join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();
        std::fs::write(
            subagents_dir.join("explore.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-07-15T09:10:00Z\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":55000,\"output_tokens\":0}}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-07-15T09:15:00Z\"}\n",
        )
        .unwrap();
        std::fs::write(
            subagents_dir.join("3fa85f64-5717-4562-b3fc-2c963f66afa6.jsonl"),
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":30000,\"output_tokens\":0}}}\n",
        )
        .unwrap();

        // --- Copilot journal for the one Copilot execution.
        let copilot_dir = home
            .join(".copilot")
            .join("session-state")
            .join("copilot-1");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(
            copilot_dir.join("events.jsonl"),
            r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":40000,"outputTokens":0}}}"#,
        )
        .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-07-15T00:00:00Z&to=2026-07-19T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        assert_eq!(
            body["harnesses"],
            serde_json::json!(["claude", "copilot", "future-harness"])
        );

        // --- Pipeline identity + latest-name-wins.
        let by_pipeline = body["by_pipeline"].as_array().unwrap();
        assert_eq!(by_pipeline.len(), 2, "got: {by_pipeline:#?}");
        let p1 = by_pipeline.iter().find(|p| p["id"] == "p1").unwrap();
        assert_eq!(p1["name"], "Loop V2");
        let p2 = by_pipeline.iter().find(|p| p["id"] == "p2").unwrap();
        assert_eq!(p2["name"], "Other pipeline");

        // --- Node identity + latest-name-wins + success-only filtering: 3
        // observations (loop lap + restart), never 4 (stopped attempt excluded)
        // or 5 (script node excluded).
        let nodes = p1["nodes"].as_array().unwrap();
        assert_eq!(
            nodes.len(),
            1,
            "the script node must never appear: {nodes:#?}"
        );
        let worker = &nodes[0];
        assert_eq!(worker["id"], "worker");
        assert_eq!(worker["name"], "Worker V2");
        let worker_harnesses = worker["harnesses"].as_array().unwrap();
        let worker_claude = worker_harnesses
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(worker_claude["context"]["measured"], 3);
        assert_eq!(worker_claude["context"]["expected"], 3);
        assert_eq!(worker_claude["context"]["stats"]["min"], 20000.0);
        assert_eq!(worker_claude["context"]["stats"]["max"], 30000.0);
        assert_eq!(worker_claude["context"]["stats"]["mean"], 25000.0);
        assert_eq!(worker_claude["context"]["stats"]["median"], 25000.0);
        // #811 — the Tukey fences the « Fenced » zoom level draws its whiskers
        // at, on every `by_pipeline` distribution. This cohort holds no outlier,
        // so they land on min/max: the same picture as « Full », which is the
        // honest answer, not a missing field.
        assert_eq!(worker_claude["context"]["stats"]["fence_low"], 20000.0);
        assert_eq!(worker_claude["context"]["stats"]["fence_high"], 30000.0);
        assert_eq!(worker_claude["duration"]["measured"], 3);
        assert_eq!(worker_claude["duration"]["stats"]["min"], 300000.0);
        assert_eq!(worker_claude["duration"]["stats"]["max"], 600000.0);
        assert_eq!(worker_claude["duration"]["stats"]["mean"], 400000.0);
        let worker_copilot = worker_harnesses
            .iter()
            .find(|h| h["harness"] == "copilot")
            .unwrap();
        assert_eq!(worker_copilot["context"]["stats"]["mean"], 40000.0);
        assert_eq!(worker_copilot["duration"]["stats"]["mean"], 600000.0);
        let worker_future = worker_harnesses
            .iter()
            .find(|h| h["harness"] == "future-harness")
            .unwrap();
        assert!(
            worker_future["context"]["stats"].is_null(),
            "a harness with no context source must never fake a value: {worker_future}"
        );
        assert_eq!(
            worker_future["context"]["measured"], 0,
            "coverage survives a fully absent metric: {worker_future}"
        );
        assert_eq!(worker_future["context"]["expected"], 1);
        assert_eq!(
            worker_future["context"]["missing_reasons"],
            serde_json::json!(["harness has no context-usage source"])
        );
        assert_eq!(worker_future["duration"]["stats"]["mean"], 300000.0);
        assert_eq!(worker_future["duration"]["measured"], 1);
        assert_eq!(worker_future["duration"]["expected"], 1);

        // --- Subagent grouping: declared name + "Unidentified subagent" fallback,
        // never touching the parent Node's own distribution above.
        let subagents = worker["subagents"].as_array().unwrap();
        assert_eq!(subagents.len(), 2, "got: {subagents:#?}");
        let explore = subagents.iter().find(|s| s["name"] == "explore").unwrap();
        let explore_claude = explore["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(explore_claude["context"]["stats"]["mean"], 55000.0);
        assert_eq!(explore_claude["duration"]["stats"]["mean"], 300000.0);
        let unidentified = subagents
            .iter()
            .find(|s| s["name"] == "Unidentified subagent")
            .unwrap();
        let unidentified_claude = unidentified["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(unidentified_claude["context"]["stats"]["mean"], 30000.0);
        assert!(
            unidentified_claude["duration"]["stats"].is_null(),
            "no reliable bounds in this subagent transcript: {unidentified_claude}"
        );
        assert_eq!(unidentified_claude["duration"]["measured"], 0);
        assert_eq!(
            unidentified_claude["duration"]["missing_reasons"],
            serde_json::json!(["no reliable start/end bounds in subagent transcript"])
        );

        // --- Pipeline aggregation pools from raw main sessions, matching its own
        // single Node here exactly (p1 has only one Node).
        let p1_claude = p1["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(p1_claude["context"]["stats"]["mean"], 25000.0);

        // --- p2's solo Node.
        let p2_worker = &p2["nodes"].as_array().unwrap()[0];
        assert_eq!(p2_worker["id"], "solo");
        let p2_claude = p2_worker["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(p2_claude["context"]["stats"]["mean"], 45000.0);

        // --- `total`: every main-session Claude observation across BOTH
        // pipelines pooled together (never Infrastructure, never subagents).
        let total_claude = body["total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(total_claude["context"]["measured"], 4);
        assert_eq!(total_claude["context"]["stats"]["min"], 20000.0);
        assert_eq!(total_claude["context"]["stats"]["max"], 45000.0);
        assert_eq!(total_claude["context"]["stats"]["mean"], 30000.0);
        assert_eq!(total_claude["context"]["stats"]["median"], 27500.0);

        // --- Infrastructure: Pipeline Manager pools BOTH successful Runs (never
        // the failed one); Merge resolver keeps only its successful pairing.
        // Both roles now discover their own residual Claude session — and its
        // subagents — by exclusion from the directory their turns are known to
        // land in (#585 user story #36), never guessing at the ambiguous ones.
        let infra = body["infrastructure"].as_array().unwrap();
        assert_eq!(infra.len(), 2, "got: {infra:#?}");
        let pipeline_manager = infra
            .iter()
            .find(|r| r["id"] == "pipeline-manager")
            .unwrap();
        assert_eq!(pipeline_manager["name"], "Pipeline Manager");
        let pm_claude = pipeline_manager["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        // Perf-r1's Run resolves a session (measured); perf-r3's Run leaves no
        // attributable session in its own top-level worktree (absent, but
        // coverage survives: expected still counts it).
        assert_eq!(pm_claude["context"]["measured"], 1);
        assert_eq!(pm_claude["context"]["expected"], 2);
        assert_eq!(pm_claude["context"]["stats"]["mean"], 12000.0);
        assert_eq!(
            pm_claude["context"]["missing_reasons"],
            serde_json::json!(["no attributable session for this infrastructure role"])
        );
        assert_eq!(pm_claude["duration"]["measured"], 2);
        assert_eq!(pm_claude["duration"]["expected"], 2);
        assert_eq!(pm_claude["duration"]["stats"]["min"], 1200000.0);
        assert_eq!(pm_claude["duration"]["stats"]["max"], 3300000.0);
        // #811 — fences on the `infrastructure` tree too, including the
        // one-observation Context distribution, where they collapse onto the
        // single value (never a null, never a theoretical bound).
        assert_eq!(pm_claude["duration"]["stats"]["fence_low"], 1200000.0);
        assert_eq!(pm_claude["duration"]["stats"]["fence_high"], 3300000.0);
        assert_eq!(pm_claude["context"]["stats"]["fence_low"], 12000.0);
        assert_eq!(pm_claude["context"]["stats"]["fence_high"], 12000.0);
        let pm_subagents = pipeline_manager["subagents"].as_array().unwrap();
        assert_eq!(pm_subagents.len(), 1, "got: {pm_subagents:#?}");
        let pm_reviewer_claude = pm_subagents[0]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(pm_subagents[0]["name"], "reviewer");
        assert_eq!(pm_reviewer_claude["context"]["stats"]["mean"], 8000.0);
        assert_eq!(pm_reviewer_claude["duration"]["stats"]["mean"], 30000.0);

        let merge_resolver = infra.iter().find(|r| r["id"] == "merge-resolver").unwrap();
        assert_eq!(merge_resolver["name"], "Merge resolver");
        let mr_claude = merge_resolver["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(mr_claude["context"]["measured"], 1);
        assert_eq!(mr_claude["context"]["stats"]["mean"], 9000.0);
        assert_eq!(mr_claude["duration"]["measured"], 1);
        assert_eq!(mr_claude["duration"]["stats"]["mean"], 60000.0);
        let mr_subagents = merge_resolver["subagents"].as_array().unwrap();
        assert_eq!(mr_subagents.len(), 1, "got: {mr_subagents:#?}");
        let mr_patch_claude = mr_subagents[0]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(mr_subagents[0]["name"], "patch-reviewer");
        assert_eq!(mr_patch_claude["context"]["stats"]["mean"], 7000.0);
        assert_eq!(mr_patch_claude["duration"]["stats"]["mean"], 30000.0);

        // --- infrastructure_total: both Infrastructure roles' raw observations
        // pooled together — NOT the mean of Pipeline Manager's mean and Merge
        // resolver's mean (#585 review follow-up, blocker 3). Context pools
        // Pipeline Manager's one measured peak (12000) with Merge resolver's
        // one measured peak (9000): 2 measured out of 3 expected occurrences
        // (Pipeline Manager's own expected=2 + Merge resolver's own expected=1).
        // Duration pools all three occurrences directly, since every occurrence
        // is paired Run/resolver timestamps with no absence possible here.
        let infra_total_claude = body["infrastructure_total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(infra_total_claude["context"]["expected"], 3);
        assert_eq!(infra_total_claude["context"]["measured"], 2);
        assert_eq!(infra_total_claude["context"]["stats"]["mean"], 10500.0);
        assert_eq!(infra_total_claude["duration"]["expected"], 3);
        assert_eq!(infra_total_claude["duration"]["measured"], 3);
        assert_eq!(infra_total_claude["duration"]["stats"]["mean"], 1520000.0);
    }

    /// `GET /stats/performance` (#585, user story #36) — an Infrastructure
    /// role's own session is never guessed at by path or timing alone: a
    /// non-isolated Claude Node sharing the Pipeline Manager's top-level
    /// worktree, with no recorded `session_id`, makes that directory's
    /// residual session ambiguous — Context (and its subagents) stays a
    /// motivated absence rather than an unsafe guess, while Duration (paired
    /// from `RunStarted`/`RunCompleted` timestamps, no session lookup
    /// involved) is unaffected.
    #[tokio::test]
    async fn stats_performance_infrastructure_context_is_ambiguous_when_a_shared_session_has_no_recorded_identity(
    ) {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-ambiguous")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        insert_event(
            &state.db,
            "perf-amb",
            "2026-08-01T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"amb","pipeline_name":"Ambiguous","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"docs","name":"Docs","node_type":"agent", "isolated_worktree":false}]
            }),
        )
        .await;
        // A non-isolated (so it shares the Run's top-level worktree)
        // Claude Node with no recorded `session_id` — the directory can no
        // longer be trusted to hold only the Pipeline Manager's own session.
        insert_event(
            &state.db,
            "perf-amb",
            "2026-08-01T09:10:00Z",
            "node_started",
            Some("docs"),
            Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":false,"harness":"claude"}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-amb",
            "2026-08-01T09:15:00Z",
            "node_completed",
            Some("docs"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "perf-amb",
            "2026-08-01T09:20:00Z",
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // A residual session does exist in the shared directory — but it must
        // never be attributed, since the co-resident Node's own identity is
        // unknown.
        let claude_root = home.join(".claude").join("projects");
        let pm_project_dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::worktree_dir_for_run(&state.repo_root, "perf-amb"),
        ));
        std::fs::create_dir_all(&pm_project_dir).unwrap();
        std::fs::write(
            pm_project_dir.join("pm-session.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":12000,"output_tokens":0}}}"#,
        )
        .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        let infra = body["infrastructure"].as_array().unwrap();
        let pipeline_manager = infra
            .iter()
            .find(|r| r["id"] == "pipeline-manager")
            .unwrap();
        let pm_claude = pipeline_manager["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(pm_claude["context"]["measured"], 0);
        assert_eq!(pm_claude["context"]["expected"], 1);
        assert!(pm_claude["context"]["stats"].is_null());
        assert_eq!(
            pm_claude["context"]["missing_reasons"],
            serde_json::json!(["ambiguous session identity in a directory shared with a Node"])
        );
        assert!(
            pipeline_manager["subagents"].as_array().unwrap().is_empty(),
            "an ambiguous session must never contribute subagents either: {pipeline_manager}"
        );
        // Duration is untouched by the ambiguity — it never looked at a
        // session file at all.
        assert_eq!(pm_claude["duration"]["measured"], 1);
        assert_eq!(pm_claude["duration"]["stats"]["mean"], 1200000.0);
    }

    /// #585 review follow-up (blocker 4): Copilot's journal has no declared
    /// subagent concept, and its store is flat — this must be a **motivated**
    /// absence (the dispatch answers empty), not a coincidence of never having
    /// the right directory to look in. Proven by planting a directory that
    /// *would* read as subagents under Claude's convention right next to the
    /// Copilot session's own journal, and asserting it never surfaces: the
    /// Copilot execution's own Context is still read correctly (the absence is
    /// specific to subagent discovery, not a wholesale failure to read Copilot
    /// at all).
    #[tokio::test]
    async fn stats_performance_copilot_subagents_are_a_motivated_absence_not_silently_missing() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-copilot-subagents")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-cs1', '2026-08-01T09:00:00Z', 'run_started', NULL, NULL, ?)",
        )
        .bind(
            serde_json::json!({
                "pipeline_id":"p1","pipeline_name":"Loop","target_repo":repo,"harness":"copilot",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            })
            .to_string(),
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-cs1', '2026-08-01T09:10:00Z', 'node_started', 'worker', 1, ?)",
        )
        .bind(
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"copilot-cs1"})
                .to_string(),
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-cs1', '2026-08-01T09:20:00Z', 'node_completed', 'worker', 1, '{}')",
        )
        .execute(&state.db)
        .await
        .unwrap();

        let copilot_session_dir = home
            .join(".copilot")
            .join("session-state")
            .join("copilot-cs1");
        std::fs::create_dir_all(&copilot_session_dir).unwrap();
        std::fs::write(
            copilot_session_dir.join("events.jsonl"),
            r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":12000,"outputTokens":0}}}"#,
        )
        .unwrap();
        // Plant a directory that reads as subagents under Claude's own
        // convention, right next to the Copilot session's journal.
        let lookalike_subagents = copilot_session_dir.join("subagents");
        std::fs::create_dir_all(&lookalike_subagents).unwrap();
        std::fs::write(
            lookalike_subagents.join("reviewer.jsonl"),
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":999,\"output_tokens\":0}}}\n",
        )
        .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        let by_pipeline = body["by_pipeline"].as_array().unwrap();
        let p1 = by_pipeline.iter().find(|p| p["id"] == "p1").unwrap();
        let worker = &p1["nodes"].as_array().unwrap()[0];
        let worker_copilot = worker["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "copilot")
            .unwrap();
        // The motivated absence is scoped to subagents — Context itself still
        // reads correctly for the Copilot execution.
        assert_eq!(worker_copilot["context"]["measured"], 1);
        assert_eq!(worker_copilot["context"]["stats"]["mean"], 12000.0);
        assert!(
            worker["subagents"].as_array().unwrap().is_empty(),
            "a look-alike subagents/ directory must never surface for copilot: {worker}"
        );
    }

    /// `GET /stats/performance` (#585 review follow-up, blocker 2) — the
    /// result is cached in memory: a repeat request for the identical `[from,
    /// to)` window, with nothing in the cohort's events or the underlying
    /// Claude transcript changed, is served from the memo — [`fold_performance`]
    /// (the transcript-reading heavy fold) never runs a second time — while
    /// still returning the exact same payload a fresh computation would.
    #[tokio::test]
    async fn stats_performance_result_is_memoized_and_reused_when_nothing_changed() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-cache")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        insert_event(
            &state.db,
            "cache-r1",
            "2031-01-15T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"cp1","pipeline_name":"Cache Pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db, "cache-r1", "2031-01-15T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-1"}),
        )
        .await;
        insert_event(
            &state.db,
            "cache-r1",
            "2031-01-15T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;

        let claude_root = home.join(".claude").join("projects");
        let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::sub_worktree_path(&state.repo_root, "cache-r1", "worker", 1),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cache-1.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10000,"output_tokens":0}}}"#,
        )
        .unwrap();

        let from = "2031-01-15T00:00:00Z";
        let to = "2031-01-16T00:00:00Z";
        let uri = format!("/stats/performance?from={from}&to={to}");

        async fn call(state: &Arc<AppState>, uri: &str) -> serde_json::Value {
            let response = build_router(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        let first = call(&state, &uri).await;
        let after_first = stats_performance::recompute_count_for_test(from, to);
        assert_eq!(
            after_first, 1,
            "first request must run the heavy fold exactly once"
        );

        let second = call(&state, &uri).await;
        let after_second = stats_performance::recompute_count_for_test(from, to);
        assert_eq!(
            after_second, after_first,
            "an identical repeat request must be served from the memo, not recomputed"
        );
        assert_eq!(
            first, second,
            "a memo hit must return byte-for-byte the same payload"
        );
        let refreshed = call(&state, &format!("{uri}&refresh=true")).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            after_second + 1,
            "an explicit user refresh must bypass the memo"
        );
        assert_eq!(first, refreshed);
        let worker_claude = first["by_pipeline"][0]["nodes"][0]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(worker_claude["context"]["stats"]["mean"], 10000.0);
    }

    /// `GET /stats/performance` (#585 review follow-up, blocker 2) — the memo
    /// key covers both halves the issue calls out: cohort **event** state (a
    /// newly appended Run changes the answer on the very next identical
    /// request) and **transcript/journal source** state (rewriting a session's
    /// own file — same events, same session id, new content/mtime — does too).
    /// Neither can silently serve a stale answer.
    #[tokio::test]
    async fn stats_performance_cache_is_invalidated_by_new_events_and_by_transcript_changes() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-cache-invalidation")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        insert_event(
            &state.db,
            "cache-i1",
            "2031-02-15T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"cip1","pipeline_name":"Cache Invalidation Pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db, "cache-i1", "2031-02-15T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-i-1"}),
        )
        .await;
        insert_event(
            &state.db,
            "cache-i1",
            "2031-02-15T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;

        let claude_root = home.join(".claude").join("projects");
        let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::sub_worktree_path(&state.repo_root, "cache-i1", "worker", 1),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cache-i-1.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10000,"output_tokens":0}}}"#,
        )
        .unwrap();

        let from = "2031-02-15T00:00:00Z";
        let to = "2031-02-17T00:00:00Z";
        let uri = format!("/stats/performance?from={from}&to={to}");

        async fn call(state: &Arc<AppState>, uri: &str) -> serde_json::Value {
            let response = build_router(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        let first = call(&state, &uri).await;
        assert_eq!(stats_performance::recompute_count_for_test(from, to), 1);
        let worker_claude_first = first["by_pipeline"][0]["nodes"][0]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap()
            .clone();
        assert_eq!(worker_claude_first["context"]["measured"], 1);
        assert_eq!(worker_claude_first["context"]["stats"]["mean"], 10000.0);

        // --- (a) cohort event state: a second Run appears in the same window.
        insert_event(
            &state.db,
            "cache-i2",
            "2031-02-16T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"cip1","pipeline_name":"Cache Invalidation Pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db, "cache-i2", "2031-02-16T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"cache-i-2"}),
        )
        .await;
        insert_event(
            &state.db,
            "cache-i2",
            "2031-02-16T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        let dir2 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::sub_worktree_path(&state.repo_root, "cache-i2", "worker", 1),
        ));
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(
            dir2.join("cache-i-2.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":20000,"output_tokens":0}}}"#,
        )
        .unwrap();

        let after_new_run = call(&state, &uri).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            2,
            "a new Run in the same window must force a recompute"
        );
        let worker_claude_after_new_run = after_new_run["by_pipeline"][0]["nodes"][0]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(
            worker_claude_after_new_run["context"]["measured"], 2,
            "the new Run's own Context observation must now be pooled: {after_new_run:#?}"
        );

        // --- (b) transcript/journal source state: rewrite the FIRST Run's own
        // session file in place (no new event at all) with a different token
        // count.
        std::fs::write(
            dir.join("cache-i-1.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":99000,"output_tokens":0}}}"#,
        )
        .unwrap();

        let after_transcript_rewrite = call(&state, &uri).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            3,
            "rewriting a session's own transcript, with no new event, must still force a recompute"
        );
        let worker_claude_after_rewrite = after_transcript_rewrite["by_pipeline"][0]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == "worker")
            .unwrap()["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap()
            .clone();
        // Node-level rows sum over both Runs' own Worker iter-1 sessions; only
        // cache-i1's was rewritten (10000 -> 99000), cache-i2's (20000) is
        // unaffected — the mean must reflect the rewrite, not the stale value.
        assert_eq!(worker_claude_after_rewrite["context"]["measured"], 2);
        assert_eq!(
            worker_claude_after_rewrite["context"]["stats"]["mean"],
            (99000.0 + 20000.0) / 2.0
        );
    }

    /// `GET /stats/performance` (#737, ADR-0065) — the « By model » axis: the
    /// peak of each session file is attributed to the model of THAT file (a
    /// subagent has its own bucket), a mute source falls back to the requested
    /// model × effort (marked `requested`), a harness with an observed source
    /// (`copilot`) buckets its journal's model + effort, and the `by_pipeline`
    /// Node leaf carries its model × effort couples — the drill "Node → modèle
    /// × effort". The existing memo fingerprints cover the axis: a subagent
    /// file's rewrite forces a recompute, an unchanged state is a memo hit.
    #[tokio::test]
    async fn stats_performance_by_model_ventilates_each_session_file_to_its_own_model() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-by-model")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // perf-m1: a claude Node run twice — iter 1 whose session file names
        // its model (opus) and whose subagent runs on another one (sonnet);
        // iter 2 whose file names no model at all (mute source), falling back
        // to the requested model × effort.
        insert_event(
            &state.db,
            "perf-m1",
            "2026-08-01T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"mp1","pipeline_name":"Model loop","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db, "perf-m1", "2026-08-01T09:10:00Z", "node_started",
            Some("worker"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"m1-iter1"}),
        ).await;
        insert_event(
            &state.db,
            "perf-m1",
            "2026-08-01T09:20:00Z",
            "node_completed",
            Some("worker"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db, "perf-m1", "2026-08-01T09:30:00Z", "node_started",
            Some("worker"), Some(2),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"m1-iter2","model":"sonnet","effort":"high"}),
        ).await;
        insert_event(
            &state.db,
            "perf-m1",
            "2026-08-01T09:35:00Z",
            "node_completed",
            Some("worker"),
            Some(2),
            serde_json::json!({}),
        )
        .await;
        // perf-m2: one copilot execution — its journal names the model AND the
        // effort at the usage points, so both are observed.
        insert_event(
            &state.db,
            "perf-m2",
            "2026-08-02T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"mp2","pipeline_name":"Other pipeline","target_repo":repo,"harness":"copilot",
                "node_defs":[{"id":"solo","name":"Solo","node_type":"agent", "isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db, "perf-m2", "2026-08-02T09:10:00Z", "node_started",
            Some("solo"), Some(1),
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"copilot","session_id":"m-copilot-1"}),
        ).await;
        insert_event(
            &state.db,
            "perf-m2",
            "2026-08-02T09:20:00Z",
            "node_completed",
            Some("solo"),
            Some(1),
            serde_json::json!({}),
        )
        .await;

        // --- claude transcripts: iter 1's main file (opus) + subagent file
        // (sonnet — its own bucket), iter 2's mute file (no model line).
        let claude_root = home.join(".claude").join("projects");
        let dir1 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::sub_worktree_path(&state.repo_root, "perf-m1", "worker", 1),
        ));
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::write(
            dir1.join("m1-iter1.jsonl"),
            r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":20000,"output_tokens":0}}}"#,
        )
        .unwrap();
        let subagents_dir = dir1.join("m1-iter1").join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();
        std::fs::write(
            subagents_dir.join("explore.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:10:00Z\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":55000,\"output_tokens\":0}}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-08-01T09:15:00Z\"}\n",
        )
        .unwrap();
        let dir2 = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
            &worktrees::sub_worktree_path(&state.repo_root, "perf-m1", "worker", 2),
        ));
        std::fs::create_dir_all(&dir2).unwrap();
        // A file that reads but says nothing: no model line, no readable peak.
        std::fs::write(dir2.join("m1-iter2.jsonl"), "{not json\n").unwrap();

        // --- copilot journal: the model and effort at the usage point, plus
        // the peak reading the shutdown carries.
        let copilot_dir = home
            .join(".copilot")
            .join("session-state")
            .join("m-copilot-1");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(
            copilot_dir.join("events.jsonl"),
            r#"{"type":"session.start","data":{"selectedModel":"gpt-5","reasoningEffort":"medium"}}
{"type":"session.usage_checkpoint","data":{"totalNanoAiu":5000000000,"modelCacheState":[{"modelId":"gpt-5"}]}}
{"type":"session.shutdown","data":{"usage":{"inputTokens":40000,"outputTokens":0}}}"#,
        )
        .unwrap();

        let from = "2026-08-01T00:00:00Z";
        let to = "2026-08-03T00:00:00Z";
        let uri = format!("/stats/performance?from={from}&to={to}");
        async fn call(state: &Arc<AppState>, uri: &str) -> serde_json::Value {
            let response = build_router(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        let body = call(&state, &uri).await;
        let by_model = body["by_model"].as_array().unwrap();
        let ids: Vec<&str> = by_model
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["claude-opus-4-8", "claude-sonnet-4-5", "gpt-5", "sonnet"],
            "one verbatim row per model id, harness-agnostic: {by_model:#?}"
        );

        let find_model = |body: &serde_json::Value, id: &str| {
            body["by_model"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .unwrap()
                .clone()
        };
        let node_of =
            |model: &serde_json::Value| model["efforts"][0]["pipelines"][0]["nodes"][0].clone();

        // Observed main-session model: the Node's peak + the execution's
        // wall-clock, effort « not set » (no effort in the startup event).
        let opus = find_model(&body, "claude-opus-4-8");
        assert_eq!(opus["provenance"], "observed");
        assert_eq!(opus["efforts"][0]["effort"], serde_json::Value::Null);
        assert_eq!(opus["efforts"][0]["name"], "not set");
        assert_eq!(opus["efforts"][0]["pipelines"][0]["id"], "mp1");
        let opus_node = node_of(&opus);
        assert_eq!(opus_node["id"], "worker");
        assert_eq!(opus_node["harnesses"][0]["harness"], "claude");
        assert_eq!(opus_node["harnesses"][0]["context"]["measured"], 1);
        assert_eq!(
            opus_node["harnesses"][0]["context"]["stats"]["max"],
            20000.0
        );
        assert_eq!(
            opus_node["harnesses"][0]["duration"]["stats"]["max"],
            600000.0
        );
        // #811 — fences on the `by_model` tree as well; one observation puts
        // both of them on it.
        assert_eq!(
            opus_node["harnesses"][0]["context"]["stats"]["fence_low"],
            20000.0
        );
        assert_eq!(
            opus_node["harnesses"][0]["context"]["stats"]["fence_high"],
            20000.0
        );
        // The by_model Node row carries no couples — the path IS the drill, so
        // the empty field is omitted on the wire (never an empty array).
        assert!(opus_node.get("models").is_none());

        // The subagent file's own model: its own bucket, its own peak and its
        // own transcript time span as duration.
        let sonnet_observed = find_model(&body, "claude-sonnet-4-5");
        assert_eq!(sonnet_observed["provenance"], "observed");
        let sonnet_node = node_of(&sonnet_observed);
        assert_eq!(sonnet_node["id"], "worker");
        assert_eq!(
            sonnet_node["harnesses"][0]["context"]["stats"]["max"],
            55000.0
        );
        assert_eq!(
            sonnet_node["harnesses"][0]["duration"]["stats"]["max"],
            300000.0
        );

        // A mute source falls back to the requested model × effort, marked.
        let sonnet_requested = find_model(&body, "sonnet");
        assert_eq!(sonnet_requested["provenance"], "requested");
        assert_eq!(sonnet_requested["efforts"][0]["effort"], "high");
        assert_eq!(sonnet_requested["efforts"][0]["provenance"], "requested");
        let requested_node = node_of(&sonnet_requested);
        assert_eq!(requested_node["id"], "worker");
        assert_eq!(requested_node["harnesses"][0]["context"]["measured"], 0);
        assert_eq!(
            requested_node["harnesses"][0]["context"]["missing_reasons"],
            serde_json::json!(["no readable context usage in transcript"])
        );
        assert_eq!(
            requested_node["harnesses"][0]["duration"]["stats"]["max"],
            300000.0
        );

        // copilot: model AND effort observed from the journal; the context peak
        // rides the same journal.
        let gpt5 = find_model(&body, "gpt-5");
        assert_eq!(gpt5["provenance"], "observed");
        assert_eq!(gpt5["efforts"][0]["effort"], "medium");
        assert_eq!(gpt5["efforts"][0]["provenance"], "observed");
        let gpt5_node = node_of(&gpt5);
        assert_eq!(gpt5_node["id"], "solo");
        assert_eq!(
            gpt5_node["harnesses"][0]["context"]["stats"]["max"],
            40000.0
        );
        assert_eq!(
            gpt5_node["harnesses"][0]["duration"]["stats"]["max"],
            600000.0
        );

        // --- by_pipeline is untouched at the top, but its Node leaf now ends
        // with the model × effort couples.
        let worker = body["by_pipeline"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "mp1")
            .unwrap()["nodes"]
            .as_array()
            .unwrap()[0]
            .clone();
        assert_eq!(worker["harnesses"][0]["context"]["measured"], 1);
        assert_eq!(
            worker["harnesses"][0]["context"]["missing_reasons"],
            serde_json::json!(["no readable context usage in transcript"])
        );
        let couples = worker["models"].as_array().unwrap();
        assert_eq!(couples.len(), 3, "got: {couples:#?}");
        let couple_keys: Vec<String> = couples
            .iter()
            .map(|pair| {
                format!(
                    "{}·{}",
                    pair["model"].as_str().unwrap(),
                    pair["effort"].as_str().unwrap_or("not set")
                )
            })
            .collect();
        assert_eq!(
            couple_keys,
            vec![
                "claude-opus-4-8·not set",
                "claude-sonnet-4-5·not set",
                "sonnet·high"
            ]
        );
        let opus_couple = &couples[0];
        assert_eq!(opus_couple["model_provenance"], "observed");
        assert_eq!(opus_couple["effort_provenance"], serde_json::Value::Null);
        assert_eq!(
            opus_couple["harnesses"][0]["context"]["stats"]["max"],
            20000.0
        );
        let sonnet_high_couple = &couples[2];
        assert_eq!(sonnet_high_couple["model_provenance"], "requested");
        assert_eq!(sonnet_high_couple["effort_provenance"], "requested");
        let sonnet_sub_couple = &couples[1];
        assert_eq!(sonnet_sub_couple["model_provenance"], "observed");
        assert_eq!(
            sonnet_sub_couple["harnesses"][0]["context"]["stats"]["max"],
            55000.0
        );

        // The whole-cohort total pools the main-session observations only —
        // never a subagent, never Infrastructure. Untouched by the axis.
        let total_claude = body["total"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["harness"] == "claude")
            .unwrap();
        assert_eq!(total_claude["context"]["measured"], 1);

        // --- the memo key covers the axis: unchanged state is a hit, a
        // subagent file's rewrite (no event at all) forces a recompute that
        // moves ITS model's bucket.
        let _ = call(&state, &uri).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            1,
            "an unchanged state must be a memo hit"
        );
        std::fs::write(
            subagents_dir.join("explore.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-08-01T09:10:00Z\",\"message\":{\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":66000,\"output_tokens\":0}}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-08-01T09:15:00Z\"}\n",
        )
        .unwrap();
        let after_rewrite = call(&state, &uri).await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            2,
            "rewriting a subagent file, with no new event, must force a recompute"
        );
        let rewritten = find_model(&after_rewrite, "claude-sonnet-4-5");
        assert_eq!(
            node_of(&rewritten)["harnesses"][0]["context"]["stats"]["max"],
            66000.0
        );
    }

    /// An empty cohort (no Runs in the window) is not an error — an empty
    /// Performance payload, distinct from the source-wide-unreadable case below.
    /// #792: the Steering metric at the derivation seam — a forged event log
    /// plus session files in temporary roots for the three instrumented
    /// harnesses, read back through `GET /stats/performance`.
    #[tokio::test]
    async fn stats_performance_steering_counts_typed_turns_after_launch_per_harness() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-steering")
            .join(uuid::Uuid::new_v4().to_string());
        let _cleanup = Cleanup(home.clone());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        let run = "steer-r1";
        insert_event(
            &state.db,
            run,
            "2026-08-01T09:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"sp","pipeline_name":"Steered pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[
                    {"id":"coder","name":"Coder","node_type":"agent","isolated_worktree":true},
                    {"id":"pilot","name":"Pilot","node_type":"agent","isolated_worktree":true},
                    {"id":"pi-node","name":"Pi node","node_type":"agent","isolated_worktree":true},
                    {"id":"ship","name":"Ship","node_type":"agent","isolated_worktree":true}
                ]
            }),
        )
        .await;
        let mut minute = 10;
        let mut events = Vec::new();
        for (node, iter, harness, session) in [
            ("coder", 1, "claude", "c1"),
            ("coder", 2, "claude", "c2"),
            ("coder", 3, "claude", "c3"),
            ("pilot", 1, "copilot", "cp1"),
            ("pi-node", 1, "pi", "pi1"),
            ("ship", 1, "opencode", "oc1"),
        ] {
            let start = format!("2026-08-01T09:{minute:02}:00Z");
            let end = format!("2026-08-01T09:{:02}:00Z", minute + 1);
            minute += 2;
            let payload = serde_json::json!({
                "node_type":"agent","isolated_worktree":true,"harness":harness,"session_id":session
            });
            events.push((node, iter, start, end, payload));
        }
        for (node, iter, start, end, payload) in &events {
            insert_event(
                &state.db,
                run,
                start,
                "node_started",
                Some(node),
                Some(*iter),
                payload.clone(),
            )
            .await;
            insert_event(
                &state.db,
                run,
                end,
                "node_completed",
                Some(node),
                Some(*iter),
                serde_json::json!({}),
            )
            .await;
        }
        insert_event(
            &state.db,
            run,
            "2026-08-01T09:40:00Z",
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // --- claude: c1 = launch + 2 human turns + 1 runtime turn + a tool
        // result, a skill load (isMeta) and a compaction summary — count 2. Its
        // subagent file carries human-looking turns that must never count. c2 =
        // launch only — count 0 (readable, autonomous). c3 = assistant lines only
        // — unreadable, never 0.
        let claude_root = home.join(".claude").join("projects");
        let assistant = r#"{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":1000,"output_tokens":0}}}"#;
        let user = |text: &str| {
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":{}}}}}"#,
                serde_json::Value::from(text)
            )
        };
        let claude_dir = |iter: i64| {
            let dir = claude_root.join(pdo_harness::claude_transcript::encode_working_dir(
                &worktrees::sub_worktree_path(&state.repo_root, run, "coder", iter),
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let c1_dir = claude_dir(1);
        std::fs::write(
            c1_dir.join("c1.jsonl"),
            [
                user("launch prompt").as_str(),
                assistant,
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"x"}]},"toolUseResult":{"stdout":"x"}}"#,
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: /s"}]}}"#,
                user("first steer").as_str(),
                assistant,
                user("[pdo-runtime] your output is missing frontmatter").as_str(),
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"This session is being continued"}}"#,
                user("second steer").as_str(),
                assistant,
            ]
            .join("\n"),
        )
        .unwrap();
        let sub_dir = c1_dir.join("c1").join("subagents");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(
            sub_dir.join("explore.jsonl"),
            [
                user("subagent prompt").as_str(),
                assistant,
                user("looks like a steer but nobody typed it").as_str(),
            ]
            .join("\n"),
        )
        .unwrap();
        std::fs::write(
            claude_dir(2).join("c2.jsonl"),
            [user("launch prompt").as_str(), assistant].join("\n"),
        )
        .unwrap();
        std::fs::write(claude_dir(3).join("c3.jsonl"), assistant).unwrap();

        // --- copilot: launch + runtime + one human turn — count 1.
        let copilot_dir = home.join(".copilot").join("session-state").join("cp1");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(
            copilot_dir.join("events.jsonl"),
            [
                r#"{"type":"session.start","data":{"selectedModel":"gpt-5.6-sol","reasoningEffort":"medium"}}"#,
                r#"{"type":"user.message","data":{"content":"launch prompt"}}"#,
                r#"{"type":"user.message","data":{"content":"[pdo-runtime] retry"}}"#,
                r#"{"type":"user.message","data":{"content":"human steer"}}"#,
                r#"{"type":"session.shutdown","data":{"usage":{"inputTokens":4000,"outputTokens":0},"totalNanoAiu":1000000000}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        // --- pi: launch + two human turns — count 2.
        let pi_dir = home.join(".pi").join("agent").join("sessions").join(
            pdo_harness::pi_session::session_dir_name(&worktrees::sub_worktree_path(
                &state.repo_root,
                run,
                "pi-node",
                1,
            )),
        );
        std::fs::create_dir_all(&pi_dir).unwrap();
        std::fs::write(
            pi_dir.join("2026-08-01T09-18-00-000Z_pi1.jsonl"),
            [
                r#"{"type":"session","id":"pi1"}"#,
                r#"{"type":"message","id":"a","message":{"role":"user","content":[{"type":"text","text":"launch prompt"}]}}"#,
                r#"{"type":"message","id":"b","message":{"role":"assistant","model":"claude-sonnet-4-5","provider":"anthropic","content":[{"type":"text","text":"ok"}],"usage":{"totalTokens":3000,"cost":{"total":0.01}}}}"#,
                r#"{"type":"message","id":"c","message":{"role":"user","content":"steer one"}}"#,
                r#"{"type":"message","id":"d","message":{"role":"user","content":"steer two"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-08-01T00:00:00Z&to=2026-08-02T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();

        let harness = |row: &serde_json::Value, name: &str| -> serde_json::Value {
            row["harnesses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h["harness"] == name)
                .unwrap_or_else(|| panic!("no {name} in {row:#?}"))
                .clone()
        };
        let pipeline = &body["by_pipeline"][0];
        assert_eq!(pipeline["id"], "sp");
        let node = |id: &str| -> serde_json::Value {
            pipeline["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id)
                .unwrap()
                .clone()
        };

        // claude: two readable counts (2, 0) out of three executions, one steered.
        let coder = harness(&node("coder"), "claude");
        assert_eq!(coder["steering"]["measured"], 2, "{coder:#?}");
        assert_eq!(coder["steering"]["expected"], 3);
        assert_eq!(coder["steering"]["stats"]["max"], 2.0);
        assert_eq!(coder["steering"]["stats"]["min"], 0.0);
        assert_eq!(coder["steering"]["stats"]["mean"], 1.0);
        assert_eq!(
            coder["steering"]["missing_reasons"],
            serde_json::json!(["no readable human turn in transcript"])
        );
        assert_eq!(
            coder["steered"],
            serde_json::json!({"steered":1,"readable":2})
        );
        // The additive contract: the two existing metrics are untouched.
        assert_eq!(coder["context"]["measured"], 3);
        assert_eq!(coder["duration"]["measured"], 3);

        // The subagent is never steered: a declared absence, no expected observation.
        let explore = harness(&node("coder")["subagents"][0], "claude");
        assert_eq!(explore["steering"]["expected"], 0);
        assert_eq!(explore["steering"]["measured"], 0);
        assert_eq!(
            explore["steering"]["missing_reasons"],
            serde_json::json!(["subagents are never steered"])
        );
        assert_eq!(
            explore["steered"],
            serde_json::json!({"steered":0,"readable":0})
        );

        // copilot / pi: their own formats.
        let pilot = harness(&node("pilot"), "copilot");
        assert_eq!(pilot["steering"]["stats"]["max"], 1.0, "{pilot:#?}");
        assert_eq!(
            pilot["steered"],
            serde_json::json!({"steered":1,"readable":1})
        );
        let pi_node = harness(&node("pi-node"), "pi");
        assert_eq!(pi_node["steering"]["stats"]["max"], 2.0, "{pi_node:#?}");
        assert_eq!(
            pi_node["steered"],
            serde_json::json!({"steered":1,"readable":1})
        );

        // opencode: unavailable with a named reason, never 0.
        let ship = harness(&node("ship"), "opencode");
        assert_eq!(ship["steering"]["measured"], 0);
        assert_eq!(ship["steering"]["expected"], 1);
        assert!(ship["steering"]["stats"].is_null());
        assert_eq!(
            ship["steering"]["missing_reasons"],
            serde_json::json!(["session not resolvable on opencode"])
        );
        assert_eq!(
            ship["steered"],
            serde_json::json!({"steered":0,"readable":0})
        );

        // The cohort total pools the raw counts: 2, 0 (claude) — per harness.
        let total_claude = harness(&body["total"], "claude");
        assert_eq!(
            total_claude["steered"],
            serde_json::json!({"steered":1,"readable":2})
        );

        // Infrastructure: « runtime messages only », never 0.
        let pm = &body["infrastructure"][0];
        assert_eq!(pm["id"], "pipeline-manager");
        let pm_claude = harness(pm, "claude");
        assert_eq!(pm_claude["steering"]["expected"], 1);
        assert_eq!(pm_claude["steering"]["measured"], 0);
        assert_eq!(
            pm_claude["steering"]["missing_reasons"],
            serde_json::json!(["runtime messages only"])
        );
        let infra_total = harness(&body["infrastructure_total"], "claude");
        assert_eq!(
            infra_total["steered"],
            serde_json::json!({"steered":0,"readable":0})
        );

        // The « By model » axis carries the metric too, down to the Node row,
        // and the Node's own model × effort couples on the by_pipeline side.
        let by_model = body["by_model"].as_array().unwrap();
        let opus = by_model
            .iter()
            .find(|m| m["id"] == "claude-opus-4-8")
            .unwrap_or_else(|| panic!("{by_model:#?}"));
        let opus_claude = harness(opus, "claude");
        assert_eq!(
            opus_claude["steered"],
            serde_json::json!({"steered":1,"readable":2})
        );
        let opus_node = &opus["efforts"][0]["pipelines"][0]["nodes"][0];
        assert_eq!(opus_node["id"], "coder");
        assert_eq!(harness(opus_node, "claude")["steering"]["measured"], 2);
        let couples = node("coder")["models"].as_array().unwrap().clone();
        let opus_couple = couples
            .iter()
            .find(|c| c["model"] == "claude-opus-4-8")
            .unwrap();
        assert_eq!(
            harness(opus_couple, "claude")["steered"],
            serde_json::json!({"steered":1,"readable":2})
        );
        let sonnet = by_model
            .iter()
            .find(|m| m["id"] == "claude-sonnet-4-5")
            .unwrap();
        assert_eq!(
            harness(sonnet, "pi")["steered"],
            serde_json::json!({"steered":1,"readable":1})
        );
    }

    /// `GET /stats/performance` (#810, story #808) — the **durée active** and
    /// the **genre de nœud**, read at the HTTP seam over a forged event log.
    ///
    /// One Run exercises every wait rule at once: a `declared` wait lifted by a
    /// `NodeResumed`, a `completion_not_released` wait lifted by a
    /// `NodeCompletionReleased`, a `children_pending` marker that opens NOTHING
    /// (an orchestrator on `pdo run wait` is working), a `NodeAwaitingUser`
    /// with no cause at all (ignored — defensive, for pre-ADR-0069 history), a
    /// wait closed by the very terminal that records the observation, and the
    /// Infrastructure row subtracting the sum of its Nodes' waits. The same
    /// payloads carry the genre: frozen on `NodeStarted`, or read off the Run's
    /// pipeline snapshot when the startup event predates the freeze.
    #[tokio::test]
    async fn stats_performance_subtracts_declared_waits_and_carries_the_node_kind() {
        let state = test_state().await;
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        // `orch`'s snapshot says orchestrator; its `node_started` will NOT, so
        // the fold must fall back to the snapshot (pre-#810 history).
        insert_event(
            &state.db,
            "wait-r1",
            "2033-03-10T10:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"wp1","pipeline_name":"Wait Pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[
                    {"id":"chat","name":"chat","node_type":"agent","isolated_worktree":true},
                    {"id":"orch","name":"orch","node_type":"agent","isolated_worktree":true,"orchestrator":true},
                    {"id":"plain","name":"plain","node_type":"agent","isolated_worktree":true},
                    {"id":"late","name":"late","node_type":"agent","isolated_worktree":true}
                ]
            }),
        )
        .await;

        // `chat` — interactive, two waits of two different causes, both lifted.
        // 10 min wall-clock, 3 min waited → 7 min active.
        for (ts, kind, payload) in [
            (
                "2033-03-10T10:00:00Z",
                "node_started",
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","interactive":true,"orchestrator":false,"model":"claude-opus-5"}),
            ),
            (
                "2033-03-10T10:01:00Z",
                "node_awaiting_user",
                serde_json::json!({"cause":"declared","message":"Strip or card?"}),
            ),
            (
                "2033-03-10T10:03:00Z",
                "node_resumed",
                serde_json::json!({}),
            ),
            (
                "2033-03-10T10:04:00Z",
                "node_awaiting_user",
                serde_json::json!({"cause":"completion_not_released"}),
            ),
            (
                "2033-03-10T10:05:00Z",
                "node_completion_released",
                serde_json::json!({}),
            ),
            (
                "2033-03-10T10:10:00Z",
                "node_completed",
                serde_json::json!({}),
            ),
        ] {
            insert_event(
                &state.db,
                "wait-r1",
                ts,
                kind,
                Some("chat"),
                Some(1),
                payload,
            )
            .await;
        }

        // `orch` — the binding marker (`reason: children_pending`) opens no wait.
        // 6 min wall-clock, 6 min active. No `orchestrator` in the payload.
        for (ts, kind, payload) in [
            (
                "2033-03-10T10:00:00Z",
                "node_started",
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
            ),
            (
                "2033-03-10T10:02:00Z",
                "node_awaiting_user",
                serde_json::json!({"reason":"children_pending","active":["c1"],"failed":[]}),
            ),
            (
                "2033-03-10T10:06:00Z",
                "node_completed",
                serde_json::json!({}),
            ),
        ] {
            insert_event(
                &state.db,
                "wait-r1",
                ts,
                kind,
                Some("orch"),
                Some(1),
                payload,
            )
            .await;
        }

        // `plain` — a `NodeAwaitingUser` with no cause at all is ignored.
        // 4 min wall-clock, 4 min active.
        for (ts, kind, payload) in [
            (
                "2033-03-10T10:00:00Z",
                "node_started",
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
            ),
            (
                "2033-03-10T10:01:00Z",
                "node_awaiting_user",
                serde_json::json!({}),
            ),
            (
                "2033-03-10T10:04:00Z",
                "node_completed",
                serde_json::json!({}),
            ),
        ] {
            insert_event(
                &state.db,
                "wait-r1",
                ts,
                kind,
                Some("plain"),
                Some(1),
                payload,
            )
            .await;
        }

        // `late` — still awaiting when it completes: the terminal closes the
        // wait. 12 min wall-clock, 10 min waited → 2 min active.
        for (ts, kind, payload) in [
            (
                "2033-03-10T10:00:00Z",
                "node_started",
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
            ),
            (
                "2033-03-10T10:02:00Z",
                "node_awaiting_user",
                serde_json::json!({"cause":"declared"}),
            ),
            (
                "2033-03-10T10:12:00Z",
                "node_completed",
                serde_json::json!({}),
            ),
        ] {
            insert_event(
                &state.db,
                "wait-r1",
                ts,
                kind,
                Some("late"),
                Some(1),
                payload,
            )
            .await;
        }

        // Infrastructure: 20 min of Run, 3 + 10 min of declared wait → 7 min.
        insert_event(
            &state.db,
            "wait-r1",
            "2033-03-10T10:20:00Z",
            "run_completed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        // A second Run of the same pipeline that FAILS: its successful node is
        // an observation for the default cohort and vanishes under
        // `completed_only`.
        insert_event(
            &state.db,
            "wait-r2",
            "2033-03-10T11:00:00Z",
            "run_started",
            None,
            None,
            serde_json::json!({
                "pipeline_id":"wp2","pipeline_name":"Doomed Pipeline","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"doomed","name":"doomed","node_type":"agent","isolated_worktree":true}]
            }),
        )
        .await;
        insert_event(
            &state.db,
            "wait-r2",
            "2033-03-10T11:00:00Z",
            "node_started",
            Some("doomed"),
            Some(1),
            serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude"}),
        )
        .await;
        insert_event(
            &state.db,
            "wait-r2",
            "2033-03-10T11:05:00Z",
            "node_completed",
            Some("doomed"),
            Some(1),
            serde_json::json!({}),
        )
        .await;
        insert_event(
            &state.db,
            "wait-r2",
            "2033-03-10T11:10:00Z",
            "run_failed",
            None,
            None,
            serde_json::json!({}),
        )
        .await;

        async fn call(state: &Arc<AppState>, uri: &str) -> serde_json::Value {
            let response = build_router(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        fn node<'a>(
            body: &'a serde_json::Value,
            pipeline: &str,
            id: &str,
        ) -> &'a serde_json::Value {
            body["by_pipeline"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == pipeline)
                .unwrap_or_else(|| panic!("pipeline {pipeline} missing from {body}"))["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id)
                .unwrap_or_else(|| panic!("node {id} missing"))
        }

        fn claude_metric<'a>(entity: &'a serde_json::Value, metric: &str) -> &'a serde_json::Value {
            &entity["harnesses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h["harness"] == "claude")
                .unwrap()[metric]
        }

        let from = "2033-03-10T00:00:00Z";
        let to = "2033-03-11T00:00:00Z";
        let body = call(&state, &format!("/stats/performance?from={from}&to={to}")).await;

        const MIN: f64 = 60_000.0;
        for (id, wall, active, waiting) in [
            ("chat", 10.0, 7.0, 3.0),
            ("orch", 6.0, 6.0, 0.0),
            ("plain", 4.0, 4.0, 0.0),
            ("late", 12.0, 2.0, 10.0),
        ] {
            let row = node(&body, "wp1", id);
            assert_eq!(
                claude_metric(row, "duration")["stats"]["median"],
                wall * MIN,
                "{id} wall-clock"
            );
            assert_eq!(
                claude_metric(row, "active_duration")["stats"]["median"],
                active * MIN,
                "{id} active duration"
            );
            // #819 — the third reading: the declared wait alone. A node nobody
            // waited on measures a real `0`, never an absence.
            assert_eq!(
                claude_metric(row, "wait_duration")["stats"]["median"],
                waiting * MIN,
                "{id} waiting"
            );
            assert_eq!(
                active * MIN + waiting * MIN,
                wall * MIN,
                "{id}: Total = Active + Waiting"
            );
            // Coverage never disagrees between the three readings: the mode
            // switch must not change `n=`.
            for metric in ["active_duration", "wait_duration"] {
                assert_eq!(
                    claude_metric(row, "duration")["measured"],
                    claude_metric(row, metric)["measured"],
                    "{id} {metric} coverage"
                );
            }
        }

        // #819 — the wait coverage the « i » reads: the five Node executions of
        // the default cohort (four here plus the failed Run's `doomed`), two of
        // which declared a wait. Infrastructure is not an execution.
        assert_eq!(body["executions"], 5);
        assert_eq!(body["waited_executions"], 2);

        // The genre travels on every Node row: frozen in the payload for
        // `chat`, read off the Run's snapshot for `orch`.
        assert_eq!(node(&body, "wp1", "chat")["interactive"], true);
        assert_eq!(node(&body, "wp1", "chat")["orchestrator"], false);
        assert_eq!(node(&body, "wp1", "orch")["orchestrator"], true);
        assert_eq!(node(&body, "wp1", "orch")["interactive"], false);
        assert_eq!(node(&body, "wp1", "plain")["interactive"], false);
        assert_eq!(node(&body, "wp1", "plain")["orchestrator"], false);
        // A Pipeline row is not a Node: it carries no kind at all.
        assert!(
            body["by_pipeline"][0].get("interactive").is_none(),
            "a Pipeline row carries no kind: {}",
            body["by_pipeline"][0]
        );

        // The « By model » axis carries the same genre on its Node leaves.
        let model_node = &body["by_model"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "claude-opus-5")
            .unwrap()["efforts"][0]["pipelines"][0]["nodes"][0];
        assert_eq!(model_node["id"], "chat");
        assert_eq!(model_node["interactive"], true);
        assert_eq!(
            claude_metric(model_node, "active_duration")["stats"]["median"],
            7.0 * MIN
        );

        // Infrastructure: the Run's own span minus the sum of its Nodes' waits.
        let manager = body["infrastructure"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "pipeline-manager")
            .unwrap();
        assert_eq!(
            claude_metric(manager, "duration")["stats"]["median"],
            20.0 * MIN
        );
        assert_eq!(
            claude_metric(manager, "active_duration")["stats"]["median"],
            7.0 * MIN
        );
        // #819 — and its Waiting is that same sum, so the three modes reconcile
        // at the Infrastructure level too.
        assert_eq!(
            claude_metric(manager, "wait_duration")["stats"]["median"],
            13.0 * MIN
        );
        assert!(
            body["by_pipeline"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "wp2"),
            "the failed Run's successful execution counts in the default cohort"
        );

        // --- « Runs terminés seulement » --------------------------------------
        let before = stats_performance::recompute_count_for_test(from, to);
        let narrowed = call(
            &state,
            &format!("/stats/performance?from={from}&to={to}&completed_only=true"),
        )
        .await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            before + 1,
            "the cohort switch is part of the memo key: it must not serve the other cohort"
        );
        assert!(
            !narrowed["by_pipeline"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "wp2"),
            "a Run without a run_completed leaves the cohort whole: {narrowed}"
        );
        assert_eq!(
            claude_metric(node(&narrowed, "wp1", "chat"), "active_duration")["stats"]["median"],
            7.0 * MIN,
            "the completed Run is untouched by the cohort switch"
        );
        // The coverage follows the cohort it describes: `doomed` left with its
        // Run, so the denominator drops to the four executions on screen.
        assert_eq!(narrowed["executions"], 4);
        assert_eq!(narrowed["waited_executions"], 2);
        // A repeat of the narrowed request is served from ITS memo entry.
        let repeat = call(
            &state,
            &format!("/stats/performance?from={from}&to={to}&completed_only=true"),
        )
        .await;
        assert_eq!(
            stats_performance::recompute_count_for_test(from, to),
            before + 1,
            "each cohort keeps its own memo entry"
        );
        assert_eq!(narrowed, repeat);
    }

    /// `GET /stats/overview` and `GET /stats/cost` (#810) — the SAME cohort
    /// switch, so the three sections never describe different Runs. Overview
    /// keeps its Errors series and shows **zero**, rather than hiding the card.
    #[tokio::test]
    async fn stats_overview_and_cost_share_the_completed_only_cohort() {
        let state = test_state().await;
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn insert_event(
            db: &sqlx::SqlitePool,
            run: &str,
            ts: &str,
            kind: &str,
            node: Option<&str>,
            iter: Option<i64>,
            payload: serde_json::Value,
        ) {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(run)
            .bind(ts)
            .bind(kind)
            .bind(node)
            .bind(iter)
            .bind(payload.to_string())
            .execute(db)
            .await
            .unwrap();
        }

        for (run, pipeline, terminal) in [
            ("cohort-done", "cdone", "run_completed"),
            ("cohort-failed", "cfail", "run_failed"),
        ] {
            insert_event(
                &state.db,
                run,
                "2033-04-02T09:00:00Z",
                "run_started",
                None,
                None,
                serde_json::json!({
                    "pipeline_id":pipeline,"pipeline_name":pipeline,"target_repo":repo,"harness":"claude",
                    "node_defs":[{"id":"worker","name":"Worker","node_type":"agent","isolated_worktree":true}]
                }),
            )
            .await;
            insert_event(
                &state.db,
                run,
                "2033-04-02T09:05:00Z",
                "node_started",
                Some("worker"),
                Some(1),
                serde_json::json!({"node_type":"agent","isolated_worktree":true,"harness":"claude","session_id":run}),
            )
            .await;
            insert_event(
                &state.db,
                run,
                "2033-04-02T09:30:00Z",
                terminal,
                None,
                None,
                serde_json::json!({}),
            )
            .await;
        }

        async fn call(state: &Arc<AppState>, uri: &str) -> serde_json::Value {
            let response = build_router(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap()
        }

        fn total(series: &serde_json::Value) -> i64 {
            series
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["count"].as_i64().unwrap())
                .sum()
        }

        let window = "from=2033-04-02T00:00:00Z&to=2033-04-03T00:00:00Z&bucket=day";
        let whole = call(&state, &format!("/stats/overview?{window}")).await;
        assert_eq!(total(&whole["runs"]), 2);
        assert_eq!(total(&whole["errors"]), 1);
        assert_eq!(total(&whole["sessions"]), 2);

        let narrowed = call(
            &state,
            &format!("/stats/overview?{window}&completed_only=true"),
        )
        .await;
        assert_eq!(total(&narrowed["runs"]), 1);
        assert_eq!(
            total(&narrowed["errors"]),
            0,
            "zero errors, card kept — nothing is masked: {narrowed}"
        );
        assert_eq!(total(&narrowed["sessions"]), 1);
        assert_eq!(
            narrowed["sessions_by_pipeline"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["cdone"],
        );

        let cost = call(&state, &format!("/stats/cost?{window}")).await;
        assert_eq!(cost["by_pipeline"].as_array().unwrap().len(), 2);
        let narrowed_cost =
            call(&state, &format!("/stats/cost?{window}&completed_only=true")).await;
        assert_eq!(
            narrowed_cost["by_pipeline"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["cdone"],
        );
    }

    #[tokio::test]
    async fn stats_performance_empty_cohort_is_not_an_error() {
        let state = test_state().await;
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-01-01T00:00:00Z&to=2026-01-02T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["harnesses"], serde_json::json!([]));
        assert_eq!(body["by_pipeline"], serde_json::json!([]));
        assert_eq!(body["infrastructure"], serde_json::json!([]));
        assert_eq!(body["total"]["harnesses"], serde_json::json!([]));
    }

    /// A harness's whole transcript source existing but unreadable (permissions,
    /// a corrupted mount) is a visible endpoint error — distinct from a single
    /// unreadable session, which just degrades to a local absence reason.
    #[tokio::test]
    async fn stats_performance_source_wide_unreadable_is_a_visible_error() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(
                    &self.0,
                    std::os::unix::fs::PermissionsExt::from_mode(0o755),
                );
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let mut state = test_state().await;
        let home = state
            .repo_root
            .join(".pdo")
            .join("test-stats-performance-unreadable")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home.clone());
        let repo = state.repo_root.to_string_lossy().into_owned();

        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-unreadable', '2026-07-15T09:00:00Z', 'run_started', NULL, NULL, ?)",
        )
        .bind(
            serde_json::json!({
                "pipeline_id":"p1","pipeline_name":"Loop","target_repo":repo,"harness":"claude",
                "node_defs":[{"id":"worker","name":"Worker","node_type":"agent", "isolated_worktree":true}]
            })
            .to_string(),
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-unreadable', '2026-07-15T09:10:00Z', 'node_started', 'worker', 1, ?)",
        )
        .bind(
            serde_json::json!({"node_type":"agent", "isolated_worktree":true,"harness":"claude","session_id":"claude-1"})
                .to_string(),
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES \
             ('perf-unreadable', '2026-07-15T09:20:00Z', 'node_completed', 'worker', 1, '{}')",
        )
        .execute(&state.db)
        .await
        .unwrap();

        let claude_root = home.join(".claude").join("projects");
        std::fs::create_dir_all(&claude_root).unwrap();
        std::fs::set_permissions(
            &claude_root,
            std::os::unix::fs::PermissionsExt::from_mode(0o000),
        )
        .unwrap();
        let _cleanup = Cleanup(claude_root.clone());

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/stats/performance?from=2026-07-15T00:00:00Z&to=2026-07-16T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let message = body["error"].as_str().unwrap_or_default();
        assert!(
            message.contains("Claude"),
            "the error must name the harness/source: {message}"
        );
    }

    /// Like [`test_state`], but with an injected [`ServiceHealth`] so the
    /// `/sessions` `service` field can be asserted without real systemd (#156, D5).
    async fn test_state_with_service_health(service_health: ServiceHealth) -> Arc<AppState> {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (bus_tx, _) = broadcast::channel(80);
        let workspace = workspace_context(
            &db,
            &bus_tx,
            &std::env::current_dir().unwrap(),
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        );
        let skills = skill_context(
            &db,
            &std::env::current_dir().unwrap(),
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        );
        let platform = test_platform(&db, Some(service_health)).await;
        let test_repo_root: PathBuf = std::env::current_dir().unwrap();
        let test_port: u16 = next_test_daemon_port();
        Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
            let session =
                contexts::test_session(weak.clone(), test_repo_root.clone(), test_port, None);
            let run = contexts::test_run(contexts::TestRun {
                state: weak.clone(),
                db: &db,
                bus_tx: &bus_tx,
                session: &session,
                workspace: workspace.clone(),
                skills: skills.clone(),
                repo_root: test_repo_root.clone(),
                port: test_port,
                docker_cmd_override: None,
                sandbox_home_override: None,
            });
            AppState {
                workspace,
                skills,
                platform,
                db,
                run,
                bus_tx,
                repo_root: test_repo_root.clone(),
                recent_writes: SelfWrites::new(),
                docker_cmd_override: None,
                sandbox_home_override: None,
                docker_probe_cache: Arc::new(tokio::sync::Mutex::new(None)),
                harness_catalogue: Arc::new(
                    pdo_harness::adapters::binary_probe::installed_catalogue_reader(),
                ),
                price_sync_lock: tokio::sync::Mutex::new(()),
                price_source_url: None,
                price_refresh_at_boot: false,
                allowed_ws_origins: Vec::new(),
                session,
            }
        })
    }

    fn legacy_app() -> Router {
        // Backwards-compatible app without state for static-serving tests
        Router::new()
            .route(
                "/ws",
                get(|ws: axum::extract::WebSocketUpgrade| async { ws.on_upgrade(|_| async {}) }),
            )
            .fallback(static_handler)
    }

    #[tokio::test]
    async fn root_returns_html() {
        let resp = legacy_app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("<!DOCTYPE html>") || text.contains("<!doctype html>"));
    }

    #[tokio::test]
    async fn unknown_path_falls_back_to_index() {
        let resp = legacy_app()
            .oneshot(
                Request::builder()
                    .uri("/some/client/route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// DOCTRINE INVARIANT (ADR-0012(a)): the reaper sweep kills tmux sessions
    /// but MUST NEVER delete a run's worktree or branches, nor archive the run.
    /// This pins the verdict of the #128 grilling — a future change that makes
    /// the runtime auto-delete will turn this test red.
    #[tokio::test]
    async fn reaper_never_deletes_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "doctrine-completed";
        seed_run_with_run_events(&state, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
        mk_run_worktree_dir(tmp.path(), run_id);
        let worktree = tmp
            .path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree");
        assert!(worktree.exists(), "precondition: worktree on disk");

        // Run the reaper with maximum aggression (ttl=0) against the test daemon's
        // tmux socket, where no server runs (no sessions to kill — we only care that
        // the sweep leaves the disk and run status untouched).
        state
            .session
            .sweep()
            .run_with(pdo_session::domain::reaper::SweepTtls {
                node_run: std::time::Duration::ZERO,
                libassist_idle: std::time::Duration::ZERO,
            })
            .await
            .unwrap();

        assert!(
            worktree.exists(),
            "DOCTRINE REGRESSION: reaper deleted the worktree"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.status,
            event_log::RunStatus::Completed,
            "DOCTRINE REGRESSION: reaper archived the run on its own"
        );
    }

    #[tokio::test]
    async fn sessions_endpoint_reports_live_count_and_cap() {
        // The status-bar counter reads `GET /sessions`: the live
        // NodeRun-session count and the configured cap. A single running node
        // counts as one live session.
        let state = test_state().await;
        let run_id = "sessions-run";
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
        )
        .await
        .unwrap();
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
        )
        .await
        .unwrap();

        let status = state.session.read_status().execute().await;
        assert_eq!(status.live, 1, "one running node = one live session");
        assert!(status.cap >= 1, "cap should be a positive integer");
        // #485 / ADR-0038: the orphan-sweep gauge rides the same payload. This
        // daemon has never swept, so the timestamp is null and both counters are
        // zero (the moving parts are pinned by
        // `tests/tmux_lifecycle.rs::genuine_orphans_are_still_reaped_and_counted`,
        // the payload's version and shape by `tests/snapshots_temporary`).
        assert_eq!(status.last_sweep_at, None);
        assert_eq!((status.killed, status.killed_for_absent_run), (0, 0));
    }

    #[tokio::test]
    async fn sessions_endpoint_reports_cached_service_health() {
        // #156 (D5): `GET /sessions` carries the boot-cached `service` object.
        // Here it is ephemeral (reachable but not installed as a unit) — the
        // exact state that drives the status bar's amber `ephemeral` pill.
        let state = test_state_with_service_health(ServiceHealth {
            supervisor: "systemd".to_string(),
            persistent: Some(false),
        })
        .await;
        let json = state.session.read_status().execute().await;
        assert_eq!(
            json.service["supervisor"], "systemd",
            "the service supervisor hint is surfaced verbatim from AppState"
        );
        assert_eq!(
            json.service["persistent"], false,
            "persistent:false is the reachable-but-ephemeral signal the pill consumes"
        );
    }

    #[tokio::test]
    async fn sessions_service_persistent_is_null_when_unknown() {
        // The neutral default (non-Linux / no systemd / spawn failure) serializes
        // `persistent: null` — the UI silences on null exactly as it does on true.
        let state = test_state_with_service_health(ServiceHealth {
            supervisor: "none".to_string(),
            persistent: None,
        })
        .await;
        let json = state.session.read_status().execute().await;
        assert!(
            json.service["persistent"].is_null(),
            "unknown persistence must serialize as null, not an error or false"
        );
    }

    #[tokio::test]
    async fn sessions_endpoint_excludes_session_holding_node_of_a_terminal_run() {
        // #215 repro at the HTTP boundary: a run fails (fail-fast) while a
        // sibling node is still projected Running. `GET /sessions` must report
        // `live: 0`, not leak a phantom slot. This is the exact symptom the
        // issue filed (`/sessions` returned `live:1` with no live tmux).
        let state = test_state().await;
        let run_id = "sessions-terminal-run";
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": "fail-fast" })),
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let status = state.session.read_status().execute().await;
        assert_eq!(
            status.live, 0,
            "a session-holding node in a terminal run must not count (#215)"
        );
    }

    #[tokio::test]
    async fn send_review_comments_refuses_when_the_run_branch_is_gone() {
        // #750: the seeded run never had a `pdo/run-<id>` branch in this
        // checkout (nor an archived Run has one) — sending answers 409
        // `run_branch_gone` with the reason the UI prints on its disabled
        // buttons, and NO event is appended: drafts stay drafts.
        let state = test_state().await;
        let run_id = "review-send-branch-gone";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/review/comments/send"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "comments": [{ "path": "src/a.rs", "side": "new", "line": 3, "text": "why?" }]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["error"], "run_branch_gone");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains(&format!("pdo/run-{run_id} no longer exists")),
            "got {body}"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(
            !events
                .iter()
                .any(|e| e.kind == event_log::EventKind::ReviewCommentSent),
            "no comment recorded on a refused send"
        );
    }

    #[tokio::test]
    async fn send_review_comments_validates_the_batch_and_lists_nothing_by_default() {
        // #750: an empty batch and an unknown run are refused before any git or
        // tmux work; the list endpoint of a never-reviewed Run is `[]`.
        let state = test_state().await;
        let run_id = "review-send-validation";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state.clone());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/review/comments/send"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "comments": [] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/no-such-run/review/comments/send")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "comments": [{ "path": "a", "side": "new", "line": 1, "text": "t" }] })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/runs/{run_id}/review/comments"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["comments"], serde_json::json!([]));
    }

    /// Append one `review_comment_sent` straight into the log — the send endpoint
    /// needs git + tmux, which these handler tests do not.
    async fn seed_sent_comment(state: &Arc<AppState>, run_id: &str, id: &str) {
        let c = review_comments::ReviewComment {
            id: id.into(),
            path: "lib.rs".into(),
            side: review_comments::ReviewSide::New,
            line: 4,
            from_ref: "fork".into(),
            to_ref: "tip".into(),
            from_sha: None,
            to_sha: None,
            text: "Name this `fn delta`.".into(),
            excerpt: String::new(),
            author: "user".into(),
            sent_at: pdo_kernel::clock::now_iso(),
            batch_id: Some("b1".into()),
            status: review_comments::ReviewCommentStatus::Sent,
            replies: vec![],
            proposal_pending: false,
            resolved_by: None,
            resolved_at: None,
            reopened_by: None,
            reopened_at: None,
            proposal_declined: false,
        };
        append_event(
            state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::ReviewCommentSent,
                node_id: None,
                iter: None,
                payload: Some(serde_json::to_value(&c).unwrap()),
            },
        )
        .await
        .unwrap();
    }

    async fn review_post(
        app: &axum::Router,
        uri: &str,
        body: serde_json::Value,
        session: Option<(&str, &str)>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some((run, node)) = session {
            req = req
                .header(SESSION_RUN_HEADER, run)
                .header(SESSION_NODE_HEADER, node);
        }
        let resp = app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    async fn review_get(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn review_reply_from_the_manager_session_is_a_proposal_the_human_resolves_or_reopens() {
        // #751, ADR-0067 §4 — setting off (default): `resolved: true` PROPOSES.
        // Author from the session headers (`__manager__` ⇒ `manager`); every
        // step is an event; resolve / reopen are the human's verbs; a reopen on a
        // proposal declines it and keeps the comment open.
        let state = test_state().await;
        let run_id = "review-reply-proposal";
        seed_completed_run(&state, run_id).await;
        seed_sent_comment(&state, run_id, "rc-001").await;
        let app = build_router(state.clone());
        let base = format!("/runs/{run_id}/review/comments");

        // Plain reply from a node session: author = node id, still open.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reply"),
            serde_json::json!({ "text": "Looking into it." }),
            Some((run_id, "xuTJYLUa")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "replied");
        assert_eq!(body["author"], "xuTJYLUa");
        assert_eq!(body["comment"]["replies"][0]["author"], "xuTJYLUa");
        assert!(body["comment"].get("proposal_pending").is_none());

        // `--resolved` from the manager session: a proposal.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reply"),
            serde_json::json!({ "text": "Renamed in 7f2b0d1.", "resolved": true }),
            Some((run_id, review_comments::MANAGER_NODE_ID)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "proposed");
        assert_eq!(body["author"], "manager");
        assert_eq!(body["comment"]["status"], "sent");
        assert_eq!(body["comment"]["proposal_pending"], true);
        assert_eq!(body["comment"]["replies"][1]["proposes_resolution"], true);

        // The list filters by state and says what `--resolved` does.
        let (status, list) = review_get(&app, &format!("{base}?state=open")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["comments"].as_array().unwrap().len(), 1);
        assert_eq!(list["agent_can_resolve"], false);
        let (_, none) = review_get(&app, &format!("{base}?state=resolved")).await;
        assert_eq!(none["comments"], serde_json::json!([]));
        let (status, _) = review_get(&app, &format!("{base}?state=nope")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Reopen on the proposal = declined, still open, recorded.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reopen"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["changed"], true);
        assert_eq!(body["comment"]["status"], "sent");
        assert!(body["comment"].get("proposal_pending").is_none());
        assert_eq!(body["comment"]["proposal_declined"], true);
        assert_eq!(body["comment"]["reopened_by"], "user");
        // Nothing pending any more: a second reopen changes nothing, no event.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reopen"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["changed"], false);

        // Resolve by the human, then reopen: back to sent, resolver cleared.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/resolve"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["comment"]["status"], "resolved");
        assert_eq!(body["comment"]["resolved_by"], "user");
        let (_, again) = review_post(
            &app,
            &format!("{base}/rc-001/resolve"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(again["changed"], false, "already resolved is not an error");
        let (_, list) = review_get(&app, &format!("{base}?state=resolved")).await;
        assert_eq!(list["comments"].as_array().unwrap().len(), 1);
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reopen"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["comment"]["status"], "sent");
        assert!(body["comment"].get("resolved_by").is_none());
        assert!(body["comment"].get("proposal_declined").is_none());

        // Refusals: unknown id, empty text.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-999/reply"),
            serde_json::json!({ "text": "x" }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("rc-999"));
        let (status, _) = review_post(
            &app,
            &format!("{base}/rc-001/reply"),
            serde_json::json!({ "text": "   " }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Everything above is in the log, immutable and timestamped.
        let events = load_events(&state.db, run_id).await.unwrap();
        let kinds: Vec<String> = events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    event_log::EventKind::ReviewCommentReplied
                        | event_log::EventKind::ReviewCommentResolved
                        | event_log::EventKind::ReviewCommentReopened
                )
            })
            .map(|e| format!("{:?}", e.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                "ReviewCommentReplied",
                "ReviewCommentReplied",
                "ReviewCommentReopened",
                "ReviewCommentResolved",
                "ReviewCommentReopened"
            ]
        );
    }

    #[tokio::test]
    async fn review_reply_resolves_directly_when_the_instance_setting_is_on() {
        // #751 AC: setting on ⇒ `--resolved` resolves on the spot, by the agent; the
        // human keeps Reopen. Without session headers the declared author is taken.
        let state = test_state().await;
        instance_config::update(
            &state.db,
            instance_config::UpdateInstanceConfig {
                review_agent_can_resolve: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let run_id = "review-reply-direct";
        seed_completed_run(&state, run_id).await;
        seed_sent_comment(&state, run_id, "rc-001").await;
        let app = build_router(state.clone());
        let base = format!("/runs/{run_id}/review/comments");

        let (_, list) = review_get(&app, &base).await;
        assert_eq!(list["agent_can_resolve"], true);

        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reply"),
            serde_json::json!({ "text": "Fixed.", "resolved": true, "author": "fixer" }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "resolved");
        assert_eq!(body["author"], "fixer");
        assert_eq!(body["comment"]["status"], "resolved");
        assert_eq!(body["comment"]["resolved_by"], "fixer");
        assert!(body["comment"].get("proposal_pending").is_none());

        // A reply on a resolved comment is just a reply, `resolved` or not.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reply"),
            serde_json::json!({ "text": "Also added a test.", "resolved": true }),
            Some((run_id, review_comments::MANAGER_NODE_ID)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "replied");
        assert_eq!(body["comment"]["replies"].as_array().unwrap().len(), 2);
        assert!(body["comment"]["replies"][1]
            .get("proposes_resolution")
            .is_none());

        // The human keeps the last word.
        let (status, body) = review_post(
            &app,
            &format!("{base}/rc-001/reopen"),
            serde_json::json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["comment"]["status"], "sent");
        assert_eq!(body["comment"]["reopened_by"], "user");
    }

    async fn seed_started_node(state: &Arc<AppState>, run_id: &str, interactive: bool) {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "declared",
                    "node_defs": [
                        { "id": "griller", "node_type": "agent", "inputs": [], "outputs": [] },
                        { "id": "runner", "node_type": "script", "inputs": [], "outputs": [] },
                        { "id": "later", "node_type": "agent", "inputs": [], "outputs": [] }
                    ]
                })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({ "interactive": interactive })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("runner".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(state, &event).await.unwrap();
        }
    }

    /// Declare the wait through the session context's use case, over this
    /// crate's Run ports — what `POST …/wait-user` runs.
    async fn post_wait_user(
        state: &Arc<AppState>,
        run_id: &str,
        node_id: &str,
        body: serde_json::Value,
    ) -> Result<
        pdo_session::domain::declared_wait::DeclareOutcome,
        pdo_session::application::declared_wait::DeclareWaitError,
    > {
        state
            .session
            .declare_wait()
            .execute(
                &pdo_kernel::Actor::LocalOperator,
                pdo_session::application::declared_wait::DeclareWaitCommand {
                    run_id: run_id.into(),
                    node_id: node_id.into(),
                    iter: body["iter"].as_i64().unwrap_or(1),
                    message: body["message"].as_str().map(str::to_string),
                },
            )
            .await
    }

    /// The bytes a browser typed into `session`, as the PTY bridge hands them over.
    async fn note_pty_input(state: &Arc<AppState>, session: &str, bytes: &[u8]) {
        state
            .session
            .terminals()
            .note_input(&pdo_kernel::Actor::LocalOperator, session, bytes)
            .await;
    }

    fn wait_refusal(
        result: Result<
            pdo_session::domain::declared_wait::DeclareOutcome,
            pdo_session::application::declared_wait::DeclareWaitError,
        >,
    ) -> &'static str {
        match result {
            Err(pdo_session::application::declared_wait::DeclareWaitError::Wait(r)) => r.slug(),
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }

    async fn projected(state: &Arc<AppState>, run_id: &str) -> event_log::RunState {
        event_log::project(&load_events(&state.db, run_id).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn wait_user_declares_the_wait_with_the_message_as_run_reason_and_no_code() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "declared-wait-message";
        seed_started_node(&state, run_id, true).await;
        // A fresh interactive node is NOT awaiting (ADR-0069: nothing at spawn).
        let before = projected(&state, run_id).await;
        assert_eq!(before.status, event_log::RunStatus::Running);
        assert_eq!(
            before.nodes["griller"].status,
            event_log::NodeStatus::Running
        );

        let outcome = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Which layout: strip or card?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Declared)
        );

        let after = projected(&state, run_id).await;
        assert_eq!(after.status, event_log::RunStatus::AwaitingUser);
        assert_eq!(
            after.awaiting_reason.as_deref(),
            Some("Which layout: strip or card?")
        );
        assert!(
            after.awaiting_reason_code.is_none(),
            "a declared wait has no code"
        );
        assert!(after.is_declared_wait());
        let node = &after.nodes["griller"];
        assert_eq!(node.status, event_log::NodeStatus::AwaitingUser);
        let info = node.awaiting.as_ref().expect("awaiting info");
        assert_eq!(info.cause, event_log::AWAITING_CAUSE_DECLARED);
        assert_eq!(
            info.message.as_deref(),
            Some("Which layout: strip or card?")
        );

        // Same message again: a legal no-op, nothing appended.
        let events_before = load_events(&state.db, run_id).await.unwrap().len();
        let outcome = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Which layout: strip or card?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Noop)
        );
        assert_eq!(
            load_events(&state.db, run_id).await.unwrap().len(),
            events_before
        );

        // A new message refreshes the banner.
        let outcome = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "Dark theme too?" }),
        )
        .await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Refreshed)
        );
        let refreshed = projected(&state, run_id).await;
        assert_eq!(
            refreshed.awaiting_reason.as_deref(),
            Some("Dark theme too?")
        );

        // A message over the cap is truncated by the daemon too.
        let long = "z".repeat(300);
        let _ = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": long }),
        )
        .await;
        let capped = projected(&state, run_id).await;
        assert_eq!(
            capped.awaiting_reason.as_deref().unwrap().chars().count(),
            pdo_session::domain::declared_wait::MESSAGE_MAX_CHARS
        );
    }

    #[tokio::test]
    async fn wait_user_bare_is_accepted_on_a_non_interactive_node_and_refused_by_name() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "declared-wait-refusals";
        seed_started_node(&state, run_id, false).await;

        // Bare, on a plain (non-interactive) agent node: accepted, no message.
        let outcome =
            post_wait_user(&state, run_id, "griller", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(
            outcome,
            Ok(pdo_session::domain::declared_wait::DeclareOutcome::Declared)
        );
        let rs = projected(&state, run_id).await;
        assert_eq!(rs.status, event_log::RunStatus::AwaitingUser);
        assert!(rs.awaiting_reason.is_none(), "no message ⇒ no prose reason");
        assert!(rs.awaiting_reason_code.is_none());
        assert!(rs.nodes["griller"]
            .awaiting
            .as_ref()
            .unwrap()
            .message
            .is_none());

        // A script node runs no agent: named refusal, still the caller's turn.
        let refusal =
            post_wait_user(&state, run_id, "runner", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_is_script");

        // A defined node that never spawned has no session.
        let refusal =
            post_wait_user(&state, run_id, "later", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_session_not_live");

        // An unknown node.
        let refusal =
            post_wait_user(&state, run_id, "ghost", serde_json::json!({ "iter": 1 })).await;
        assert_eq!(wait_refusal(refusal), "node_not_found");
    }

    #[tokio::test]
    async fn a_human_enter_after_typed_bytes_lifts_the_wait_but_nothing_else_does() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "20260912-100000-a11ce01";
        seed_started_node(&state, run_id, true).await;
        let _ = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "q?" }),
        )
        .await;
        let session = pdo_session::domain::names::node_session_name(run_id, "griller", 1);

        // An Enter on an empty line (a click to focus the pane): nothing.
        note_pty_input(&state, &session, b"\r").await;
        assert_eq!(
            projected(&state, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
        // Arrows, scroll, mouse: nothing, and they do not arm the rule either.
        note_pty_input(&state, &session, b"\x1b[A\x1b[B\x1b[<65;3;4M").await;
        note_pty_input(&state, &session, b"\r").await;
        assert_eq!(
            projected(&state, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
        // Typed bytes, then Enter: the wait lifts, node and run back to running.
        note_pty_input(&state, &session, b"the strip").await;
        note_pty_input(&state, &session, b"\r").await;
        let lifted = projected(&state, run_id).await;
        assert_eq!(lifted.status, event_log::RunStatus::Running);
        assert_eq!(
            lifted.nodes["griller"].status,
            event_log::NodeStatus::Running
        );
        assert!(lifted.nodes["griller"].awaiting.is_none());
        assert!(lifted.awaiting_reason.is_none());
        assert!(load_events(&state.db, run_id)
            .await
            .unwrap()
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeResumed));

        // The manager / shell sessions never lift anything.
        let _ = post_wait_user(&state, run_id, "griller", serde_json::json!({ "iter": 1 })).await;
        note_pty_input(
            &state,
            &pdo_session::domain::names::manager_session_name(run_id),
            b"hello\r",
        )
        .await;
        assert_eq!(
            projected(&state, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );

        // Typed bytes BEFORE a re-declaration do not count: the rule restarts.
        note_pty_input(&state, &session, b"typed before").await;
        let _ = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "another?" }),
        )
        .await;
        note_pty_input(&state, &session, b"\r").await;
        assert_eq!(
            projected(&state, run_id).await.status,
            event_log::RunStatus::AwaitingUser
        );
    }

    #[tokio::test]
    async fn a_refused_unreleased_completion_declares_the_wait_and_release_lifts_it() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "declared-wait-refused-completion";
        seed_started_node(&state, run_id, true).await;

        let attempt = complete_node_iteration(
            &state.run,
            run_id.into(),
            "griller".into(),
            1,
            CompletionSource::Explicit,
        )
        .await;
        assert!(matches!(
            attempt,
            CompletionAttempt::Refused {
                refusal: completion_refusal::CompletionRefusal::CompletionNotReleased
            }
        ));
        let rs = projected(&state, run_id).await;
        assert_eq!(rs.status, event_log::RunStatus::AwaitingUser);
        let info = rs.nodes["griller"]
            .awaiting
            .as_ref()
            .expect("declared by the daemon");
        assert_eq!(
            info.cause,
            event_log::AWAITING_CAUSE_COMPLETION_NOT_RELEASED
        );
        assert_eq!(
            rs.awaiting_reason.as_deref(),
            Some(pdo_session::domain::declared_wait::COMPLETION_NOT_RELEASED_MESSAGE)
        );
        assert!(rs.awaiting_reason_code.is_none());

        // A second refusal does not stack a second marker.
        let n = load_events(&state.db, run_id).await.unwrap().len();
        complete_node_iteration(
            &state.run,
            run_id.into(),
            "griller".into(),
            1,
            CompletionSource::StopHook,
        )
        .await;
        assert_eq!(load_events(&state.db, run_id).await.unwrap().len(), n);

        // The release (ADR-0068) brings the node back to running — unchanged.
        let response = release_node_completion(&state.run, run_id, "griller", 1).await;
        assert_eq!(response.status(), StatusCode::OK);
        let released = projected(&state, run_id).await;
        assert_eq!(released.status, event_log::RunStatus::Running);
        assert_eq!(
            released.nodes["griller"].status,
            event_log::NodeStatus::Running
        );
        assert!(released.nodes["griller"].awaiting.is_none());
        assert!(released.nodes["griller"].iterations[0].completion_released);
    }

    #[tokio::test]
    async fn a_children_pending_marker_is_not_lifted_by_enter() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "20260912-100000-b1nd1e2";
        seed_started_node(&state, run_id, false).await;
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeAwaitingUser,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({
                    "reason": run_advance::BINDING_MARKER_REASON, "active": 1, "failed": 0
                })),
            },
        )
        .await
        .unwrap();
        let rs = projected(&state, run_id).await;
        assert_eq!(
            rs.nodes["griller"].awaiting.as_ref().unwrap().cause,
            run_advance::BINDING_MARKER_REASON
        );
        assert!(rs.awaiting_reason.is_none(), "the marker carries no prose");
        let session = pdo_session::domain::names::node_session_name(run_id, "griller", 1);
        note_pty_input(&state, &session, b"hi\r").await;
        assert_eq!(
            projected(&state, run_id).await.nodes["griller"].status,
            event_log::NodeStatus::AwaitingUser
        );
    }

    #[tokio::test]
    async fn a_declared_wait_is_not_an_incident_for_the_reopen_embedding() {
        let state = test_state_with_alive_sessions().await;
        let run_id = "declared-wait-no-reopen";
        seed_started_node(&state, run_id, true).await;
        let _ = post_wait_user(
            &state,
            run_id,
            "griller",
            serde_json::json!({ "iter": 1, "message": "q?" }),
        )
        .await;
        let rs = projected(&state, run_id).await;
        let n = load_events(&state.db, run_id).await.unwrap().len();
        let same = embed_reopen_for_targeted_command(&state.run, run_id, rs)
            .await
            .unwrap();
        assert_eq!(same.status, event_log::RunStatus::AwaitingUser);
        assert_eq!(
            load_events(&state.db, run_id).await.unwrap().len(),
            n,
            "no reopen command was appended for a declared wait"
        );
    }

    // Node-completion tail convergence: the *observable* behaviour `complete_node`
    // must preserve across all three entrypoints (`node_done`, the `mark_node_done`
    // command arm, `handle_merge_resolver_done`).
    //
    // The headline invariant is exactly-one `RunCompleted` per run: `append_event`
    // does NOT de-dup it, so a regression that double-routes completion surfaces here.
    // Each test runs in the gate-only harness (no pipeline file), so
    // `handle_node_completion` / `advance_run` no-op and the shared completion gate is
    // exercised in isolation.

    fn init_test_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };
        run(&["init"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-m", "initial"]);
    }

    async fn test_state_with_dir(dir: &std::path::Path) -> Arc<AppState> {
        test_state_with_dir_and_docker(dir, None).await
    }

    /// Like [`test_state_with_dir`] but with an explicit `docker_cmd_override` —
    /// e.g. `Some("true")` so a sandboxed `cleanup_run`'s `docker rm -f` is a
    /// no-op (never spawns the real `docker`, whether or not it is installed).
    async fn test_state_with_dir_and_docker(
        dir: &std::path::Path,
        docker_cmd_override: Option<String>,
    ) -> Arc<AppState> {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (bus_tx, _) = broadcast::channel(80);
        let workspace = workspace_context(
            &db,
            &bus_tx,
            dir,
            Arc::new(pdo_kernel::extension_host::NoExtensions),
        );
        let skills = skill_context(&db, dir, Arc::new(pdo_kernel::extension_host::NoExtensions));
        let platform = test_platform(&db, None).await;
        let test_repo_root: PathBuf = dir.to_path_buf();
        let test_port: u16 = next_test_daemon_port();
        Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
            let session =
                contexts::test_session(weak.clone(), test_repo_root.clone(), test_port, None);
            let run = contexts::test_run(contexts::TestRun {
                state: weak.clone(),
                db: &db,
                bus_tx: &bus_tx,
                session: &session,
                workspace: workspace.clone(),
                skills: skills.clone(),
                repo_root: test_repo_root.clone(),
                port: test_port,
                docker_cmd_override: docker_cmd_override.clone(),
                sandbox_home_override: None,
            });
            AppState {
                workspace,
                skills,
                platform,
                db,
                run,
                bus_tx,
                repo_root: test_repo_root.clone(),
                recent_writes: SelfWrites::new(),
                docker_cmd_override,
                sandbox_home_override: None,
                docker_probe_cache: Arc::new(tokio::sync::Mutex::new(None)),
                harness_catalogue: Arc::new(
                    pdo_harness::adapters::binary_probe::installed_catalogue_reader(),
                ),
                price_sync_lock: tokio::sync::Mutex::new(()),
                price_source_url: None,
                price_refresh_at_boot: false,
                allowed_ws_origins: Vec::new(),
                session,
            }
        })
    }

    const START_END_YAML: &str = "  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

    #[test]
    fn cli_parses_daemon_subcommand() {
        let cli = Cli::try_parse_from(["pdo", "daemon"]).unwrap();
        // With no `--port`, clap resolves the port from `PDO_PORT` if set,
        // otherwise `DEFAULT_PORT` (see the `#[arg(env = "PDO_PORT", ...)]`
        // on the Daemon variant). Compute the expectation the same way so the
        // test is deterministic whether or not the ambient env carries
        // `PDO_PORT` — e.g. when the suite itself runs inside a PDO node.
        let expected = std::env::var("PDO_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        match cli.command {
            Commands::Daemon { bind, port } => {
                assert_eq!(
                    bind,
                    std::env::var("PDO_BIND").ok().and_then(|v| v.parse().ok())
                );
                assert_eq!(port, expected);
            }
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_port() {
        let cli = Cli::try_parse_from(["pdo", "daemon", "--port", "9999"]).unwrap();
        match cli.command {
            Commands::Daemon { port, .. } => assert_eq!(port, 9999),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_bind() {
        let cli = Cli::try_parse_from(["pdo", "daemon", "--bind", "127.0.0.1", "--port", "9999"])
            .unwrap();
        match cli.command {
            Commands::Daemon { bind, port } => {
                assert_eq!(bind, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(port, 9999);
            }
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_complete_subcommand() {
        // Bare `complete` ⇒ `auto = false` (the agent-typed path, unchanged).
        let cli = Cli::try_parse_from(["pdo", "complete"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete { auto: false }));
    }

    #[test]
    fn cli_parses_complete_auto_flag() {
        // `--auto` is the runtime-initiated (`Stop` hook) path.
        let cli = Cli::try_parse_from(["pdo", "complete", "--auto"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete { auto: true }));
    }

    #[test]
    fn cli_parses_fail_subcommand() {
        let cli = Cli::try_parse_from(["pdo", "fail", "--reason", "timeout"]).unwrap();
        match cli.command {
            Commands::Fail { reason } => assert_eq!(reason, "timeout"),
            _ => panic!("expected Fail subcommand"),
        }
    }

    #[test]
    fn cli_fail_requires_reason() {
        assert!(Cli::try_parse_from(["pdo", "fail"]).is_err());
    }

    #[test]
    fn cli_parses_reap_with_defaults() {
        let cli = Cli::try_parse_from(["pdo", "reap"]).unwrap();
        match cli.command {
            Commands::Reap {
                count,
                dry_run,
                ttl_hours,
                terminal_ttl_hours,
                budget_secs,
            } => {
                assert!(!count);
                assert!(!dry_run);
                assert_eq!(ttl_hours, None);
                assert_eq!(terminal_ttl_hours, None);
                assert_eq!(budget_secs, 40, "the wall-clock budget defaults to 40s");
            }
            _ => panic!("expected Reap subcommand"),
        }
    }

    #[test]
    fn cli_parses_reap_flags() {
        let cli = Cli::try_parse_from([
            "pdo",
            "reap",
            "--count",
            "--dry-run",
            "--ttl-hours",
            "1",
            "--terminal-ttl-hours",
            "48",
            "--budget-secs",
            "10",
        ])
        .unwrap();
        match cli.command {
            Commands::Reap {
                count,
                dry_run,
                ttl_hours,
                terminal_ttl_hours,
                budget_secs,
            } => {
                assert!(count);
                assert!(dry_run);
                assert_eq!(ttl_hours, Some(1));
                assert_eq!(terminal_ttl_hours, Some(48));
                assert_eq!(budget_secs, 10);
            }
            _ => panic!("expected Reap subcommand"),
        }
    }

    #[test]
    fn cli_parses_service_install() {
        let cli = Cli::try_parse_from(["pdo", "service", "install"]).unwrap();
        let expected_port = std::env::var("PDO_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(bind, None);
                assert_eq!(port, expected_port);
                assert!(!dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_install_with_port_and_dry_run() {
        let cli = Cli::try_parse_from(["pdo", "service", "install", "--port", "6183", "--dry-run"])
            .unwrap();
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(
                    bind,
                    std::env::var("PDO_BIND").ok().and_then(|v| v.parse().ok())
                );
                assert_eq!(port, 6183);
                assert!(dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_install_with_bind() {
        let cli = Cli::try_parse_from([
            "pdo",
            "service",
            "install",
            "--bind",
            "127.0.0.1",
            "--port",
            "6183",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(bind, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(port, 6183);
                assert!(dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_uninstall() {
        let cli = Cli::try_parse_from(["pdo", "service", "uninstall"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Service {
                action: ServiceAction::Uninstall
            }
        ));
    }

    #[test]
    fn cli_parses_service_status() {
        let cli = Cli::try_parse_from(["pdo", "service", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Service {
                action: ServiceAction::Status
            }
        ));
    }

    #[test]
    fn cli_service_requires_an_action() {
        // `pdo service` with no subcommand is a usage error, not a silent no-op.
        assert!(Cli::try_parse_from(["pdo", "service"]).is_err());
    }

    #[test]
    fn cli_parses_run_create_bare() {
        // The one-argument common case: pipeline positional, everything else
        // deferred to the daemon (empty input, session-derived parent).
        let cli = Cli::try_parse_from(["pdo", "run", "create", "my-pipeline"]).unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            pipeline,
            input,
            variables,
            skills,
            harness,
            target_repo,
            name,
            ..
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(pipeline, "my-pipeline");
        assert_eq!(
            input, "",
            "empty input defers to the pipeline's prompt_required"
        );
        assert!(variables.is_none());
        assert!(skills.is_none());
        assert!(harness.is_none());
        assert!(target_repo.is_none());
        assert!(name.is_none());
    }

    #[test]
    fn cli_parses_run_create_flags() {
        let cli = Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "my-pipeline",
            "--input",
            "do the work",
            "--variables",
            r#"{"pool":"a"}"#,
            "--skills",
            "alpha,beta",
            "--agent-choice",
            r#"{"mode":"custom","harness":"pi"}"#,
            "--harness",
            "pi",
            "--sandbox",
            "off",
            "--target-repo",
            "/somewhere/repo",
            "--target-repos",
            r#"[{"repo":"/somewhere/other","read_only":true}]"#,
            "--source-branch",
            "feat/x",
            "--name",
            "child run",
            "--auto-name",
            "false",
            "--auto-fail",
            "true",
            "--provisioning",
            r#"{}"#,
        ])
        .unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            pipeline,
            input,
            input_file,
            image,
            file,
            variables,
            skills,
            agent_choice,
            harness,
            sandbox,
            target_repo,
            target_repos,
            source_branch,
            name,
            auto_name,
            auto_fail,
            provisioning,
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(pipeline, "my-pipeline");
        assert_eq!(input, "do the work");
        assert!(input_file.is_none());
        assert!(image.is_empty());
        assert!(file.is_empty());
        assert_eq!(variables.as_deref(), Some(r#"{"pool":"a"}"#));
        assert_eq!(skills.as_deref(), Some("alpha,beta"));
        assert_eq!(
            agent_choice.as_deref(),
            Some(r#"{"mode":"custom","harness":"pi"}"#)
        );
        assert_eq!(harness.as_deref(), Some("pi"));
        assert_eq!(sandbox.as_deref(), Some("off"));
        assert_eq!(target_repo.as_deref(), Some("/somewhere/repo"));
        assert_eq!(
            target_repos.as_deref(),
            Some(r#"[{"repo":"/somewhere/other","read_only":true}]"#)
        );
        assert_eq!(source_branch.as_deref(), Some("feat/x"));
        assert_eq!(name.as_deref(), Some("child run"));
        assert_eq!(auto_name, Some(false));
        assert_eq!(auto_fail, Some(true));
        assert_eq!(provisioning.as_deref(), Some("{}"));
    }

    #[test]
    fn cli_parses_run_create_attachments() {
        // #779: `--image` / `--file` repeat, `--input-file` replaces `--input`.
        let cli = Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "my-pipeline",
            "--input-file",
            "/tmp/spec.md",
            "--image",
            "/tmp/a.png",
            "--image",
            "/tmp/b.png",
            "--file",
            "/tmp/fixtures.json",
        ])
        .unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            input_file,
            image,
            file,
            ..
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(
            input_file.as_deref(),
            Some(std::path::Path::new("/tmp/spec.md"))
        );
        assert_eq!(image.len(), 2);
        assert_eq!(file.len(), 1);
    }

    #[test]
    fn cli_refuses_input_and_input_file_together() {
        assert!(Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "p",
            "--input",
            "x",
            "--input-file",
            "/tmp/spec.md",
        ])
        .is_err());
    }

    #[test]
    fn cli_run_requires_an_action() {
        assert!(Cli::try_parse_from(["pdo", "run"]).is_err());
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["pdo", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    async fn seed_running_run(state: &Arc<AppState>, run_id: &str, node_id: &str) {
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some(node_id.into()),
                iter: Some(1),
                payload: None,
            },
        ];
        for ev in &events {
            append_event(state, ev).await.unwrap();
        }
    }

    /// A node's pane, through the session context's use case over this crate's
    /// Run ports — what `GET …/pane` runs.
    async fn read_pane(
        state: &Arc<AppState>,
        run_id: &str,
        node_id: &str,
        iter: i64,
    ) -> Result<
        (String, pdo_session::domain::pane::Pane),
        pdo_session::application::read_pane::ReadPaneError,
    > {
        state
            .session
            .read_pane()
            .execute(&pdo_kernel::Actor::LocalOperator, run_id, node_id, iter)
            .await
    }

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_run() {
        let state = test_state().await;
        assert_eq!(
            read_pane(&state, "nonexistent", "worker", 1).await,
            Err(pdo_session::application::read_pane::ReadPaneError::Run(
                pdo_session::application::ports::RunLookup::NotFound
            ))
        );
    }

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_node() {
        let state = test_state().await;
        seed_running_run(&state, "pane-404-node", "worker").await;
        assert_eq!(
            read_pane(&state, "pane-404-node", "bogus", 1).await,
            Err(pdo_session::application::read_pane::ReadPaneError::NodeNotFound)
        );
    }

    #[tokio::test]
    async fn pane_returns_200_with_placeholder_when_no_tmux() {
        let state = test_state().await;
        seed_running_run(&state, "pane-no-tmux", "worker").await;
        let (session_name, pane) = read_pane(&state, "pane-no-tmux", "worker", 1)
            .await
            .unwrap();
        assert_eq!(
            session_name,
            pdo_session::domain::names::node_session_name("pane-no-tmux", "worker", 1)
        );
        assert!(!pane.content.is_empty());
    }

    #[tokio::test]
    async fn pane_non_latest_iter_returns_placeholder() {
        let state = test_state().await;
        let run_id = "pane-old-iter";
        // Node at iter 3 (simulating cyclic node)
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("reviewer".into()),
                iter: Some(3),
                payload: None,
            },
        ];
        for ev in &events {
            append_event(&state, ev).await.unwrap();
        }

        // Request iter=1 (not latest, which is 3)
        let (_, pane) = read_pane(&state, run_id, "reviewer", 1).await.unwrap();
        assert_eq!(pane.content, "Session no longer available");
        assert!(!pane.resumed);
        assert!(pane.stale, "non-latest iter should be marked stale");
    }

    #[tokio::test]
    async fn pane_latest_iter_is_not_stale() {
        let state = test_state().await;
        seed_running_run(&state, "pane-latest-stale", "worker").await;
        let (_, pane) = read_pane(&state, "pane-latest-stale", "worker", 1)
            .await
            .unwrap();
        assert!(!pane.stale, "latest iter should not be stale");
    }

    // -----------------------------------------------------------------------
    // node_prompt endpoint — Layer 2 contract tests
    // -----------------------------------------------------------------------

    /// Seed a Run whose single `agent` node has already started, with the
    /// isolation (#653) that decides where its NodeRun works.
    async fn seed_run_with_node(
        state: &Arc<AppState>,
        run_id: &str,
        node_id: &str,
        isolated: bool,
    ) {
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "test",
                "node_defs": [
                    {
                        "id": node_id,
                        "node_type": "agent",
                        "isolated_worktree": isolated,
                        "inputs": [],
                        "outputs": []
                    }
                ],
                "edges": []
            })),
        };
        append_event(state, &run_started).await.unwrap();

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node_id.into()),
            iter: Some(1),
            payload: Some(
                serde_json::json!({ "node_type": "agent", "isolated_worktree": isolated }),
            ),
        };
        append_event(state, &node_started).await.unwrap();
    }

    /// Seed a `blocker` run holding enough Running nodes to saturate the
    /// global session cap, so any legitimate spawn in the run under test lands
    /// as an observable `node_waiting` event instead of a real tmux session.
    async fn saturate_session_cap(state: &Arc<AppState>) {
        let run_id = "cap-blocker";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "blocker" })),
        };
        append_event(state, &run_started).await.unwrap();
        for i in 0..admission::DEFAULT_SESSION_CAP {
            let started = event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some(format!("hog-{i}")),
                iter: Some(1),
                payload: None,
            };
            append_event(state, &started).await.unwrap();
        }
    }

    fn seed_event(
        run_id: &str,
        kind: event_log::EventKind,
        node_id: Option<&str>,
        iter: Option<i64>,
        payload: Option<serde_json::Value>,
    ) -> event_log::Event {
        event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload,
        }
    }

    #[tokio::test]
    async fn a_historical_node_stale_run_still_projects_and_displays() {
        // AC3. `NodeStale` keeps its `EventKind` variant and its projection
        // arm on purpose: the log is APPEND-ONLY, so removing the variant would
        // fail deserialisation for every historical Run that carries one,
        // `project()` would return None, and those Runs would VANISH from the UI.
        // Nothing emits it any more — but everything can still read it.
        let state = test_state().await;
        let run_id = "historical-stale";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": "test" })),
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("worker"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStale,
                Some("worker"),
                Some(1),
                Some(serde_json::json!({ "reason": "idle_outputs_incomplete" })),
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        // The projection survives …
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).expect("a historical stale Run must project");
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Stale
        );

        // … and so does the HTTP view the UI reads.
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "the Run must still be served"
        );
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            body["nodes"]["worker"]["status"], "stale",
            "a historical stale node must still read `stale` in the UI: {body}"
        );
    }

    #[tokio::test]
    async fn settings_view_discloses_the_autocomplete_tiers() {
        // #469 §4 / ADR-0015: `effective` comes from the SAME resolver the sweep
        // consumes, and both directions of an explicit save are a STORED decision
        // (a stored 0 must beat `PDO_AUTOCOMPLETE_TURN_END=1`, or unticking the box
        // would be a no-op on a machine that sets the env var).
        let state = test_state().await;

        let fresh = build_settings_view(&state).await.unwrap();
        assert_eq!(fresh["autocomplete_turn_end"]["effective"], false);
        assert_eq!(fresh["autocomplete_turn_end"]["default"], false);
        assert!(fresh["autocomplete_turn_end"]["stored"].is_null());

        for (on, want_source) in [(true, "stored"), (false, "stored")] {
            instance_config::update(
                &state.db,
                instance_config::UpdateInstanceConfig {
                    autocomplete_turn_end: Some(on),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let view = build_settings_view(&state).await.unwrap();
            assert_eq!(
                view["autocomplete_turn_end"]["effective"],
                serde_json::json!(on)
            );
            assert_eq!(
                view["autocomplete_turn_end"]["stored"],
                serde_json::json!(on),
                "the stored tier is disclosed as a bool, not as 0/1"
            );
            assert_eq!(
                view["autocomplete_turn_end"]["source"],
                serde_json::json!(want_source)
            );
        }
    }

    #[tokio::test]
    async fn settings_view_discloses_the_default_auto_name_tiers() {
        // #338 / ADR-0015: `effective` comes from the SAME resolver the create
        // chokepoint consumes, the built-in default is ON, and both directions of an
        // explicit save are a STORED decision (a stored 0 must beat
        // `PDO_DEFAULT_AUTO_NAME=1`, or unticking the box would be a no-op with the env set).
        let state = test_state().await;

        let fresh = build_settings_view(&state).await.unwrap();
        assert_eq!(fresh["default_auto_name"]["effective"], true);
        assert_eq!(fresh["default_auto_name"]["default"], true);
        assert!(fresh["default_auto_name"]["stored"].is_null());

        for on in [false, true] {
            instance_config::update(
                &state.db,
                instance_config::UpdateInstanceConfig {
                    default_auto_name: Some(on),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let view = build_settings_view(&state).await.unwrap();
            assert_eq!(
                view["default_auto_name"]["effective"],
                serde_json::json!(on)
            );
            assert_eq!(
                view["default_auto_name"]["stored"],
                serde_json::json!(on),
                "the stored tier is disclosed as a bool, not as 0/1"
            );
            assert_eq!(
                view["default_auto_name"]["source"],
                serde_json::json!("stored")
            );
        }
    }

    // Characterization net for POST /runs/{id}/commands
    //
    // Written BEFORE the dispatcher refactor and green on the unmodified tree.
    // These do not describe a contract anyone designed — they RECORD what HEAD
    // actually answers, so a "pure move" that quietly changes a status, a
    // content-type or a side effect goes red instead of shipping. Every value
    // below was observed, never deduced: the surface asserts no content-type
    // anywhere else in the repo, and the unknown-run behaviour is a three-way
    // split that no test covered.

    #[tokio::test]
    async fn run_diff_returns_404_for_nonexistent_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/no-such-run/diff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn run_diff_returns_empty_diff_when_no_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-empty";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", true).await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.is_empty() || body.trim().is_empty());
    }

    #[tokio::test]
    async fn run_diff_returns_aggregate_diff_with_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-agg";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", true).await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Make a change on the pipeline branch
        std::fs::write(wt_dir.join("new_file.rs"), "fn hello() {}\n").unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&wt_dir)
                .output()
                .unwrap()
        };
        run(&["add", "new_file.rs"]);
        run(&["commit", "-m", "add new_file"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(
            body.contains("new_file.rs"),
            "diff should mention new_file.rs"
        );
        assert!(
            body.contains("fn hello()"),
            "diff should contain the added content"
        );
    }

    #[tokio::test]
    async fn run_diff_excludes_main_advance_after_fork() {
        // Three-dot range → commits landed on main AFTER the fork must not
        // appear as phantom deletions. RED under two-dot `git diff HEAD pdo/run-<id>`.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-fork";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", true).await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let git = |dir: &std::path::Path, args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap()
        };

        // The run's real work on the run branch.
        std::fs::write(wt_dir.join("run_file.rs"), "fn run_work() {}\n").unwrap();
        git(&wt_dir, &["add", "run_file.rs"]);
        git(&wt_dir, &["commit", "-m", "run work"]);

        // Advance main/HEAD AFTER the fork.
        std::fs::write(repo.join("main_advance.rs"), "fn advanced_on_main() {}\n").unwrap();
        git(repo, &["add", "main_advance.rs"]);
        git(repo, &["commit", "-m", "advance main after fork"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        assert!(
            body.contains("run_file.rs") && body.contains("fn run_work()"),
            "diff must contain the run's own change: {body}"
        );
        // RED under two-dot (main_advance.rs shown as a deletion); GREEN under three-dot.
        assert!(
            !body.contains("main_advance.rs"),
            "diff must not mention main's post-fork file (phantom deletion): {body}"
        );
        assert!(
            !body.contains("advanced_on_main"),
            "diff must not contain main's post-fork content as a removed line: {body}"
        );
    }

    #[tokio::test]
    async fn run_diff_ignores_parked_checkout_via_frozen_fork_sha() {
        // #417 (layer-2 mirror of the layer-3a `run_diff_range` test): with a `fork_sha`
        // frozen in `RunStarted`, `GET /runs/<id>/diff` must base on the fork point even
        // when the shared checkout's HEAD is parked on a divergent branch that forks
        // BEFORE it. RED under the old `HEAD...` range (the divergent branch's pre-fork
        // files render as a phantom); GREEN with `run_diff_base`.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let git = |dir: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        let default_branch =
            String::from_utf8_lossy(&git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
                .trim()
                .to_string();

        // Divergent branch off c0 with a phantom-carrying commit; then advance the
        // default branch past the fork.
        git(repo, &["checkout", "-b", "wip/parked"]);
        std::fs::write(repo.join("phantom.rs"), "fn phantom() {}\n").unwrap();
        git(repo, &["add", "phantom.rs"]);
        git(repo, &["commit", "-m", "parked: unrelated work"]);
        git(repo, &["checkout", &default_branch]);
        std::fs::write(repo.join("main_advance.rs"), "fn advanced_on_main() {}\n").unwrap();
        git(repo, &["add", "main_advance.rs"]);
        git(repo, &["commit", "-m", "advance main after divergence"]);

        // Freeze the fork point = current default-branch tip.
        let fork_sha = String::from_utf8_lossy(&git(repo, &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        let run_id = "diff-parked-fork";
        let state = test_state_with_dir(repo).await;
        // Seed a RunStarted carrying the frozen `fork_sha` (the fix's mechanism), plus a
        // node so the projection is realistic.
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "test",
                "fork_sha": fork_sha,
                "node_defs": [
                    { "id": "impl-1", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] }
                ],
                "edges": []
            })),
        };
        append_event(&state, &run_started).await.unwrap();

        // Cut the run branch/worktree from the fork point.
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, &default_branch).unwrap();

        // The run's only real change.
        std::fs::write(wt_dir.join("run_file.rs"), "fn run_work() {}\n").unwrap();
        git(&wt_dir, &["add", "run_file.rs"]);
        git(&wt_dir, &["commit", "-m", "run work"]);

        // PARK the shared checkout on the divergent branch (the #417 / #451 condition).
        git(repo, &["checkout", "wip/parked"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        assert!(
            body.contains("run_file.rs") && body.contains("fn run_work()"),
            "diff must contain the run's own change: {body}"
        );
        // The phantom: the parked branch's pre-fork file. RED under `HEAD...`.
        assert!(
            !body.contains("phantom.rs"),
            "parked divergent HEAD must not add a phantom file to the diff: {body}"
        );
        assert!(
            !body.contains("main_advance.rs"),
            "main's post-fork file must not appear either (three-dot base): {body}"
        );
    }

    #[tokio::test]
    async fn run_diff_excludes_pdo_artifacts() {
        // `:(exclude).pdo/` keeps the blackboard out of the diff. RED under
        // the current no-pathspec two-dot call.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-pdo";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", true).await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&wt_dir)
                .output()
                .unwrap()
        };
        std::fs::write(wt_dir.join("code.rs"), "fn code_change() {}\n").unwrap();
        std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
        std::fs::write(wt_dir.join(".pdo/artifact.txt"), "internal-artifact\n").unwrap();
        git(&["add", "code.rs"]);
        // Force so it's genuinely committed → test is not vacuous
        git(&["add", "-f", ".pdo/artifact.txt"]);
        git(&["commit", "-m", "code + pdo artifact"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        assert!(
            body.contains("code.rs") && body.contains("fn code_change()"),
            "diff should contain the real code change: {body}"
        );
        assert!(
            !body.contains("artifact.txt") && !body.contains("internal-artifact"),
            "diff must exclude .pdo/ artifacts: {body}"
        );
    }

    /// #748 fixture: a Run whose `target_repo` is a DIFFERENT git repo than the
    /// daemon's `repo_root`. The daemon root is a bare-ish repo with no run
    /// branch at all, so anything computed in the cwd answers 404 / empty —
    /// the exact pre-#748 symptom. Returns `(target_repo, worktree_dir)`.
    async fn seed_run_in_other_repo(
        state: &Arc<AppState>,
        target: &std::path::Path,
        run_id: &str,
    ) -> PathBuf {
        init_test_repo(target);
        let fork_sha = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(target)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "test",
                "target_repo": target.to_string_lossy(),
                "fork_sha": fork_sha,
                "node_defs": [
                    { "id": "impl-1", "node_type": "agent", "isolated_worktree": true, "inputs": [], "outputs": [] }
                ],
                "edges": []
            })),
        };
        append_event(state, &run_started).await.unwrap();
        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("impl-1".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "node_type": "agent", "isolated_worktree": true })),
        };
        append_event(state, &node_started).await.unwrap();

        let wt_dir = target.join(".pdo/runs").join(run_id).join("worktree");
        create_worktree(target, &wt_dir, &format!("pdo/run-{run_id}"), "HEAD").unwrap();
        wt_dir
    }

    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    async fn body_string(resp: Response) -> String {
        String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn run_diff_computes_in_effective_repo_not_daemon_cwd() {
        // #748 AC1: the daemon's cwd has NO run branch; the Run's target repo has.
        // RED under `.current_dir(&state.repo_root)`: git answers "unknown
        // revision" → 404.
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let run_id = "diff-other-repo";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target_dir.path(), run_id).await;

        std::fs::write(wt_dir.join("elsewhere.rs"), "fn elsewhere() {}\n").unwrap();
        git_in(&wt_dir, &["add", "elsewhere.rs"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "work in target repo"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(
            body.contains("elsewhere.rs") && body.contains("fn elsewhere()"),
            "raw diff must come from the target repo: {body}"
        );
    }

    #[tokio::test]
    async fn structured_diff_lists_files_hunks_and_matches_loc_bounds() {
        // #748 AC2: files + hunks, three-dot fork → tip, `.pdo/` excluded, and the
        // totals equal the LOC stat computed over the same bounds.
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "sdiff";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;

        // Modify README (1 del / 2 add), add a file (3 add), add a blackboard
        // artefact (excluded), add a binary (counted as a file, 0/0).
        std::fs::write(wt_dir.join("README.md"), "# test\n\nmore\n").unwrap();
        std::fs::write(wt_dir.join("src.rs"), "a\nb\nc\n").unwrap();
        std::fs::create_dir_all(wt_dir.join(".pdo")).unwrap();
        std::fs::write(wt_dir.join(".pdo/artifact.txt"), "blackboard\n").unwrap();
        std::fs::write(wt_dir.join("blob.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["add", "-f", ".pdo/artifact.txt"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "run work"]);
        // Advance the target's main after the fork: three-dot must ignore it.
        std::fs::write(target.join("main_only.rs"), "fn main_only() {}\n").unwrap();
        git_in(target, &["add", "main_only.rs"]);
        git_in(target, &["commit", "-q", "-m", "main advance"]);

        // The events are what `load_projected` reads; compute LOC over the same
        // repo/bounds the endpoint must use.
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        let loc = compute_run_loc(target, run_id, run_diff_base(&run_state)).expect("loc");

        let app = build_router(state);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();

        assert_eq!(json["three_dot"], serde_json::json!(true));
        // #835: the worktree exists, so the default destination is the working
        // tree; clean after the commit, its snapshot is HEAD's own tree and the
        // bounds equal fork → tip.
        assert_eq!(json["to_ref"], serde_json::json!("worktree"));
        assert_eq!(
            json["from_sha"],
            serde_json::json!(run_state.fork_sha.clone().unwrap())
        );
        assert_eq!(
            json["to_sha"].as_str().unwrap(),
            git_in(&wt_dir, &["rev-parse", "HEAD^{tree}"])
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured?from=fork&to=tip"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let tip_json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(
            tip_json["to_ref"],
            serde_json::json!(format!("pdo/run-{run_id}"))
        );
        assert_eq!(
            tip_json["to_sha"].as_str().unwrap(),
            git_in(&wt_dir, &["rev-parse", "HEAD"])
        );
        assert_eq!(tip_json["files"], json["files"]);

        assert_eq!(
            json["files_changed"].as_u64().unwrap(),
            loc.files_changed,
            "{json}"
        );
        assert_eq!(json["additions"].as_u64().unwrap(), loc.insertions);
        assert_eq!(json["deletions"].as_u64().unwrap(), loc.deletions);

        let files = json["files"].as_array().unwrap();
        let by_path = |p: &str| {
            files
                .iter()
                .find(|f| f["new_path"] == serde_json::json!(p))
                .unwrap_or_else(|| panic!("{p} missing in {json}"))
                .clone()
        };
        assert!(files
            .iter()
            .all(|f| f["new_path"] != serde_json::json!("main_only.rs")));
        assert!(files
            .iter()
            .all(|f| f["new_path"] != serde_json::json!(".pdo/artifact.txt")));

        let readme = by_path("README.md");
        assert_eq!(readme["status"], serde_json::json!("modified"));
        assert_eq!(readme["additions"], serde_json::json!(2));
        assert_eq!(readme["deletions"], serde_json::json!(0));
        let hunk = &readme["hunks"][0];
        assert_eq!(hunk["old_start"], serde_json::json!(1));
        let kinds: Vec<&str> = hunk["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["context", "add", "add"]);
        assert_eq!(hunk["lines"][2]["content"], serde_json::json!("more"));
        assert_eq!(hunk["lines"][2]["new_no"], serde_json::json!(3));
        assert!(hunk["lines"][2]["old_no"].is_null());

        let src = by_path("src.rs");
        assert_eq!(src["status"], serde_json::json!("added"));
        assert!(src["old_path"].is_null());
        assert_eq!(src["additions"], serde_json::json!(3));

        let bin = by_path("blob.bin");
        assert_eq!(bin["binary"], serde_json::json!(true));
        assert_eq!(bin["status"], serde_json::json!("added"));
        assert_eq!(bin["hunks"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn run_refs_lists_deliveries_and_stable_ids_resolve_on_diff_and_file() {
        // #749 AC1/AC5: the refs endpoint reads deliveries from the event log,
        // labels them by node and iter, and the stable ids resolve on the
        // structured diff and file-at-ref endpoints (URLs never carry SHAs).
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "refs-run";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
        let fork = git_in(target, &["rev-parse", "HEAD"]);
        let c0 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        std::fs::write(wt_dir.join("one.rs"), "1\n").unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "one"]);
        let c1 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        std::fs::write(wt_dir.join("two.rs"), "2\n").unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "two"]);
        let c2 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        // impl-1 delivered c0 → c1 at iter 1; a second node delivered c1 → c2.
        for (node, before, after) in [("impl-1", &c0, &c1), ("worker-2", &c1, &c2)] {
            append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: run_id.into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event_log::EventKind::NodeDelivered,
                    node_id: Some(node.into()),
                    iter: Some(1),
                    payload: Some(serde_json::json!({ "before": before, "after": after })),
                },
            )
            .await
            .unwrap();
        }
        // impl-1 is still running and isolated: give it a live sub-worktree branch.
        let live_branch = worktrees::sub_worktree_branch(run_id, "impl-1", 1);
        git_in(target, &["branch", &live_branch, &c2]);

        let app = build_router(state);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/refs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        let ids: Vec<&str> = json["refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                "fork",
                "node:impl-1:1:before",
                "node:impl-1:1:after",
                "node:worker-2:1:before",
                "node:worker-2:1:after",
                "live:impl-1",
                "tip",
                "worktree",
            ]
        );
        let refs = json["refs"].as_array().unwrap();
        assert_eq!(refs[0]["sha"], serde_json::json!(fork));
        assert_eq!(refs[2]["sha"], serde_json::json!(c1));
        assert_eq!(
            refs[2]["label"],
            serde_json::json!("impl-1 · iter 1 · after")
        );
        assert_eq!(refs[5]["sha"], serde_json::json!(c2));
        assert_eq!(refs[6]["sha"], serde_json::json!(c2));
        // #835: the working tree is a ref while the worktree exists — clean here,
        // so its snapshot is HEAD's tree — and the default destination.
        assert_eq!(refs[7]["kind"], serde_json::json!("worktree"));
        assert_eq!(refs[7]["label"], serde_json::json!("Working tree"));
        assert_eq!(
            refs[7]["sha"].as_str().unwrap(),
            git_in(&wt_dir, &["rev-parse", "HEAD^{tree}"])
        );
        assert_eq!(json["default_from"], serde_json::json!("fork"));
        assert_eq!(json["default_to"], serde_json::json!("worktree"));
        let deliveries = json["deliveries"].as_array().unwrap();
        assert_eq!(deliveries.len(), 3);
        assert_eq!(deliveries[0]["status"], serde_json::json!("delivered"));
        assert_eq!(deliveries[2]["status"], serde_json::json!("running"));
        assert_eq!(deliveries[2]["live"], serde_json::json!("live:impl-1"));

        // The delivery pair, by id: only impl-1's file, two-dot.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/diff/structured?from=node:impl-1:1:before&to=node:impl-1:1:after"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(json["three_dot"], serde_json::json!(false));
        assert_eq!(json["from_sha"], serde_json::json!(c0));
        assert_eq!(json["to_sha"], serde_json::json!(c1));
        let paths: Vec<&str> = json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["new_path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["one.rs"]);

        // `fork` → `tip` spelled out is the three-dot default.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured?from=fork&to=tip"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(json["three_dot"], serde_json::json!(true));
        assert_eq!(json["files_changed"], serde_json::json!(2));

        // tip → live branch resolves too.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/diff/structured?from=tip&to=live:impl-1"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // An id the Run does not know is a 404, never handed to git.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/diff/structured?from=node:impl-1:2:before"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(body_string(resp).await.contains("unknown run ref"));

        // The file endpoint resolves ids as well.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/file?path=one.rs&ref=node:impl-1:1:after"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "1\n");
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/file?path=one.rs&ref=node:impl-1:1:before"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "one.rs did not exist before"
        );
    }

    #[tokio::test]
    async fn default_review_diff_shows_the_dirty_worktree_of_a_running_run() {
        // #835: with no query, the structured diff is fork → working tree: a
        // modified tracked file and an untracked file show up while nothing is
        // committed; the file endpoint reads the file on disk; a comment
        // anchored on the working tree freezes the snapshot tree id; and once
        // the worktree is gone, the default falls back to fork → tip.
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "dirty-run";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
        let head = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        std::fs::write(wt_dir.join("README.md"), "# test\n\nuncommitted\n").unwrap();
        std::fs::write(wt_dir.join("REPRO_835.md"), "untracked\n").unwrap();
        std::fs::create_dir_all(wt_dir.join(".pdo/artifacts")).unwrap();
        std::fs::write(wt_dir.join(".pdo/artifacts/out.md"), "blackboard\n").unwrap();
        // The documented target-repo setup (ADR-0060) ignores `.pdo/` as a
        // directory: the snapshot must not trip on it (#835 FP finding).
        let common_dir = git_in(
            &wt_dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        std::fs::create_dir_all(Path::new(&common_dir).join("info")).unwrap();
        std::fs::write(Path::new(&common_dir).join("info/exclude"), ".pdo/\n").unwrap();
        let status_before = git_in(&wt_dir, &["status", "--porcelain"]);

        let app = build_router(state);
        let get = |uri: String| {
            app.clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        };

        let resp = get(format!("/runs/{run_id}/diff/structured"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(json["to_ref"], serde_json::json!("worktree"));
        assert_eq!(json["three_dot"], serde_json::json!(true));
        let mut paths: Vec<&str> = json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["new_path"].as_str().unwrap())
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["README.md", "REPRO_835.md"], "{json}");
        let snapshot = json["to_sha"].as_str().unwrap().to_string();
        assert_ne!(snapshot, head);
        // The run branch itself has not moved: fork → tip is still empty.
        let resp = get(format!("/runs/{run_id}/diff/structured?to=tip"))
            .await
            .unwrap();
        let tip: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(tip["files_changed"], serde_json::json!(0));
        // Reading the diff moved nothing for the node.
        assert_eq!(git_in(&wt_dir, &["status", "--porcelain"]), status_before);
        assert_eq!(git_in(&wt_dir, &["rev-parse", "HEAD"]), head);

        // The working tree is the destination only.
        let resp = get(format!(
            "/runs/{run_id}/diff/structured?from=worktree&to=tip"
        ))
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // File at the default ref = the file on disk, untracked included.
        let resp = get(format!("/runs/{run_id}/file?path=REPRO_835.md"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "untracked\n");
        let resp = get(format!("/runs/{run_id}/file?path=REPRO_835.md&ref=tip"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = get(format!("/runs/{run_id}/file?path=nope.md&ref=worktree"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // The refs endpoint lists it with the same snapshot id.
        let resp = get(format!("/runs/{run_id}/refs")).await.unwrap();
        let refs: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        let wt = refs["refs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == serde_json::json!("worktree"))
            .expect("worktree ref listed");
        assert_eq!(wt["sha"], serde_json::json!(snapshot));
        assert_eq!(refs["default_to"], serde_json::json!("worktree"));

        // Worktree gone (finished + cleaned, or archived): back to fork → tip.
        git_in(
            target,
            &["worktree", "remove", "--force", wt_dir.to_str().unwrap()],
        );
        let resp = get(format!("/runs/{run_id}/refs")).await.unwrap();
        let refs: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert!(refs["refs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != serde_json::json!("worktree")));
        assert_eq!(refs["default_to"], serde_json::json!("tip"));
        let resp = get(format!("/runs/{run_id}/diff/structured"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(
            json["to_ref"],
            serde_json::json!(format!("pdo/run-{run_id}"))
        );
        let resp = get(format!("/runs/{run_id}/diff/structured?to=worktree"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn structured_diff_explicit_pair_is_two_dot_and_unsafe_ref_is_400() {
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "sdiff-pair";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
        let c0 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        std::fs::write(wt_dir.join("one.rs"), "1\n").unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "one"]);
        let c1 = git_in(&wt_dir, &["rev-parse", "HEAD"]);
        std::fs::write(wt_dir.join("two.rs"), "2\n").unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "two"]);

        let app = build_router(state);
        // A delivery-like pair (c0 → c1) shows only `one.rs`.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured?from={c0}&to={c1}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(json["three_dot"], serde_json::json!(false));
        assert_eq!(json["files_changed"], serde_json::json!(1));
        assert_eq!(json["files"][0]["new_path"], serde_json::json!("one.rs"));

        // Option injection is refused before git ever runs.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/diff/structured?from=--output%3D%2Ftmp%2Fx"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // An unknown ref is a 404, like the raw endpoint.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured?to=no-such-branch"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn structured_diff_is_empty_when_no_changes_and_404_when_branch_gone() {
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "sdiff-empty";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;

        let app = build_router(state);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(json["files_changed"], serde_json::json!(0));
        assert_eq!(json["files"].as_array().unwrap().len(), 0);

        // Cleanup deletes the run branch (ADR-0020): nothing left to diff.
        git_in(
            target,
            &["worktree", "remove", "--force", wt_dir.to_str().unwrap()],
        );
        git_in(target, &["branch", "-D", &format!("pdo/run-{run_id}")]);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/diff/structured"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    async fn seed_run_for_node_control(state: &Arc<AppState>, run_id: &str, pipeline_name: &str) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
        };
        append_event(state, &ev).await.unwrap();
    }

    async fn seed_node_started(state: &Arc<AppState>, run_id: &str, node_id: &str, iter: i64) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: None,
        };
        append_event(state, &ev).await.unwrap();
    }

    // spawn_node driven directly through the injectable SpawnDeps
    //
    // Before the carve, `spawn_node` could only be exercised end-to-end through a
    // live daemon (POST /runs, real tmux). With `SpawnDeps::from_state`, its
    // ordering invariants are drivable in-process against fakes: these three
    // tests each pin one exit path (throttle / panic-reap / guard-refuse) without
    // launching a session (`tmux_cmd_override = "exec true"`).

    // The sandbox spawn precondition (block + replay).
    //
    // A sandboxed Run's node session runs `docker exec … pdo-sbx-<run>`, and
    // `create_run`'s detached prep task used to be the ONLY thing that waited for the
    // container: the pipeline watcher called the same advance path with no such wait,
    // so the exec hit a container that did not exist and the session died ~25 s later.
    // The precondition lives in `spawn_node`, which is why these tests drive the spawn
    // and the advance rather than the watcher's mpsc plumbing — the watcher calls
    // `spawn_ready_after_event` == `advance_run`, the entry point exercised here.

    #[tokio::test]
    async fn pane_resurrection_refused_at_session_cap() {
        // #487 §3 — a click on a dead pane must not bypass the session cap
        //. At the cap the resurrection is declined and reserves NOTHING:
        // no NodeStarted, so it never overshoots the bound a spawn would respect.
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;
        saturate_session_cap(&state).await;

        let run_id = "pane-cap";
        seed_run_for_node_control(&state, run_id, "test-pipe").await;
        seed_node_started(&state, run_id, "worker", 1).await;
        // The resurrection branch is only reached when the node's working dir still
        // exists (a non-isolated node's dir is the run's pipeline worktree).
        std::fs::create_dir_all(worktree_dir_for_run(repo_root, run_id)).unwrap();

        let started_before = load_events(&state.db, run_id)
            .await
            .unwrap()
            .iter()
            .filter(|e| {
                e.kind == event_log::EventKind::NodeStarted
                    && e.node_id.as_deref() == Some("worker")
            })
            .count();

        let (_, pane) = read_pane(&state, run_id, "worker", 1).await.unwrap();
        assert_eq!(
            pane.source,
            pdo_session::domain::pane::PaneSource::Unavailable,
            "a resurrection blocked by the cap serves no live session: {pane:?}"
        );
        assert!(
            pane.content.contains("session cap"),
            "the operator is told why: {pane:?}"
        );

        let started_after = load_events(&state.db, run_id)
            .await
            .unwrap()
            .iter()
            .filter(|e| {
                e.kind == event_log::EventKind::NodeStarted
                    && e.node_id.as_deref() == Some("worker")
            })
            .count();
        assert_eq!(
            started_after, started_before,
            "a resurrection refused at the cap must reserve nothing (no NodeStarted)"
        );
    }

    async fn get_settings_json(state: &Arc<AppState>) -> serde_json::Value {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "GET /settings should 200");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn put_settings_resp(
        state: &Arc<AppState>,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/settings")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn get_settings_default_shape_stored_null() {
        // A fresh instance has an all-NULL row. Assert the robust invariants: the
        // per-field shape, the `default` tier's constants, and `stored == null`.
        // We deliberately do NOT assert `source`/`effective` for the cap here —
        // they read the process-global `PDO_SESSION_CAP`, which a concurrent
        // env-mutating unit test could transiently shadow (the resolver unit
        // tests own the env-precedence assertions).
        let state = test_state().await;
        let view = get_settings_json(&state).await;

        for field in ["session_cap", "reaper_ttl_secs", "guard_timeout_secs"] {
            let f = &view[field];
            assert!(
                f["stored"].is_null(),
                "{field}.stored must be null on a fresh row: {f}"
            );
            assert!(f.get("effective").is_some(), "{field}.effective missing");
            assert!(f.get("source").is_some(), "{field}.source missing");
            assert!(f.get("env").is_some(), "{field}.env key missing");
            assert!(f.get("default").is_some(), "{field}.default missing");
        }
        assert_eq!(view["session_cap"]["default"], 20);
        assert_eq!(view["reaper_ttl_secs"]["default"], 3600);
        assert_eq!(view["guard_timeout_secs"]["default"], 60);
        assert!(
            view["updated_at"].is_string(),
            "updated_at must be surfaced"
        );

        // default_model: string knob with no baked-in default. On a fresh
        // row (and no PDO_DEFAULT_MODEL) stored/effective/default are all null and
        // the source is "default" (= the account default, no --model).
        let dm = &view["default_model"];
        assert!(
            dm["stored"].is_null(),
            "default_model.stored must be null: {dm}"
        );
        assert!(
            dm["default"].is_null(),
            "default_model.default is always null: {dm}"
        );
        assert!(
            dm.get("effective").is_some(),
            "default_model.effective missing"
        );
        assert!(dm.get("source").is_some(), "default_model.source missing");
        assert!(dm.get("env").is_some(), "default_model.env key missing");
        // `env` reads the process-global PDO_DEFAULT_MODEL; a concurrent
        // env-mutating unit test could transiently shadow it, so we assert the
        // effective/source pairing only in the stored-wins tests below.

        // #471 AC1: the two sandbox-image knobs are GONE from the payload, along with the
        // `sandbox_image` tag disclosure that only existed to explain the Dockerfile field.
        // Asserted here, on the shape test, so a re-add cannot slip in as an extra key.
        for gone in ["image_source", "dockerfile_path", "sandbox_image"] {
            assert!(
                view.get(gone).is_none(),
                "#471 removed `{gone}` from GET /settings: {view}"
            );
        }
        // What is left on the sandbox side: the profile knob, the profile list, the probe.
        for kept in ["default_sandbox", "sandbox_profiles", "sandbox_docker"] {
            assert!(view.get(kept).is_some(), "`{kept}` must stay: {view}");
        }
    }

    #[tokio::test]
    async fn settings_view_lists_harnesses_with_source_and_installed() {
        // #586 AC#1: `GET /settings → harness_descriptors` carries, per resolved
        // harness, `name` + `source` (builtin|descriptor) + `installed` (bool) — the
        // shape the two-section, grey-if-uninstalled picker reads. The embedded floor
        // (claude/opencode) always resolves as `builtin`; disk-declared harnesses (if
        // any on this host) ride as `descriptor`. `installed` reads the real `$PATH`,
        // so we assert its TYPE, never a value — the picker only greys on it. `rejected`
        // is preserved alongside (AC: kept for an eventual display).
        let state = test_state().await;
        let view = get_settings_json(&state).await;

        let harnesses = view["harness_descriptors"]["harnesses"]
            .as_array()
            .expect("harness_descriptors.harnesses must be an array");
        assert!(
            !harnesses.is_empty(),
            "the floor alone makes this non-empty"
        );

        for h in harnesses {
            assert!(h["name"].is_string(), "each harness carries a name: {h}");
            let source = h["source"].as_str().expect("source is a string");
            assert!(
                source == "builtin" || source == "descriptor",
                "source is builtin|descriptor, got {source:?}"
            );
            assert!(h["installed"].is_boolean(), "installed is a bool: {h}");
            // #616/ADR-0053: the offer deduced from the binary rides on each entry —
            // the model & effort catalogues (arrays, possibly empty = free-text
            // fallback), the served effort-axis fact (a bool, replacing the client's
            // hard-coded map), and the probed version (a string or null).
            assert!(h["models"].is_array(), "models is an array: {h}");
            assert!(h["efforts"].is_array(), "efforts is an array: {h}");
            assert!(h["has_effort"].is_boolean(), "has_effort is a bool: {h}");
            assert!(
                h["version"].is_string() || h["version"].is_null(),
                "version is a string or null: {h}"
            );
        }

        // The embedded floor always resolves, always as `builtin` — copilot and pi
        // are the third and fourth arms, listed under "Built-in" like the other two.
        for floor in ["claude", "opencode", "copilot", "pi"] {
            let entry = harnesses
                .iter()
                .find(|h| h["name"] == floor)
                .unwrap_or_else(|| panic!("floor harness `{floor}` must appear: {view}"));
            assert_eq!(entry["source"], "builtin", "`{floor}` is built-in");
        }

        assert!(
            view["harness_descriptors"]["rejected"].is_array(),
            "rejected stays an array: {view}"
        );
    }

    #[tokio::test]
    async fn put_settings_round_trips_the_autocomplete_flag_both_ways() {
        // #469 §4: the checkbox is authoritative in BOTH directions. Unticking must
        // persist a stored `0` (source still "stored"), not clear back to unset —
        // otherwise it could not override `PDO_AUTOCOMPLETE_TURN_END=1`.
        let state = test_state().await;

        let (status, view) = put_settings_resp(&state, r#"{"autocomplete_turn_end": true}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["autocomplete_turn_end"]["effective"], true);
        assert_eq!(view["autocomplete_turn_end"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .autocomplete_turn_end,
            Some(1)
        );

        let (status, view) = put_settings_resp(&state, r#"{"autocomplete_turn_end": false}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["autocomplete_turn_end"]["effective"], false);
        assert_eq!(
            view["autocomplete_turn_end"]["source"], "stored",
            "unticking is a stored decision, not a fall-through: {view}"
        );
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .autocomplete_turn_end,
            Some(0),
            "off must persist a stored 0, never NULL"
        );
    }

    #[tokio::test]
    async fn put_settings_round_trips_the_default_auto_name_flag_both_ways() {
        // The checkbox is authoritative in BOTH directions. Unticking must persist a
        // stored `0` (source still "stored"), not clear back to unset — otherwise it could
        // not override `PDO_DEFAULT_AUTO_NAME=1`.
        let state = test_state().await;

        let (status, view) = put_settings_resp(&state, r#"{"default_auto_name": false}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["default_auto_name"]["effective"], false);
        assert_eq!(view["default_auto_name"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .default_auto_name,
            Some(0),
            "off must persist a stored 0, never NULL"
        );

        let (status, view) = put_settings_resp(&state, r#"{"default_auto_name": true}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["default_auto_name"]["effective"], true);
        assert_eq!(view["default_auto_name"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .default_auto_name,
            Some(1)
        );
    }

    #[tokio::test]
    async fn put_settings_round_trips_the_review_agent_can_resolve_flag_both_ways() {
        // #751: default OFF (the human resolves); both directions of a save are a
        // stored decision — unticking persists a `0` that beats
        // `PDO_REVIEW_AGENT_CAN_RESOLVE=1`.
        let state = test_state().await;
        let fresh = get_settings_json(&state).await;
        assert_eq!(fresh["review_agent_can_resolve"]["default"], false);
        assert!(fresh["review_agent_can_resolve"]["stored"].is_null());
        if fresh["review_agent_can_resolve"]["env"].is_null() {
            assert_eq!(fresh["review_agent_can_resolve"]["effective"], false);
            assert_eq!(fresh["review_agent_can_resolve"]["source"], "default");
        }

        let (status, view) =
            put_settings_resp(&state, r#"{"review_agent_can_resolve": true}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["review_agent_can_resolve"]["effective"], true);
        assert_eq!(view["review_agent_can_resolve"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .review_agent_can_resolve,
            Some(1)
        );

        let (status, view) =
            put_settings_resp(&state, r#"{"review_agent_can_resolve": false}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["review_agent_can_resolve"]["effective"], false);
        assert_eq!(view["review_agent_can_resolve"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .review_agent_can_resolve,
            Some(0),
            "off must persist a stored 0, never NULL"
        );
    }

    #[tokio::test]
    async fn put_settings_round_trips_the_manager_enabled_flag_both_ways() {
        // Manager on demand: the checkbox is authoritative in BOTH directions.
        // Unticking must persist a stored `0` (not a clear back to NULL), or it
        // could never override a `PDO_MANAGER_ENABLED=1`. The fresh view also
        // discloses the built-in default: OFF — a Run starts managerless.
        let state = test_state().await;

        let (_, fresh) = put_settings_resp(&state, "{}").await;
        assert_eq!(fresh["manager_enabled"]["effective"], false);
        assert_eq!(fresh["manager_enabled"]["source"], "default");
        assert_eq!(fresh["manager_enabled"]["default"], false);
        assert_eq!(
            fresh["manager_profile"]["effective"],
            serde_json::Value::Null
        );

        let (status, view) = put_settings_resp(&state, r#"{"manager_enabled": true}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["manager_enabled"]["effective"], true);
        assert_eq!(view["manager_enabled"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .manager_enabled,
            Some(1),
            "on must persist a stored 1"
        );

        let (status, view) = put_settings_resp(&state, r#"{"manager_enabled": false}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["manager_enabled"]["effective"], false);
        assert_eq!(view["manager_enabled"]["source"], "stored");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .manager_enabled,
            Some(0),
            "off must persist a stored 0, never NULL"
        );
    }

    #[tokio::test]
    async fn put_settings_persists_and_clears_the_manager_profile_pin() {
        let state = test_state().await;
        let profile = agent_profile::create(&state.db, "reviewer", "claude", Some("sonnet"), None)
            .await
            .unwrap();

        let (status, view) = put_settings_resp(&state, r#"{"manager_profile": "reviewer"}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(view["manager_profile"]["effective"], "reviewer");
        assert_eq!(view["manager_profile"]["source"], "stored");

        // "" is the clear sentinel — back to « Follow the Run », persisted as
        // SQL NULL, never as an empty string that would win precedence.
        let (status, view) = put_settings_resp(&state, r#"{"manager_profile": ""}"#).await;
        assert_eq!(status, StatusCode::OK, "got {view}");
        assert_eq!(
            view["manager_profile"]["effective"],
            serde_json::Value::Null
        );
        assert_eq!(view["manager_profile"]["source"], "default");
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .manager_profile,
            None,
            "clear must persist NULL, never ''"
        );

        // Keep the profile referenced so `create`'s row is not dead code.
        assert_eq!(profile.name, "reviewer");
    }

    #[tokio::test]
    async fn put_settings_refuses_an_unknown_manager_profile() {
        // A pin to nothing would silently fall back to « Follow the Run » at
        // spawn time — refused at registration instead. (The #432 tombstone
        // only ever arises from a LATER profile deletion, which no PUT can
        // prevent.)
        let state = test_state().await;
        let (status, body) = put_settings_resp(&state, r#"{"manager_profile": "ghost"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "got {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("unknown manager profile `ghost`"),
            "got {body}"
        );
        assert_eq!(
            instance_config::get(&state.db)
                .await
                .unwrap()
                .manager_profile,
            None,
            "a refused edit must persist nothing"
        );
    }

    #[tokio::test]
    async fn put_settings_persists_and_stored_wins() {
        // Stored always wins over env/default, so these assertions are race-free.
        let state = test_state().await;
        let (status, view) = put_settings_resp(
            &state,
            r#"{"session_cap": 4, "reaper_ttl_secs": 120, "guard_timeout_secs": 30}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // PUT returns the recomputed view (D6) — no second round-trip needed.
        assert_eq!(view["session_cap"]["stored"], 4);
        assert_eq!(view["session_cap"]["source"], "stored");
        assert_eq!(view["session_cap"]["effective"], 4);
        assert_eq!(view["reaper_ttl_secs"]["stored"], 120);
        assert_eq!(view["reaper_ttl_secs"]["source"], "stored");
        assert_eq!(view["reaper_ttl_secs"]["effective"], 120);
        assert_eq!(view["guard_timeout_secs"]["stored"], 30);
        assert_eq!(view["guard_timeout_secs"]["source"], "stored");
        assert_eq!(view["guard_timeout_secs"]["effective"], 30);

        // Re-GET confirms persistence.
        let reget = get_settings_json(&state).await;
        assert_eq!(reget["session_cap"]["stored"], 4);
        assert_eq!(reget["session_cap"]["effective"], 4);
    }

    #[tokio::test]
    async fn put_settings_persists_default_model() {
        // Stored always wins over env/default, so the effective/source assertions
        // are race-free even with a concurrent env-mutating test.
        let state = test_state().await;
        let (status, view) = put_settings_resp(&state, r#"{"default_model": "opus"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_model"]["stored"], "opus");
        assert_eq!(view["default_model"]["source"], "stored");
        assert_eq!(view["default_model"]["effective"], "opus");
        assert!(view["default_model"]["default"].is_null());

        // Re-GET confirms persistence.
        let reget = get_settings_json(&state).await;
        assert_eq!(reget["default_model"]["stored"], "opus");
        assert_eq!(reget["default_model"]["effective"], "opus");
    }

    #[tokio::test]
    async fn put_settings_clears_default_model_on_empty_string() {
        // "" is the clear sentinel (D): it must NOT be rejected, and it must reset
        // the stored tier to null so the resolver falls through to env/account.
        let state = test_state().await;
        put_settings_resp(&state, r#"{"default_model": "opus"}"#).await;
        let (status, view) = put_settings_resp(&state, r#"{"default_model": ""}"#).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "empty string must be accepted, not 400"
        );
        assert!(
            view["default_model"]["stored"].is_null(),
            "empty string clears the stored model: {}",
            view["default_model"]
        );
        assert_eq!(view["default_model"]["source"], "default");
    }

    #[tokio::test]
    async fn put_settings_rejects_cap_below_one() {
        let state = test_state().await;
        let (status, body) = put_settings_resp(&state, r#"{"session_cap": 0}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].is_string(), "400 must carry an error: {body}");
        // Store untouched.
        let view = get_settings_json(&state).await;
        assert!(view["session_cap"]["stored"].is_null());
    }

    #[tokio::test]
    async fn put_settings_rejects_ttl_below_one() {
        let state = test_state().await;
        let (status, _) = put_settings_resp(&state, r#"{"reaper_ttl_secs": 0}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let view = get_settings_json(&state).await;
        assert!(view["reaper_ttl_secs"]["stored"].is_null());
    }

    #[tokio::test]
    async fn put_settings_rejects_guard_out_of_range() {
        let state = test_state().await;
        let (lo, _) = put_settings_resp(&state, r#"{"guard_timeout_secs": 0}"#).await;
        assert_eq!(lo, StatusCode::BAD_REQUEST);
        let (hi, _) = put_settings_resp(&state, r#"{"guard_timeout_secs": 601}"#).await;
        assert_eq!(hi, StatusCode::BAD_REQUEST);
        // A boundary value is accepted.
        let (ok, view) = put_settings_resp(&state, r#"{"guard_timeout_secs": 600}"#).await;
        assert_eq!(ok, StatusCode::OK);
        assert_eq!(view["guard_timeout_secs"]["stored"], 600);
    }

    #[tokio::test]
    async fn put_settings_partial_edit_leaves_other_fields() {
        let state = test_state().await;
        put_settings_resp(&state, r#"{"session_cap": 7}"#).await;
        let (_, view) = put_settings_resp(&state, r#"{"reaper_ttl_secs": 200}"#).await;
        // The second edit must not clear the cap set by the first.
        assert_eq!(view["session_cap"]["stored"], 7);
        assert_eq!(view["reaper_ttl_secs"]["stored"], 200);
    }

    /// AC2: a `PUT` carrying a retired field is a **400 naming the field**, not a 200 that
    /// silently ignores it. Both fields, and the store is untouched either way.
    #[tokio::test]
    async fn put_settings_rejects_a_retired_sandbox_image_field() {
        let state = test_state().await;
        for (field, env_var) in instance_config::RETIRED_SANDBOX_IMAGE_COLUMNS {
            let (status, body) =
                put_settings_resp(&state, &format!(r#"{{"{field}": "registry"}}"#)).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "`{field}` must be refused, not ignored: {body}"
            );
            let err = body["error"].as_str().unwrap_or_default();
            assert!(err.contains(field), "the 400 must NAME the field: {body}");
            assert!(
                err.contains("staging profile") && err.contains(env_var),
                "…and say where the knob went: {body}"
            );
        }
        // Nothing was persisted.
        let view = get_settings_json(&state).await;
        assert!(view["session_cap"]["stored"].is_null());
    }

    /// The refusal must not let a legitimate edit riding in the same body through: a `PUT` is one
    /// transaction, so a retired field poisons the whole thing rather than applying half of it.
    #[tokio::test]
    async fn a_retired_field_rejects_the_whole_put() {
        let state = test_state().await;
        let (status, _) = put_settings_resp(
            &state,
            r#"{"session_cap": 5, "image_source": "dockerfile"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let view = get_settings_json(&state).await;
        assert!(
            view["session_cap"]["stored"].is_null(),
            "the sibling edit must NOT have been applied: {view}"
        );
    }

    /// A field nobody has ever heard of stays what it always was — ignored by serde. Only the
    /// two RETIRED names get the named 400: they are the ones a real client might still send.
    #[tokio::test]
    async fn an_unknown_field_is_still_ignored_not_rejected() {
        let state = test_state().await;
        let (status, _) =
            put_settings_resp(&state, r#"{"session_cap": 5, "no_such_knob": 1}"#).await;
        assert_eq!(status, StatusCode::OK);
        let view = get_settings_json(&state).await;
        assert_eq!(view["session_cap"]["stored"], 5);
    }

    /// A malformed body is a 400 with a message, not axum's opaque 422 — the consequence of
    /// taking the body as raw JSON so the retired-field check can run first.
    #[tokio::test]
    async fn a_type_error_in_the_body_is_a_400_with_a_message() {
        let state = test_state().await;
        let (status, body) = put_settings_resp(&state, r#"{"session_cap": "many"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("invalid settings payload"),
            "{body}"
        );
    }

    /// AC5, the message half: what the boot says when a retired value is still in the row. Pure,
    /// so the wording is pinned here rather than discovered in a terminal.
    #[test]
    fn the_retired_settings_warning_names_the_values_and_where_to_go() {
        // Nothing stored ⇒ nothing said. The reference instance's case.
        assert_eq!(retired_sandbox_settings_warning(&[], Some("full")), None);

        let stored = vec![
            ("image_source", "dockerfile".to_string()),
            ("dockerfile_path", "/repo/Dockerfile".to_string()),
        ];
        let msg = retired_sandbox_settings_warning(&stored, Some("chrome")).unwrap();
        // ONE line, both values, verbatim.
        assert!(msg.contains("`image_source` = `dockerfile`"), "{msg}");
        assert!(
            msg.contains("`dockerfile_path` = `/repo/Dockerfile`"),
            "{msg}"
        );
        assert!(!msg.contains('\n'), "one line, not two: {msg}");
        // It says the values are ignored, names the profile to edit, and the env escape hatch.
        assert!(msg.contains("IGNORED"), "{msg}");
        assert!(msg.contains("`chrome`"), "the profile to edit: {msg}");
        assert!(
            msg.contains(pdo_sandbox::domain::image::IMAGE_SOURCE_ENV),
            "{msg}"
        );
        assert!(
            msg.contains(pdo_sandbox::domain::image::DOCKERFILE_PATH_ENV),
            "{msg}"
        );
        // And that no Run is affected — the whole point of it being a warning, not an error.
        assert!(msg.contains("No Run is affected"), "{msg}");

        // With no default profile (`off`), it names the screen instead of a profile.
        let msg = retired_sandbox_settings_warning(&stored, None).unwrap();
        assert!(
            msg.contains("the staging profile your Runs launch with"),
            "{msg}"
        );
    }

    /// AC5, the wiring half: the boot path reads a seeded value out of a pre-#471 row and produces
    /// exactly that message. Exercises `retired_sandbox_image_values` + the profile hint together,
    /// which is the pair the boot uses — `warn_about_retired_sandbox_settings` itself only logs.
    #[tokio::test]
    async fn a_seeded_retired_value_produces_the_boot_warning() {
        let state = test_state().await;
        // Re-add the columns a pre-#471 daemon left behind, and seed one.
        for (column, _) in instance_config::RETIRED_SANDBOX_IMAGE_COLUMNS {
            sqlx::query(&format!(
                "ALTER TABLE instance_config ADD COLUMN {column} TEXT"
            ))
            .execute(&state.db)
            .await
            .unwrap();
        }
        sqlx::query("UPDATE instance_config SET image_source = 'dockerfile' WHERE id = 1")
            .execute(&state.db)
            .await
            .unwrap();
        put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;

        let stored = instance_config::retired_sandbox_image_values(&state.db)
            .await
            .unwrap();
        assert_eq!(stored, vec![("image_source", "dockerfile".to_string())]);
        let msg = retired_sandbox_settings_warning(&stored, Some("minimal")).unwrap();
        assert!(msg.contains("`image_source` = `dockerfile`"), "{msg}");
        assert!(msg.contains("`minimal`"), "{msg}");
        // And it did NOT come back as config: the view has no such key, so no Run can read it.
        let view = get_settings_json(&state).await;
        assert!(view.get("image_source").is_none(), "{view}");
        // The logging wrapper is exercised too — it must not panic on this row.
        warn_about_retired_sandbox_settings(&state.db).await;
    }

    #[tokio::test]
    async fn put_settings_persists_default_sandbox() {
        // Stored always wins over env/default, so the effective/source assertions are
        // race-free (the `lib.rs` convention: env reads a process-global a concurrent test
        // could shadow, so only stored-wins is safe at the HTTP level).
        let state = test_state().await;
        let (status, view) = put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["default_sandbox"]["stored"], "minimal");
        assert_eq!(view["default_sandbox"]["source"], "stored");
        assert_eq!(view["default_sandbox"]["effective"], "minimal");
        assert_eq!(view["default_sandbox"]["default"], "off");

        // Re-GET confirms persistence.
        let reget = get_settings_json(&state).await;
        assert_eq!(reget["default_sandbox"]["stored"], "minimal");
        assert_eq!(reget["default_sandbox"]["effective"], "minimal");
    }

    #[tokio::test]
    async fn put_settings_clears_default_sandbox_on_empty_string() {
        // "" is the clear sentinel: accepted (not 400), resets the stored tier to null
        // so the resolver falls back to the built-in default `off`.
        let state = test_state().await;
        put_settings_resp(&state, r#"{"default_sandbox": "minimal"}"#).await;
        let (status, view) = put_settings_resp(&state, r#"{"default_sandbox": ""}"#).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "empty string must be accepted, not 400"
        );
        assert!(
            view["default_sandbox"]["stored"].is_null(),
            "empty string clears the stored default sandbox: {}",
            view["default_sandbox"]
        );
        assert_eq!(view["default_sandbox"]["source"], "default");
        assert_eq!(view["default_sandbox"]["effective"], "off");
    }

    #[tokio::test]
    async fn put_settings_rejects_invalid_default_sandbox() {
        // A non-variant token is rejected 400 before anything is persisted.
        let state = test_state().await;
        let (status, body) = put_settings_resp(&state, r#"{"default_sandbox": "vm"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].is_string(), "400 must carry an error: {body}");
        let view = get_settings_json(&state).await;
        assert!(view["default_sandbox"]["stored"].is_null());
    }

    #[tokio::test]
    async fn get_settings_surfaces_sandbox_docker_probe() {
        // The advisory probe is folded into GET /settings. `test_state` has no
        // docker_cmd_override, so the real `docker` (absent in CI) → available:false
        // with a reason. The shape must always be present regardless of the verdict.
        let state = test_state().await;
        let view = get_settings_json(&state).await;
        let sd = &view["sandbox_docker"];
        assert!(sd.get("available").is_some(), "available missing: {sd}");
        assert!(sd["available"].is_boolean(), "available must be bool: {sd}");
        assert!(
            sd.get("reason").is_some(),
            "reason key must be present: {sd}"
        );
        assert!(
            sd["checked_at"].is_string(),
            "checked_at must be a string: {sd}"
        );
    }

    #[test]
    fn the_projet_settings_store_what_the_run_context_decodes() {
        use pdo_workspace::adapters::http::ProjectSettings;
        let choice = pdo_harness::agent_choice::AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        assert_eq!(
            LegacyProjectSettings::encode_agent_choice(&choice),
            serde_json::to_string(&choice).unwrap()
        );
        let skills = LegacyProjectSettings::encode_skills(vec![
            skill_selection::SkillRef {
                id: " tdd ".into(),
                name: "TDD".into(),
            },
            skill_selection::SkillRef {
                id: "tdd".into(),
                name: "dup".into(),
            },
        ]);
        let decoded = skill_selection::from_stored_json(skills);
        assert_eq!(decoded.len(), 1, "normalised on the way in: {decoded:?}");
        assert_eq!(decoded[0].id, "tdd");
        // An empty selection is stored as none.
        assert_eq!(LegacyProjectSettings::encode_skills(vec![]), None);
    }

    /// A rename, recorded through the pipeline context's ledger port.
    async fn record_rename(db: &sqlx::SqlitePool, old: (&str, &str), new: (&str, &str)) {
        use pdo_pipeline::application::ports::{RenameLedger, Renamed};
        crate::pipeline_ports::LegacyRenameLedger { db: db.clone() }
            .record(&Renamed {
                old_id: old.0.into(),
                old_name: old.1.into(),
                new_id: new.0.into(),
                new_name: new.1.into(),
            })
            .await;
    }

    /// #890 — an absorption is instance configuration, not a property of the
    /// library: deleting the absorbed Pipeline's YAML leaves it in place, and a
    /// dimension Stats does not combine is refused.
    #[tokio::test]
    async fn stats_absorption_survives_a_library_delete_and_refuses_unknown_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "doomed");
        let state = test_state_with_dir(tmp.path()).await;
        let post = |body: serde_json::Value| {
            let state = state.clone();
            async move {
                build_router(state)
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/stats/absorptions")
                            .header("content-type", "application/json")
                            .body(Body::from(body.to_string()))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };
        assert_eq!(
            post(serde_json::json!({
                "dimension": "pipeline",
                "absorbent": {"key": "keeper", "name": "Keeper"},
                "members": [{"key": "doomed", "name": "doomed"}],
            }))
            .await,
            StatusCode::OK
        );
        assert_eq!(
            post(serde_json::json!({
                "dimension": "project",
                "absorbent": {"key": "a", "name": "A"},
                "members": [{"key": "b", "name": "B"}],
            }))
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post(serde_json::json!({
                "dimension": "pipeline",
                "absorbent": {"key": "a", "name": "A"},
                "members": [{"key": "a", "name": "A"}],
            }))
            .await,
            StatusCode::BAD_REQUEST,
            "absorbing only itself is no absorption"
        );

        // The pipeline context deletes the YAML; Stats never hears of it.
        std::fs::remove_file(tmp.path().join(".pdo/pipelines/doomed.yaml")).unwrap();

        let listed = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/stats/absorptions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["absorptions"][0]["members"][0]["key"], "doomed");
    }

    /// #891 / ADR-0077 §4 — the rename-born absorption, as the pipeline context
    /// records it through this crate's ledger: a rename that changes the id
    /// records old → new (origin `rename`), and a chain of two renames leaves one
    /// absorbent carrying both old ids. Stats then reads one row for the old and
    /// the new Pipeline in every tab; `uncombined=true` reads two. (That a
    /// display-only rename records nothing is the pipeline context's rule, its
    /// use-case test `a_display_only_rename_splices_the_name_and_records_nothing`.)
    #[tokio::test]
    async fn stats_absorption_is_recorded_by_a_pipeline_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_state_with_dir(tmp.path()).await;
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        state = with_home_override(state, home);
        let repo = state.repo_root.to_string_lossy().into_owned();

        async fn send(
            state: &Arc<AppState>,
            method: &str,
            uri: &str,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = Request::builder().method(method).uri(uri);
            if body.is_some() {
                request = request.header("content-type", "application/json");
            }
            let response = build_router(state.clone())
                .oneshot(
                    request
                        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            )
        }
        async fn completed_run(
            db: &sqlx::SqlitePool,
            repo: &str,
            run: &str,
            pipeline_id: &str,
            name: &str,
            day: &str,
        ) {
            let events = [
                (
                    "09:00",
                    "run_started",
                    None,
                    serde_json::json!({
                        "pipeline_id": pipeline_id,
                        "pipeline_name": name,
                        "target_repo": repo,
                        "harness": "claude",
                        "node_defs": [{"id":"worker","name":"Worker","node_type":"agent"}]
                    }),
                ),
                (
                    "09:01",
                    "node_started",
                    Some("worker"),
                    serde_json::json!({"node_type":"agent","harness":"claude","session_id":format!("{run}-s")}),
                ),
                (
                    "09:11",
                    "node_completed",
                    Some("worker"),
                    serde_json::json!({}),
                ),
                ("09:12", "run_completed", None, serde_json::json!({})),
            ];
            for (time, kind, node, payload) in events {
                sqlx::query(
                    "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(run)
                .bind(format!("{day}T{time}:00Z"))
                .bind(kind)
                .bind(node)
                .bind(node.map(|_| 1_i64))
                .bind(payload.to_string())
                .execute(db)
                .await
                .unwrap();
            }
        }
        let ids = |rows: &serde_json::Value| -> Vec<String> {
            let mut ids: Vec<String> = rows
                .as_array()
                .unwrap_or_else(|| panic!("not rows: {rows}"))
                .iter()
                .map(|row| row["id"].as_str().unwrap().to_string())
                .collect();
            ids.sort();
            ids
        };

        // A Run of `alpha`, then the rename that changes its id.
        completed_run(&state.db, &repo, "ren-1", "alpha", "alpha", "2033-04-10").await;
        record_rename(&state.db, ("alpha", "alpha"), ("beta", "Beta")).await;
        let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
        assert_eq!(
            list["absorptions"],
            serde_json::json!([{
                "dimension": "pipeline",
                "scope": "",
                "absorbent": {"key": "beta", "name": "Beta"},
                "members": [{
                    "key": "alpha",
                    "name": "alpha",
                    "origin": "rename",
                    "created_at": list["absorptions"][0]["members"][0]["created_at"],
                }],
            }])
        );

        // A Run of the renamed Pipeline: Stats reads one row, uncombined two.
        completed_run(&state.db, &repo, "ren-2", "beta", "Beta", "2033-04-11").await;
        let window = "from=2033-04-10T00:00:00Z&to=2033-04-12T00:00:00Z";
        for (endpoint, rows) in [
            ("overview", "sessions_by_pipeline"),
            ("cost", "by_pipeline"),
            ("performance", "by_pipeline"),
        ] {
            let uri = format!("/stats/{endpoint}?{window}&bucket=day");
            let (status, combined) = send(&state, "GET", &uri, None).await;
            assert_eq!(status, StatusCode::OK, "{endpoint}: {combined}");
            assert_eq!(ids(&combined[rows]), vec!["beta"], "{endpoint}");
            let row = &combined[rows][0];
            assert_eq!(row["name"], "Beta", "{endpoint}");
            assert_eq!(row["absorbed"][0]["key"], "alpha", "{endpoint}");
            let (_, raw) = send(&state, "GET", &format!("{uri}&uncombined=true"), None).await;
            assert_eq!(ids(&raw[rows]), vec!["alpha", "beta"], "{endpoint}");
        }
        let (_, overview) = send(
            &state,
            "GET",
            &format!("/stats/overview?{window}&bucket=day"),
            None,
        )
        .await;
        assert_eq!(overview["sessions_by_pipeline"][0]["executions"], 2);

        // A second rename: one absorbent, both old ids under it.
        record_rename(&state.db, ("beta", "Beta"), ("gamma", "Gamma")).await;
        let (_, list) = send(&state, "GET", "/stats/absorptions", None).await;
        let absorptions = list["absorptions"].as_array().unwrap();
        assert_eq!(absorptions.len(), 1, "{list}");
        assert_eq!(absorptions[0]["absorbent"]["key"], "gamma");
        assert_eq!(absorptions[0]["absorbent"]["name"], "Gamma");
        let mut members: Vec<(String, String)> = absorptions[0]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["key"].as_str().unwrap().to_string(),
                    m["origin"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        members.sort();
        assert_eq!(
            members,
            vec![
                ("alpha".to_string(), "rename".to_string()),
                ("beta".to_string(), "rename".to_string()),
            ]
        );
    }

    /// #891 — the ledger the pipeline context records a rename through (a save
    /// whose `name:` change moves the file is one) writes the absorption row.
    #[tokio::test]
    async fn the_rename_ledger_records_a_rename_born_absorption() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        record_rename(
            &state.db,
            ("saved-old", "saved-old"),
            ("saved-new", "Saved New"),
        )
        .await;
        let rows = stats_absorption::list(&state.db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].member, "saved-old");
        assert_eq!(rows[0].member_name, "saved-old");
        assert_eq!(rows[0].absorbent, "saved-new");
        assert_eq!(rows[0].absorbent_name, "Saved New");
        assert_eq!(rows[0].origin, "rename");
    }

    // --- Seeds and fixtures shared with the Run surfaces' tests, which moved to
    // `crates/pdo-run/tests/lifecycle/` (#925). ---

    /// Seed a run as `RunStarted` followed by `extra` run-level events (e.g.
    /// `RunCompleted`, `RunFailed`, `RunArchived`), with no nodes spawned. Lets
    /// the `/runs/reapable` tests construct a run in any terminal/live status.
    async fn seed_run_with_run_events(
        state: &Arc<AppState>,
        run_id: &str,
        pipeline_name: &str,
        extra: &[event_log::EventKind],
    ) {
        let mut events = vec![event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
        }];
        for kind in extra {
            events.push(event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: kind.clone(),
                node_id: None,
                iter: None,
                payload: None,
            });
        }
        for ev in &events {
            append_event(state, ev).await.unwrap();
        }
    }

    /// Create the on-disk worktree dir the daemon would have forked for a run,
    /// so the `/runs/reapable` `worktree_present` check sees it.
    fn mk_run_worktree_dir(repo_root: &std::path::Path, run_id: &str) {
        std::fs::create_dir_all(
            repo_root
                .join(".pdo")
                .join("runs")
                .join(run_id)
                .join("worktree"),
        )
        .unwrap();
    }

    async fn seed_completed_run(state: &Arc<AppState>, run_id: &str) {
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunCompleted,
                node_id: None,
                iter: None,
                payload: None,
            },
        ];
        for ev in &events {
            append_event(state, ev).await.unwrap();
        }
    }

    fn write_test_pipeline(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }
}
