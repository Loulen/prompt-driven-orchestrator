// Don't drop this lint (ADR-0039): the daemon is one crate of sibling modules, so
// a `pub` item with no consumer outside its file is a leak, and only this warning
// (with `clippy -D warnings`) keeps the crate surface from re-widening.
#![warn(unreachable_pub)]

mod contexts;
mod pipeline_ports;
mod run_ports;
mod sandbox_ports;
mod session_ports;
mod stats_ports;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::http::{header, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::Router;
use clap::{Parser, Subcommand};
use rust_embed::Embed;
use tokio::sync::broadcast;
use tokio::time;
use tracing::{error, info, warn};

use crate::contexts::ContextResourcesSeed;
pub use crate::contexts::{
    ContextResources, Contexts, PipelineResources, RunResources, SessionRunPorts,
};
pub use pdo_platform::adapters::cli::{PageAction, ServiceAction};
pub use pdo_platform::domain::service_health::ServiceHealthOverride;

// The workspace context's modules the rest of this crate reads, until each
// context migrates onto the workspace's ports.
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;

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
// Run walks, what it allows, its verdicts, the create path a Trigger fire takes.
// Every surface of a Run, every gesture on a node, the scheduler and the
// Pipeline Manager's commands live in `pdo-run`; the review of a Run (its diffs,
// its comments) in `pdo-review` (#926).
pub use pdo_review::adapters::cli::{run_review, ReviewAction};
pub use pdo_run::adapters::cli::{run_reap, run_run_create, RunAction};
use pdo_run::application::boot_recovery;
use pdo_run::application::events::append_event as append_run_event;
use pdo_run::application::paths::{dirs_next_home, instance_pipeline_dir, resolve_pipeline_path};
use pdo_run::application::pipeline_modified::relay_pipeline_modifications;
pub(crate) use pdo_run::application::sandbox as run_sandbox;
use pdo_run::application::session_claim::SessionClaim;
pub(crate) use pdo_run::application::stale::detector as stale_detector;
use pdo_run::application::stale::sweep::run_stale_detection_supervised;
use pdo_run::application::supervision::run_isolated;
#[cfg(test)]
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
    /// The `stats` context (#928): the Stats cockpit, its absorptions and the
    /// price sync, wired by the composition root. The legacy side only drives
    /// its boot refresh.
    stats: Arc<pdo_stats::Stats>,
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
    /// The settings context (#929), for its test seams.
    settings: Arc<pdo_settings::Settings>,
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
        self.state.stats.refresh_prices().execute().await;
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
        self.settings.warm_harness_catalogues().await;
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
    /// [`pdo_stats::domain::price_sync::PRICE_SOURCE_URL_DEFAULT`]. `Some(url)` points the fetch at
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
    pdo_settings::application::boot::warn_about_retired_settings(
        &pdo_settings::adapters::sqlite::SqliteInstanceConfig::new(db.clone()),
    )
    .await;

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
    let mut settings: Option<Arc<pdo_settings::Settings>> = None;
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
        price_source_url: config.price_source_url.clone(),
        price_refresh_at_boot: config.price_refresh_at_boot,
    };

    let state = Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
        let wired = wire_contexts(resources.into_resources(weak.clone()));
        contexts_router = Some(wired.router);
        triggers = Some(wired.triggers);
        settings = Some(wired.settings);
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
            stats: wired.stats,
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
    let settings = settings.context("the context wiring gave no settings context")?;

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
        let stats = state.stats.clone();
        tokio::spawn(async move {
            run_isolated("price refresh", async move {
                stats.refresh_prices().execute().await;
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
        let settings = settings.clone();
        tokio::spawn(async move {
            run_isolated("harness catalogue probe", async move {
                settings.warm_harness_catalogues().await;
            })
            .await;
        });
    }

    Ok(DaemonHandle {
        addr: bound_addr,
        task,
        state,
        triggers,
        settings,
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

/// ROUTER TRAP: any NEW top-level path prefix (`/nodes`, `/stats`, `/fs`, `/audit`,
/// `/projects`, …) also needs its own entry in the vite dev proxy whitelist
/// (`frontend/vite.config.ts`). Without it a dev-mode request answers 200 with the
/// SPA shell and any smoke test lies. Sub-paths of an already-proxied prefix are
/// free (the proxy key is a prefix), and a static segment beside a `{param}` in the
/// same subtree is fine — axum 0.8 prefers the static one.
fn build_router(state: Arc<AppState>) -> Router {
    let router = Router::new()
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

    // The stored instance settings and the agentic profiles (seeded with the
    // reserved `Default`) belong to the settings context (#929).
    pdo_settings::adapters::sqlite::init(db)
        .await
        .context("failed to create the instance settings and agent profiles tables")?;

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

    // The Banque de skills index (#668, ADR-0062). Content lives on disk under
    // `<repo_root>/.pdo/skills/<id>/`; nothing is seeded.
    pdo_skill::adapters::sqlite::init(db)
        .await
        .context("failed to create skill bank tables")?;

    // Stats absorptions (ADR-0077): instance configuration, nothing seeded.
    pdo_stats::adapters::sqlite_absorptions::init(db)
        .await
        .context("failed to create stats_absorptions table")?;

    Ok(())
}

// --- API handlers ---

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
    let instance = pdo_settings::adapters::sqlite::instance_config::get(db)
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
                update_setting: Arc::new(pdo_settings::adapters::contexts::UpdateCheckOfPlatform(
                    Arc::new(pdo_settings::adapters::sqlite::SqliteInstanceConfig::new(
                        db.clone(),
                    )),
                )),
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
            let stats = contexts::test_stats(
                &db,
                test_repo_root.clone(),
                platform.clone(),
                &bus_tx,
                workspace.clone(),
            );
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
                stats,
                allowed_ws_origins: Vec::new(),
                session,
            }
        })
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
            let stats = contexts::test_stats(
                &db,
                test_repo_root.clone(),
                platform.clone(),
                &bus_tx,
                workspace.clone(),
            );
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
                stats,
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
                axum::routing::get(|ws: axum::extract::WebSocketUpgrade| async {
                    ws.on_upgrade(|_| async {})
                }),
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
            let stats = contexts::test_stats(
                &db,
                test_repo_root.clone(),
                platform.clone(),
                &bus_tx,
                workspace.clone(),
            );
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
                stats,
                allowed_ws_origins: Vec::new(),
                session,
            }
        })
    }

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

    // Characterization net for POST /runs/{id}/commands
    //
    // Written BEFORE the dispatcher refactor and green on the unmodified tree.
    // These do not describe a contract anyone designed — they RECORD what HEAD
    // actually answers, so a "pure move" that quietly changes a status, a
    // content-type or a side effect goes red instead of shipping. Every value
    // below was observed, never deduced: the surface asserts no content-type
    // anywhere else in the repo, and the unknown-run behaviour is a three-way
    // split that no test covered.

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
}
