pub mod admission;
mod blackboard;
#[allow(dead_code)]
mod condition;
#[allow(dead_code)]
mod cron_schedule;
mod edge_router;
mod event_log;
#[allow(dead_code)]
mod fire_decision;
mod frontmatter_parser;
pub mod graph_resolver;
mod guard_runner;
mod input_resolution;
pub mod library_store;
#[allow(dead_code)]
mod loop_region;
#[allow(dead_code)]
mod merge_action;
mod mutation_validator;
mod node_io_resolver;
pub mod node_primitives;
mod outputs_validator;
mod pipeline;
pub mod pipeline_migrator;
mod pipeline_watcher;
mod prompt_augmenter;
mod pty_bridge;
#[allow(dead_code)]
mod scheduler;
mod scheduler_dispatcher;
pub mod stale_detector;
mod switch_router;
pub mod tmux_session_manager;
pub mod transition_guard;
#[allow(dead_code)]
mod trigger_scheduler;
#[allow(dead_code)]
mod trigger_store;
#[allow(dead_code)]
mod variable_resolver;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{
    FromRequest, Json, Multipart, Path as AxumPath, Query, State, WebSocketUpgrade,
};
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use clap::{Parser, Subcommand};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time;
use tracing::{error, info, warn};

const DEFAULT_PORT: u16 = 5172;
const DEFAULT_DAEMON_URL: &str = "http://localhost:5172";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

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
        #[arg(short, long, env = "PDO_PORT", default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Signal that the current NodeRun has completed successfully
    Complete,
    /// Signal that the current NodeRun has failed
    Fail {
        #[arg(long)]
        reason: String,
    },
}

struct AppState {
    db: sqlx::SqlitePool,
    event_tx: broadcast::Sender<event_log::Event>,
    pipeline_tx: broadcast::Sender<serde_json::Value>,
    repo_root: PathBuf,
    port: u16,
    merge_lock: tokio::sync::Mutex<()>,
    /// Serializes admission so the slot check is atomic check-and-reserve
    /// (#213). A spawn holds this from the moment it counts live sessions until
    /// it has appended the reservation event (`NodeStarted` / `NodeWaiting`),
    /// after which the projected state already reflects the new session and the
    /// next spawn's count sees it. Without it, concurrent spawns (retries of
    /// `waiting` nodes across runs) can all observe the same free slot and
    /// overshoot the cap.
    admission_lock: tokio::sync::Mutex<()>,
    /// Serializes Trigger scheduler ticks. A tick is read-decide-write
    /// (`due_triggers` → guard subprocess → `set_next_fire`) with an await on
    /// the guard in the middle; two concurrent ticks (the 30 s background loop
    /// and the `run_trigger_tick` test seam) can otherwise both see the same
    /// Trigger as due and double-fire it.
    trigger_tick_lock: tokio::sync::Mutex<()>,
    /// Paths the daemon has just written. The pipeline watcher consults this map
    /// and suppresses `pipeline_changed` broadcasts for paths it sees within the
    /// TTL window — that's how we tell our own writes apart from external ones
    /// (vim, git checkout, future Pipeline Manager) without ignoring the latter.
    recent_writes: Arc<Mutex<HashMap<PathBuf, Instant>>>,
    /// Live handle to the pipeline file watcher. Run dirs are watched
    /// individually and non-recursively (see `pipeline_watcher::watch_run_dir`),
    /// so run creation must register each new run dir here.
    run_watcher: Arc<Mutex<Option<pipeline_watcher::PipelineDebouncer>>>,
    /// Per-daemon override for the `claude …` tail of spawned tmux scripts.
    /// `None` in production (real claude); `Some(cmd)` in tests, seeded via
    /// [`DaemonConfig`] by `TestDaemon::spawn`, so no test ever launches real
    /// claude and no `std::env::set_var` race can clobber it (#181).
    tmux_cmd_override: Option<String>,
}

impl AppState {
    /// Tmux socket name (`-L <name>`) scoped to this daemon's port.
    /// Every tmux call from this daemon goes through this socket.
    fn tmux_socket(&self) -> String {
        tmux_session_manager::tmux_socket_name(self.port)
    }
}

/// TTL during which a recorded self-write suppresses watcher broadcasts.
/// Larger than the debouncer interval (1s) with margin for filesystem latency.
pub(crate) const SELF_WRITE_TTL: Duration = Duration::from_secs(2);

/// Entries older than this are GCed when a new write is recorded.
const SELF_WRITE_GC: Duration = Duration::from_secs(5);

/// Record a path the daemon is about to write so the file watcher can skip its
/// own broadcast when notify reports the change. Call this *before* `std::fs::write`
/// — the watcher otherwise races against the insert.
fn mark_self_write(map: &Mutex<HashMap<PathBuf, Instant>>, path: &Path) {
    let now = Instant::now();
    let mut guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.retain(|_, t| now.duration_since(*t) < SELF_WRITE_GC);
    guard.insert(path.to_path_buf(), now);
}

#[derive(Deserialize)]
struct CreateRunRequest {
    pipeline: String,
    input: String,
    #[serde(default)]
    variables: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pipeline_id: Option<String>,
    #[serde(default)]
    target_repo: Option<String>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// Provenance: the id of the Trigger that created this Run, if any. Set by
    /// the trigger scheduler; absent for manual runs.
    #[serde(default)]
    triggered_by: Option<String>,
}

#[derive(Serialize)]
struct CreateRunResponse {
    run_id: String,
}

#[derive(Serialize)]
struct RunListEntry {
    run_id: String,
    pipeline_name: String,
    status: event_log::RunStatus,
    /// Display-only "no forward progress" overlay (#180): true when the run has
    /// no running/waiting node and a stale node, so its dot renders amber even
    /// though `status` stays `running`. Derived per read by `event_log::is_stalled`.
    stalled: bool,
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Provenance: the Trigger that created this Run, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    triggered_by: Option<String>,
}

#[derive(Serialize)]
struct PipelineVariableInfo {
    var_type: pipeline::VariableType,
    default: serde_json::Value,
}

#[derive(Deserialize)]
struct NodeDoneRequest {
    #[serde(default)]
    iter: Option<i64>,
}

#[derive(Deserialize)]
struct NodeFailRequest {
    reason: String,
    #[serde(default)]
    iter: Option<i64>,
}

#[derive(Deserialize)]
struct RunCommandRequest {
    kind: String,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    iter: Option<i64>,
    #[serde(default)]
    additional_iter: Option<i64>,
    /// Identifies the loop region a `bump_region` / `end_region` command targets
    /// (ADR-0011 / #152 — the Pipeline Manager routes a region by id).
    #[serde(default)]
    region_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct IterQuery {
    #[serde(default = "default_iter")]
    iter: i64,
}

fn default_iter() -> i64 {
    1
}

/// Optional `?scope=` qualifier for the `/pipelines/{id}` open/save/delete
/// routes. When present it pins the operation to a single store so it can never
/// silently fall through to a same-named file in a *different* store (#216).
/// `library` routes to the disk-first library store; `repo`/`user` resolve
/// strictly to that store; absent keeps the historical repo-then-user default.
#[derive(Deserialize)]
struct ScopeQuery {
    scope: Option<String>,
}

fn cli_daemon_url() -> String {
    std::env::var("PDO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string())
}

fn cli_run_id() -> Result<String> {
    std::env::var("PDO_RUN_ID")
        .context("PDO_RUN_ID not set — this command must be run inside a PDO NodeRun session")
}

fn cli_node_id() -> Result<String> {
    std::env::var("PDO_NODE_ID")
        .context("PDO_NODE_ID not set — this command must be run inside a PDO NodeRun session")
}

fn cli_node_iter() -> i64 {
    std::env::var("PDO_NODE_ITER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

pub fn run_complete() -> Result<()> {
    let url = cli_daemon_url();
    let rid = cli_run_id()?;
    let nid = cli_node_id()?;
    let iter = cli_node_iter();

    let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/done");
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&endpoint)
        .json(&serde_json::json!({ "iter": iter }))
        .send()
        .context("failed to reach daemon")?;

    if resp.status().is_success() {
        eprintln!("Node {nid} marked complete.");
    } else {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {body}");
    }
    Ok(())
}

pub fn run_fail(reason: String) -> Result<()> {
    let url = cli_daemon_url();
    let rid = cli_run_id()?;
    let nid = cli_node_id()?;
    let iter = cli_node_iter();

    let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/fail");
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&endpoint)
        .json(&serde_json::json!({ "reason": reason, "iter": iter }))
        .send()
        .context("failed to reach daemon")?;

    if resp.status().is_success() {
        eprintln!("Node {nid} marked failed.");
    } else {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {body}");
    }
    Ok(())
}

pub struct DaemonHandle {
    pub addr: SocketAddr,
    pub task: tokio::task::JoinHandle<Result<()>>,
    state: Arc<AppState>,
}

impl DaemonHandle {
    /// Run a single Trigger scheduler tick synchronously. Lets integration
    /// tests drive firing deterministically instead of waiting for the ~30 s
    /// background interval.
    pub async fn run_trigger_tick(&self) {
        run_trigger_scheduler_tick(&self.state).await;
    }

    /// Run a single stale-detection sweep synchronously (#213). Lets
    /// integration tests drive liveness detection (dead session -> node Failed)
    /// deterministically instead of waiting for the ~30 s background interval.
    pub async fn run_stale_detection_tick(&self) {
        run_stale_detection(&self.state).await;
    }

    /// Run the boot-recovery reconciliation pass synchronously (#213). The
    /// daemon runs this once at startup; the seam lets integration tests drive
    /// it deterministically (orphaned Running node -> Failed at boot).
    pub async fn run_boot_recovery_tick(&self) {
        run_boot_recovery(&self.state).await;
    }

    /// Force a Trigger's next fire into the past so the next
    /// [`Self::run_trigger_tick`] treats it as due. Test seam only — production
    /// next-fire times come from the cron schedule.
    pub async fn force_trigger_due(&self, trigger_id: &str) {
        let _ = trigger_store::set_next_fire(
            &self.state.db,
            trigger_id,
            Some("2020-01-01T00:00:00.000Z"),
        )
        .await;
    }
}

/// Boot-time configuration for a daemon instance.
///
/// Carries the knobs that must be decided *per daemon* rather than read from
/// process-global env in the hot path. Production builds this from the
/// environment ([`DaemonConfig::from_env`]); tests construct it directly so two
/// daemons in the same test process can't race on a shared env var (#181).
#[derive(Debug, Clone, Default)]
pub struct DaemonConfig {
    /// Replaces the `claude …` tail in spawned tmux scripts when `Some`. Tests
    /// set this (e.g. `Some("exec sleep 600")`) so node sessions run a harmless
    /// command instead of launching a real `claude` process. `None` →
    /// production default (real claude).
    pub tmux_cmd_override: Option<String>,
}

impl DaemonConfig {
    /// Build config from the environment — the production / CLI daemon path.
    ///
    /// Reads [`tmux_session_manager::TMUX_CMD_OVERRIDE_ENV`] exactly once, here
    /// at boot, preserving the documented env seam for an operator launching a
    /// real `pdo daemon` while keeping that read out of the spawn hot path.
    pub fn from_env() -> Self {
        Self {
            tmux_cmd_override: std::env::var(tmux_session_manager::TMUX_CMD_OVERRIDE_ENV).ok(),
        }
    }
}

pub async fn serve(addr: SocketAddr, repo_root: PathBuf) -> Result<DaemonHandle> {
    serve_with_config(addr, repo_root, DaemonConfig::from_env()).await
}

/// Like [`serve`], but takes an explicit [`DaemonConfig`] instead of reading the
/// environment. Tests use this to seed a per-daemon `tmux_cmd_override` without
/// mutating process-global env.
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

    let (event_tx, _) = broadcast::channel::<event_log::Event>(256);
    let (pipeline_tx, _) = broadcast::channel::<serde_json::Value>(64);

    let recent_writes: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

    let (run_modified_tx, run_modified_rx) =
        tokio::sync::mpsc::unbounded_channel::<pipeline_watcher::RunPipelineModified>();

    let watcher = pipeline_watcher::spawn_watcher(
        repo_root.clone(),
        pipeline_tx.clone(),
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

    let state = Arc::new(AppState {
        db,
        event_tx,
        pipeline_tx,
        repo_root,
        port: bound_addr.port(),
        merge_lock: tokio::sync::Mutex::new(()),
        admission_lock: tokio::sync::Mutex::new(()),
        trigger_tick_lock: tokio::sync::Mutex::new(()),
        recent_writes,
        run_watcher: run_watcher.clone(),
        tmux_cmd_override: config.tmux_cmd_override,
    });

    // The orphan sweep — and every other tmux call this daemon makes —
    // is scoped to its own private socket (`pdo-<port>`) so we can
    // never reach into another daemon's tmux state on the same host.
    //
    // If we detect that we were spawned from inside a PDO pipeline
    // (sub-claude context exports `PDO_NODE_ID` via wrap_with_env),
    // or the operator set `PDO_DAEMON_NO_CLEANUP=1`, suppress every
    // cleanup pathway. A Tester or Implementer that accidentally runs
    // `pdo daemon` then can't trigger reaper-based kills, even on
    // its own socket. The daemon still serves HTTP and accepts
    // explicit `cleanup_run` calls — only the *automatic* sweeps go away.
    let nested_daemon = std::env::var("PDO_NODE_ID").is_ok()
        || std::env::var("PDO_DAEMON_NO_CLEANUP")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);

    if nested_daemon {
        warn!(
            "PDO daemon launched from a sub-claude context \
             (PDO_NODE_ID or PDO_DAEMON_NO_CLEANUP set) — \
             skipping boot-time orphan sweep and periodic reaper. \
             This daemon will not auto-reap any tmux sessions."
        );
    } else {
        if let Err(e) = run_orphan_sweep(
            &state.db,
            &state.tmux_socket(),
            tmux_session_manager::reaper_ttl(),
        )
        .await
        {
            warn!("Orphan sweep at boot failed: {e}");
        }

        // Boot recovery (#213): reconcile persisted run state against the live
        // process world — fail-fast on orphaned Running nodes and surface
        // git/event-log divergence. Suppressed for a nested daemon (it stays
        // passive on tmux state, same as the sweep/reaper).
        run_boot_recovery(&state).await;
    }

    let app = build_router(state.clone());

    info!("PDO daemon listening on http://{bound_addr}");

    // Spawn reaper background task — unless we're a nested daemon, in
    // which case we stay completely passive on tmux state.
    let _reaper_handle = if nested_daemon {
        None
    } else {
        let reaper_state = state.clone();
        Some(tokio::spawn(async move {
            let interval = tmux_session_manager::reaper_interval();
            let ttl = tmux_session_manager::reaper_ttl();
            let socket = reaper_state.tmux_socket();
            let mut tick = time::interval(interval);
            loop {
                tick.tick().await;
                if let Err(e) = run_orphan_sweep(&reaper_state.db, &socket, ttl).await {
                    warn!("Reaper sweep failed: {e}");
                }
            }
        }))
    };

    // Background task: process run-scoped pipeline modifications
    let mod_state = state.clone();
    let _run_modified_handle = tokio::spawn(async move {
        handle_run_pipeline_modifications(mod_state, run_modified_rx).await;
    });

    // Background task: stale detection (dead sessions + idle agents)
    let _stale_handle = if nested_daemon {
        None
    } else {
        let stale_state = state.clone();
        Some(tokio::spawn(async move {
            let mut tick = time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                run_stale_detection(&stale_state).await;
            }
        }))
    };

    // Background task: Trigger scheduler (sibling of reaper/stale). Fires due
    // Triggers on a ~30s tick. Best-effort: only runs while the daemon lives,
    // forward-only (no backfill of missed slots). Suppressed in a nested daemon
    // for the same reason the reaper is — a sub-claude must stay passive.
    let _trigger_handle = if nested_daemon {
        None
    } else {
        let trigger_state = state.clone();
        Some(tokio::spawn(async move {
            let mut tick =
                time::interval(Duration::from_secs(trigger_scheduler::TICK_INTERVAL_SECS));
            loop {
                tick.tick().await;
                run_trigger_scheduler_tick(&trigger_state).await;
            }
        }))
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

    Ok(DaemonHandle {
        addr: bound_addr,
        task,
        state,
    })
}

pub async fn run_daemon(port: u16) -> Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let repo_root = std::env::current_dir().context("failed to determine current directory")?;
    let handle = serve(addr, repo_root).await?;
    handle.task.await.context("daemon task join error")?
}

// --- Repo endpoints ---

#[derive(Deserialize)]
struct RepoPathQuery {
    path: String,
}

async fn repos_branches(Query(q): Query<RepoPathQuery>) -> Response {
    let repo = match validate_target_repo(&q.path) {
        Ok(p) => p,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };
    match list_branches(&repo) {
        Ok(branches) => Json(branches).into_response(),
        Err(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
    }
}

async fn repos_validate(Query(q): Query<RepoPathQuery>) -> Response {
    match validate_target_repo(&q.path) {
        Ok(_) => Json(serde_json::json!({ "valid": true })).into_response(),
        Err(msg) => Json(serde_json::json!({ "valid": false, "error": msg })).into_response(),
    }
}

async fn repos_recent(State(state): State<Arc<AppState>>) -> Response {
    let rows: Result<Vec<(String,)>, _> = sqlx::query_as(
        "SELECT payload FROM events \
         WHERE kind = 'run_started' AND payload IS NOT NULL \
         ORDER BY ts DESC",
    )
    .fetch_all(&state.db)
    .await;

    match rows {
        Ok(rows) => {
            let mut seen = std::collections::HashSet::new();
            let mut repos = Vec::new();
            for (payload_str,) in &rows {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload_str) {
                    if let Some(repo) = val["target_repo"].as_str() {
                        if seen.insert(repo.to_string()) {
                            repos.push(repo.to_string());
                            if repos.len() >= 5 {
                                break;
                            }
                        }
                    }
                }
            }
            Json(repos).into_response()
        }
        Err(e) => {
            error!("failed to query recent repos: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to query recent repos" })),
            )
                .into_response()
        }
    }
}

// --- Trigger endpoints ---

#[derive(Deserialize)]
struct CreateTriggerRequest {
    name: String,
    /// Library pipeline id the Trigger fires.
    pipeline_id: String,
    #[serde(default)]
    target_repo: Option<String>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    input_template: String,
    #[serde(default)]
    variables: HashMap<String, serde_yaml::Value>,
    /// 5-field cron expression.
    cron: String,
    #[serde(default)]
    guard_command: Option<String>,
    #[serde(default = "default_overlap_policy")]
    overlap_policy: String,
}

fn default_overlap_policy() -> String {
    "skip".to_string()
}

async fn create_trigger(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTriggerRequest>,
) -> Response {
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "trigger name must not be empty" })),
        )
            .into_response();
    }

    // Validate the cron expression up front (Sharp tool: fail loud at config
    // time, not at 3am).
    let schedule = match cron_schedule::CronSchedule::parse(&req.cron) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid cron expression: {e}") })),
            )
                .into_response();
        }
    };

    // Resolve the target pipeline and its prompt_required flag.
    let yaml =
        library_store::pipelines::get_yaml(&state.repo_root, &req.pipeline_id).or_else(|| {
            std::fs::read_to_string(resolve_pipeline_path(&state.repo_root, &req.pipeline_id)).ok()
        });
    let yaml = match yaml {
        Some(y) => y,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("pipeline not found: {}", req.pipeline_id)
                })),
            )
                .into_response();
        }
    };
    let (pipeline_name, prompt_required) = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => (r.pipeline.name.clone(), r.pipeline.prompt_required),
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("pipeline parse error: {e}") })),
            )
                .into_response();
        }
    };

    // Server-side fire_decision reject rule: an empty resolvable input on a
    // prompt-required pipeline (cron-only: no guard, so the only source is the
    // input template) is a misconfiguration — refuse at creation.
    if req.guard_command.as_deref().unwrap_or("").trim().is_empty()
        && req.input_template.trim().is_empty()
        && prompt_required
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "this pipeline requires a prompt; add a guard, an input \
                          template, or mark the pipeline prompt-not-required"
            })),
        )
            .into_response();
    }

    // Validate target repo if provided.
    if let Some(ref repo) = req.target_repo {
        if let Err(msg) = validate_target_repo(repo) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }

    let next_fire_at = schedule
        .next_fire_after(chrono::Local::now())
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

    let variables = serde_json::to_value(&req.variables).unwrap_or(serde_json::json!({}));

    let new = trigger_store::NewTrigger {
        name: req.name,
        pipeline_id: req.pipeline_id,
        pipeline_name,
        target_repo: req.target_repo,
        source_branch: req.source_branch,
        input_template: req.input_template,
        variables,
        cron: req.cron,
        guard_command: req.guard_command.filter(|g| !g.trim().is_empty()),
        overlap_policy: if req.overlap_policy == "allow" {
            "allow".to_string()
        } else {
            "skip".to_string()
        },
        next_fire_at,
    };

    match trigger_store::create(&state.db, new).await {
        Ok(trigger) => {
            let _ = state.pipeline_tx.send(serde_json::json!({
                "type": "trigger_created",
                "trigger_id": trigger.id,
            }));
            (StatusCode::CREATED, Json(trigger)).into_response()
        }
        Err(e) => {
            error!("failed to create trigger: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to persist trigger" })),
            )
                .into_response()
        }
    }
}

async fn list_triggers(State(state): State<Arc<AppState>>) -> Response {
    match trigger_store::list(&state.db).await {
        Ok(triggers) => Json(triggers).into_response(),
        Err(e) => {
            error!("failed to list triggers: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to list triggers" })),
            )
                .into_response()
        }
    }
}

async fn delete_trigger(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
) -> Response {
    match trigger_store::delete(&state.db, &trigger_id).await {
        Ok(true) => {
            let _ = state.pipeline_tx.send(serde_json::json!({
                "type": "trigger_deleted",
                "trigger_id": trigger_id,
            }));
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "trigger not found" })),
        )
            .into_response(),
        Err(e) => {
            error!("failed to delete trigger: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to delete trigger" })),
            )
                .into_response()
        }
    }
}

async fn get_trigger(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
) -> Response {
    match trigger_store::get(&state.db, &trigger_id).await {
        Ok(Some(trigger)) => Json(trigger).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "trigger not found" })),
        )
            .into_response(),
        Err(e) => {
            error!("failed to fetch trigger: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to fetch trigger" })),
            )
                .into_response()
        }
    }
}

/// A partial Trigger edit (#162). Every field is optional; absent fields are
/// left untouched. `enabled` toggles activation; the config fields cover the
/// schedule, input template, and overlap policy per the acceptance criteria,
/// plus name/repo/branch/guard/variables for completeness.
#[derive(Deserialize)]
struct PatchTriggerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    input_template: Option<String>,
    #[serde(default)]
    overlap_policy: Option<String>,
    #[serde(default)]
    target_repo: Option<Option<String>>,
    #[serde(default)]
    source_branch: Option<Option<String>>,
    #[serde(default)]
    guard_command: Option<Option<String>>,
    #[serde(default)]
    variables: Option<HashMap<String, serde_yaml::Value>>,
}

async fn patch_trigger(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
    Json(req): Json<PatchTriggerRequest>,
) -> Response {
    // The Trigger must exist.
    let existing = match trigger_store::get(&state.db, &trigger_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "trigger not found" })),
            )
                .into_response();
        }
        Err(e) => {
            error!("failed to load trigger for patch: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to load trigger" })),
            )
                .into_response();
        }
    };

    // Name, when supplied, must not be blank (Sharp tool: fail at edit time).
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "trigger name must not be empty" })),
            )
                .into_response();
        }
    }

    // A schedule edit re-validates the cron and recomputes the next fire forward.
    let mut next_fire_at: Option<Option<String>> = None;
    if let Some(ref cron) = req.cron {
        match cron_schedule::CronSchedule::parse(cron) {
            Ok(schedule) => {
                let next = schedule
                    .next_fire_after(chrono::Local::now())
                    .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                next_fire_at = Some(next);
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid cron expression: {e}") })),
                )
                    .into_response();
            }
        }
    }

    // Validate a target-repo edit (Some(Some(path)) means set; Some(None) clears).
    if let Some(Some(ref repo)) = req.target_repo {
        if let Err(msg) = validate_target_repo(repo) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }

    // Re-apply the fire_decision reject rule against the *resulting* config: if
    // the pipeline requires a prompt and the edit would leave neither a guard
    // nor an input template, refuse (mirrors create-time validation).
    let resulting_guard = match &req.guard_command {
        Some(g) => g.clone(),
        None => existing.guard_command.clone(),
    };
    let resulting_input = req
        .input_template
        .clone()
        .unwrap_or_else(|| existing.input_template.clone());
    let prompt_required = trigger_prompt_required(&state, &existing);
    if prompt_required
        && resulting_guard.as_deref().unwrap_or("").trim().is_empty()
        && resulting_input.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "this pipeline requires a prompt; add a guard, an input \
                          template, or mark the pipeline prompt-not-required"
            })),
        )
            .into_response();
    }

    let edit = trigger_store::UpdateTrigger {
        name: req.name,
        target_repo: req.target_repo,
        source_branch: req.source_branch,
        input_template: req.input_template,
        variables: req
            .variables
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::json!({}))),
        cron: req.cron,
        guard_command: req.guard_command,
        overlap_policy: req.overlap_policy.map(|p| {
            if p == "allow" {
                "allow".to_string()
            } else {
                "skip".to_string()
            }
        }),
        next_fire_at,
    };

    if let Err(e) = trigger_store::update(&state.db, &trigger_id, edit).await {
        error!("failed to update trigger: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "failed to update trigger" })),
        )
            .into_response();
    }

    // Enable/disable is a dedicated column toggle.
    if let Some(enabled) = req.enabled {
        if let Err(e) = trigger_store::set_enabled(&state.db, &trigger_id, enabled).await {
            error!("failed to toggle trigger enabled: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update trigger" })),
            )
                .into_response();
        }
    }

    match trigger_store::get(&state.db, &trigger_id).await {
        Ok(Some(updated)) => {
            let _ = state.pipeline_tx.send(serde_json::json!({
                "type": "trigger_updated",
                "trigger_id": trigger_id,
            }));
            Json(updated).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "failed to reload trigger" })),
        )
            .into_response(),
    }
}

async fn list_trigger_fires(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
) -> Response {
    match trigger_store::fire_history(&state.db, &trigger_id).await {
        Ok(fires) => Json(fires).into_response(),
        Err(e) => {
            error!("failed to list trigger fires: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to list trigger fires" })),
            )
                .into_response()
        }
    }
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/pipelines", get(list_pipelines))
        .route("/pipelines/{pipeline_id}", get(get_pipeline))
        .route(
            "/pipelines/{pipeline_id}",
            axum::routing::put(save_pipeline).delete(delete_pipeline),
        )
        .route("/pipelines", post(create_pipeline))
        .route("/runs", post(create_run))
        .route("/runs", get(list_runs))
        .route("/sessions", get(sessions))
        .route("/runs/{run_id}", get(get_run).delete(forget_run))
        .route("/runs/{run_id}/events", get(get_run_events))
        .route("/runs/{run_id}/nodes/{node_id}/done", post(node_done))
        .route("/runs/{run_id}/nodes/{node_id}/fail", post(node_fail))
        .route("/runs/{run_id}/nodes/{node_id}/pane", get(node_pane))
        .route("/runs/{run_id}/nodes/{node_id}/prompt", get(node_prompt))
        .route("/runs/{run_id}/nodes/{node_id}/io", get(node_io))
        .route("/runs/{run_id}/diff", get(run_diff))
        .route("/runs/{run_id}/nodes/{node_id}/diff", get(node_diff))
        .route("/runs/{run_id}/artifact", get(artifact))
        .route("/runs/{run_id}/pipeline", get(get_run_pipeline))
        .route(
            "/runs/{run_id}/pipeline",
            axum::routing::put(save_run_pipeline),
        )
        .route("/runs/{run_id}/commands", post(run_command))
        .route("/runs/{run_id}/nodes/{node_id}/start", post(node_start))
        .route("/runs/{run_id}/nodes/{node_id}/stop", post(node_stop))
        .route("/runs/{run_id}/nodes/{node_id}/retry", post(node_retry))
        .route(
            "/runs/{run_id}/nodes/{node_id}/retry/preview",
            get(node_retry_preview),
        )
        .route(
            "/sessions/{session_id}/pty",
            get(pty_bridge::session_pty_handler),
        )
        .route("/sessions/{session_id}/attach", post(session_attach))
        .route("/sessions/{run_id}/manager/attach", post(manager_attach))
        .route("/library", get(list_library))
        .route("/library", post(save_to_library))
        .route(
            "/library/{name}",
            axum::routing::delete(delete_from_library),
        )
        .route(
            "/library/{name}/instantiate",
            post(instantiate_from_library),
        )
        .route("/library/pipelines", get(list_library_pipelines))
        .route("/library/pipelines", post(save_library_pipeline))
        .route(
            "/library/pipelines/{id}",
            axum::routing::delete(delete_library_pipeline),
        )
        .route("/pipelines/{pipeline_id}/promote", post(promote_pipeline))
        .route("/repos/branches", get(repos_branches))
        .route("/repos/validate", get(repos_validate))
        .route("/repos/recent", get(repos_recent))
        .route("/triggers", get(list_triggers).post(create_trigger))
        .route(
            "/triggers/{trigger_id}",
            get(get_trigger).patch(patch_trigger).delete(delete_trigger),
        )
        .route("/triggers/{trigger_id}/fires", get(list_trigger_fires))
        .fallback(static_handler)
        .with_state(state)
}

async fn init_db(db: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,
            node_id TEXT,
            iter INTEGER,
            payload JSON
        )",
    )
    .execute(db)
    .await
    .context("failed to create events table")?;

    trigger_store::init(db)
        .await
        .context("failed to create trigger tables")?;

    Ok(())
}

async fn append_event(state: &AppState, event: &event_log::Event) -> Result<()> {
    // Transition guard backstop (#212): every lifecycle append is validated
    // against the freshly projected state, so no emitter — present or future —
    // can bypass the guard. Emitters with side effects (spawn, merge) must
    // ALSO pre-validate before acting; this backstop only protects the log.
    if matches!(
        event.kind,
        event_log::EventKind::NodeStarted
            | event_log::EventKind::NodeWaiting
            | event_log::EventKind::NodeCompleted
            | event_log::EventKind::NodeAutoCompleted
            | event_log::EventKind::NodeStale
            | event_log::EventKind::NodeFailed
    ) {
        let events = load_events(&state.db, &event.run_id).await?;
        let run_state = event_log::project(&events);
        match transition_guard::validate_transition(run_state.as_ref(), event) {
            transition_guard::Verdict::Allow => {}
            transition_guard::Verdict::NoOp { reason } => {
                info!("append_event no-op ({:?}): {reason}", event.kind);
                return Ok(());
            }
            transition_guard::Verdict::Reject { reason } => {
                anyhow::bail!("transition rejected ({:?}): {reason}", event.kind);
            }
        }
    }

    let kind_str = serde_json::to_value(&event.kind)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let payload_str = event.payload.as_ref().map(|p| p.to_string());

    sqlx::query(
        "INSERT INTO events (run_id, ts, kind, node_id, iter, payload) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.run_id)
    .bind(&event.ts)
    .bind(&kind_str)
    .bind(&event.node_id)
    .bind(event.iter)
    .bind(&payload_str)
    .execute(&state.db)
    .await
    .context("failed to append event")?;

    let _ = state.event_tx.send(event.clone());

    Ok(())
}

async fn emit_run_event(
    state: &AppState,
    run_id: &str,
    kind: event_log::EventKind,
    payload: Option<serde_json::Value>,
) {
    let event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: None,
        iter: None,
        payload,
    };
    if let Err(e) = append_event(state, &event).await {
        error!("failed to append {:?}: {e}", event.kind);
    }
}

async fn emit_loop_action(state: &AppState, run_id: &str, action: &scheduler::SchedulerAction) {
    match action {
        scheduler::SchedulerAction::LoopIterStarted {
            loop_node_id,
            iter,
            max_iter,
        } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::LoopIterStarted,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                    "iter": iter,
                    "max_iter": max_iter,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopBreakReceived { loop_node_id } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::LoopBreakReceived,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopMaxReached {
            loop_node_id,
            max_iter,
        } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::LoopMaxReached,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                    "max_iter": max_iter,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::LoopDone { loop_node_id } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::LoopDone,
                Some(serde_json::json!({
                    "loop_node_id": loop_node_id,
                })),
            )
            .await;
        }
        _ => {}
    }
}

async fn emit_foreach_action(state: &AppState, run_id: &str, action: &scheduler::SchedulerAction) {
    match action {
        scheduler::SchedulerAction::ForEachStarted {
            foreach_node_id,
            total_items,
            ..
        } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::ForEachStarted,
                Some(serde_json::json!({
                    "foreach_node_id": foreach_node_id,
                    "total_items": total_items,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::ForEachEmpty { foreach_node_id } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::ForEachEmpty,
                Some(serde_json::json!({
                    "foreach_node_id": foreach_node_id,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::ForEachBreakReceived { foreach_node_id } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::ForEachBreakReceived,
                Some(serde_json::json!({
                    "foreach_node_id": foreach_node_id,
                })),
            )
            .await;
        }
        scheduler::SchedulerAction::ForEachDone { foreach_node_id } => {
            emit_run_event(
                state,
                run_id,
                event_log::EventKind::ForEachDone,
                Some(serde_json::json!({
                    "foreach_node_id": foreach_node_id,
                })),
            )
            .await;
        }
        _ => {}
    }
}

async fn load_events(db: &sqlx::SqlitePool, run_id: &str) -> Result<Vec<event_log::Event>> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT id, run_id, ts, kind, node_id, iter, payload FROM events WHERE run_id = ? ORDER BY id",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
    .context("failed to load events")?;

    Ok(rows.into_iter().map(|r| r.into_event()).collect())
}

async fn load_all_run_ids(db: &sqlx::SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT run_id FROM events ORDER BY run_id DESC")
            .fetch_all(db)
            .await
            .context("failed to load run ids")?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Count the live NodeRun sessions across *all* runs (admission control, #159).
///
/// Projects every run from the event log and delegates the count to
/// [`admission::count_live_node_sessions`]. Pipeline Manager sessions are not
/// nodes, so they are excluded by construction.
async fn count_global_live_sessions(db: &sqlx::SqlitePool) -> usize {
    let run_ids = match load_all_run_ids(db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("admission: failed to load run ids: {e}");
            return 0;
        }
    };
    let mut states = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        if let Ok(events) = load_events(db, &run_id).await {
            if let Some(state) = event_log::project(&events) {
                states.push(state);
            }
        }
    }
    admission::count_live_node_sessions(states.iter())
}

// --- Trigger scheduler ---

/// Whether the Trigger's *own* previous Run is still live. Scans projected Run
/// state for a run carrying this `triggered_by` whose status is live.
async fn trigger_has_live_run(db: &sqlx::SqlitePool, trigger_id: &str) -> bool {
    let run_ids = match load_all_run_ids(db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("trigger scheduler: failed to load run ids: {e}");
            return false;
        }
    };
    for run_id in run_ids {
        let events = match load_events(db, &run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(state) = event_log::project(&events) {
            if state.triggered_by.as_deref() == Some(trigger_id) && state.status.is_live() {
                return true;
            }
        }
    }
    false
}

/// Load a Trigger's target pipeline yaml from the library (falling back to a
/// pipeline file under the repo). `None` means a dangling pipeline reference.
fn trigger_pipeline_yaml(state: &AppState, trigger: &trigger_store::Trigger) -> Option<String> {
    library_store::pipelines::get_yaml(&state.repo_root, &trigger.pipeline_id).or_else(|| {
        std::fs::read_to_string(resolve_pipeline_path(
            &state.repo_root,
            &trigger.pipeline_id,
        ))
        .ok()
    })
}

/// Resolve the target pipeline's `prompt_required` flag for a Trigger; defaults
/// to `true` (and treats a missing/unparseable pipeline as such) so a dangling
/// reference rejects rather than fires blind.
fn trigger_prompt_required(state: &AppState, trigger: &trigger_store::Trigger) -> bool {
    match trigger_pipeline_yaml(state, trigger) {
        Some(y) => pipeline::parse_pipeline(&y)
            .map(|r| r.pipeline.prompt_required)
            .unwrap_or(true),
        None => true,
    }
}

/// Check a Trigger's external references before firing. Returns an error reason
/// if the pipeline or the target repo is dangling (deleted/renamed since the
/// Trigger was created), so the scheduler can surface an error outcome and stop
/// firing rather than rot silently (*Sharp tool*; ADR-0012).
fn trigger_dangling_reason(state: &AppState, trigger: &trigger_store::Trigger) -> Option<String> {
    if trigger_pipeline_yaml(state, trigger).is_none() {
        return Some(format!("pipeline not found: {}", trigger.pipeline_id));
    }
    if let Some(ref repo) = trigger.target_repo {
        if let Err(msg) = validate_target_repo(repo) {
            return Some(msg);
        }
    }
    None
}

/// Resolve the working directory a guard runs in: the Trigger's target repo when
/// set, otherwise the daemon's repo root.
fn trigger_guard_cwd(state: &AppState, trigger: &trigger_store::Trigger) -> PathBuf {
    trigger
        .target_repo
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| state.repo_root.clone())
}

/// Run one scheduler tick: fire every due Trigger that the decision admits,
/// recording every significant outcome and recomputing each next fire.
async fn run_trigger_scheduler_tick(state: &AppState) {
    // At most one tick in flight: see `AppState::trigger_tick_lock`.
    let _tick = state.trigger_tick_lock.lock().await;
    let now_str = event_log::now_iso();
    let due = match trigger_store::due_triggers(&state.db, &now_str).await {
        Ok(t) => t,
        Err(e) => {
            warn!("trigger scheduler: due_triggers failed: {e}");
            return;
        }
    };
    let now = chrono::Utc::now();

    for trigger in due {
        // A dangling pipeline/repo reference: surface an error outcome and stop
        // firing (clear next_fire) rather than rot silently or auto-delete.
        if let Some(reason) = trigger_dangling_reason(state, &trigger) {
            let _ = trigger_store::set_next_fire(&state.db, &trigger.id, None).await;
            let record = trigger_store::FireRecord {
                outcome: "error".to_string(),
                reason: Some(reason),
                run_id: None,
            };
            record_and_broadcast_fire(state, &trigger.id, &record).await;
            continue;
        }

        let has_live = trigger_has_live_run(&state.db, &trigger.id).await;
        let prompt_required = trigger_prompt_required(state, &trigger);

        // Run the guard (if any) off the tick, bounded by a hard timeout, but
        // only when overlap wouldn't already skip — never spend a guard run on a
        // tick we'd skip anyway.
        let overlap_skips = has_live && trigger.overlap_policy != "allow";
        let guard = match (&trigger.guard_command, overlap_skips) {
            (Some(cmd), false) if !cmd.trim().is_empty() => {
                let cwd = trigger_guard_cwd(state, &trigger);
                Some(guard_runner::run_guard(cmd, &cwd, guard_runner::guard_timeout()).await)
            }
            _ => None,
        };

        let plan = trigger_scheduler::plan_tick(&trigger, now, has_live, guard, prompt_required);

        // Recompute the next fire (forward-only).
        if let Err(e) =
            trigger_store::set_next_fire(&state.db, &trigger.id, plan.next_fire_at.as_deref()).await
        {
            warn!(
                "trigger scheduler: set_next_fire failed for {}: {e}",
                trigger.id
            );
        }

        match plan.decision {
            fire_decision::FireDecision::Fire { input } => {
                let req = CreateRunRequest {
                    pipeline: trigger.pipeline_id.clone(),
                    input,
                    variables: trigger_variables(&trigger),
                    pipeline_id: Some(trigger.pipeline_id.clone()),
                    target_repo: trigger.target_repo.clone(),
                    source_branch: trigger.source_branch.clone(),
                    name: None,
                    triggered_by: Some(trigger.id.clone()),
                };
                match create_run_inner(state, req, Vec::new()).await {
                    Ok(run_id) => {
                        let record = trigger_store::FireRecord {
                            outcome: "fired".to_string(),
                            reason: None,
                            run_id: Some(run_id),
                        };
                        record_and_broadcast_fire(state, &trigger.id, &record).await;
                    }
                    Err((_, body)) => {
                        let reason = body
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("run creation failed")
                            .to_string();
                        let record = trigger_store::FireRecord {
                            outcome: "error".to_string(),
                            reason: Some(reason),
                            run_id: None,
                        };
                        record_and_broadcast_fire(state, &trigger.id, &record).await;
                    }
                }
            }
            fire_decision::FireDecision::Skip { .. }
            | fire_decision::FireDecision::Reject { .. } => {
                if let Some(record) = plan.record {
                    record_and_broadcast_fire(state, &trigger.id, &record).await;
                }
            }
        }
    }
}

/// Parse a Trigger's stored variable overrides (a JSON object) into the
/// `CreateRunRequest` shape; an empty/invalid value yields no overrides.
fn trigger_variables(trigger: &trigger_store::Trigger) -> HashMap<String, serde_yaml::Value> {
    serde_json::from_value(trigger.variables.clone()).unwrap_or_default()
}

/// Persist a fire audit row and broadcast a `trigger_fired` update over the
/// pipeline channel so the UI refreshes live.
async fn record_and_broadcast_fire(
    state: &AppState,
    trigger_id: &str,
    record: &trigger_store::FireRecord,
) {
    if let Err(e) = trigger_store::record_fire(&state.db, trigger_id, record).await {
        warn!("trigger scheduler: record_fire failed for {trigger_id}: {e}");
    }
    let _ = state.pipeline_tx.send(serde_json::json!({
        "type": "trigger_fired",
        "trigger_id": trigger_id,
        "outcome": record.outcome,
        "run_id": record.run_id,
    }));
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    run_id: String,
    ts: String,
    kind: String,
    node_id: Option<String>,
    iter: Option<i64>,
    payload: Option<String>,
}

impl EventRow {
    fn into_event(self) -> event_log::Event {
        let kind: event_log::EventKind =
            serde_json::from_value(serde_json::Value::String(self.kind.clone()))
                .unwrap_or(event_log::EventKind::RunStarted);

        let payload = self
            .payload
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        event_log::Event {
            id: Some(self.id),
            run_id: self.run_id,
            ts: self.ts,
            kind,
            node_id: self.node_id,
            iter: self.iter,
            payload,
        }
    }
}

// --- Pipeline CRUD ---

#[derive(Serialize)]
struct PipelineListEntry {
    id: String,
    name: String,
    scope: String,
    path: String,
    node_count: usize,
    modified: Option<String>,
    variables: HashMap<String, PipelineVariableInfo>,
    /// Whether a manual Run must supply a non-empty prompt (#158). Surfaced so
    /// the New Run modal can make the prompt field optional.
    prompt_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    drifted: Option<bool>,
}

#[derive(Deserialize)]
struct CreatePipelineRequest {
    name: String,
    scope: String,
}

#[derive(Deserialize)]
struct SavePipelineRequest {
    yaml: String,
    #[serde(default)]
    prompts: HashMap<String, String>,
}

fn scan_pipeline_dir(dir: &std::path::Path, scope: &str) -> Vec<PipelineListEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return entries;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            });

        let (name, node_count, variables, prompt_required) = match std::fs::read_to_string(&path) {
            Ok(yaml) => match pipeline::parse_pipeline(&yaml) {
                Ok(r) => {
                    let vars: HashMap<String, PipelineVariableInfo> = r
                        .pipeline
                        .variables
                        .iter()
                        .map(|(k, v)| {
                            let default = yaml_value_to_json(&v.default);
                            (
                                k.clone(),
                                PipelineVariableInfo {
                                    var_type: v.var_type.clone(),
                                    default,
                                },
                            )
                        })
                        .collect();
                    (
                        r.pipeline.name.clone(),
                        r.pipeline.nodes.len(),
                        vars,
                        r.pipeline.prompt_required,
                    )
                }
                Err(_) => (file_stem.clone(), 0, HashMap::new(), true),
            },
            Err(_) => (file_stem.clone(), 0, HashMap::new(), true),
        };

        entries.push(PipelineListEntry {
            id: file_stem,
            name,
            scope: scope.to_string(),
            path: path.to_string_lossy().to_string(),
            node_count,
            modified,
            variables,
            prompt_required,
            drifted: None,
        });
    }
    entries
}

async fn list_pipelines(State(state): State<Arc<AppState>>) -> Response {
    let repo_dir = state.repo_root.join(".pdo").join("pipelines");
    let mut pipelines = scan_pipeline_dir(&repo_dir, "repo");

    if let Some(home) = dirs_next_home() {
        let user_dir = home.join(".pdo").join("pipelines");
        pipelines.extend(scan_pipeline_dir(&user_dir, "user"));
    }

    if let Some(lib_dir) = library_store::pipelines::user_pipelines_dir() {
        let lib_entries = scan_pipeline_dir(&lib_dir, "library");
        for mut entry in lib_entries {
            entry.drifted = library_store::pipelines::check_drift(&entry.id);
            pipelines.push(entry);
        }
    }

    Json(pipelines).into_response()
}

async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
    Query(scope_q): Query<ScopeQuery>,
) -> Response {
    // A library-scoped open reads the entry's *own* stored YAML, so a promoted
    // library pipeline stays openable even when the source repo file is gone
    // (#216, fix direction 3).
    if scope_q.scope.as_deref() == Some("library") {
        return library_pipeline_detail_response(&state.repo_root, &pipeline_id);
    }

    let path =
        resolve_pipeline_path_scoped(&state.repo_root, &pipeline_id, scope_q.scope.as_deref());
    let yaml = match std::fs::read_to_string(&path) {
        Ok(y) => y,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
        }
    };

    let parse_result = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("parse error: {e}") })),
            )
                .into_response();
        }
    };

    let scope = if path.starts_with(&state.repo_root) {
        "repo"
    } else {
        "user"
    };

    let mut prompts: HashMap<String, String> = HashMap::new();
    for node in &parse_result.pipeline.nodes {
        if let Ok(c) = std::fs::read_to_string(pipeline::canonical_prompt_path(&path, &node.id)) {
            prompts.insert(node.id.clone(), c);
        }
    }

    let diagnostics: Vec<String> = parse_result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();

    Json(serde_json::json!({
        "id": pipeline_id,
        "scope": scope,
        "path": path.to_string_lossy(),
        "yaml": yaml,
        "pipeline": parse_result.pipeline,
        "prompts": prompts,
        "diagnostics": diagnostics,
    }))
    .into_response()
}

fn parse_error_to_structured(e: &pipeline::ParseError) -> (String, Option<usize>) {
    match e {
        pipeline::ParseError::InvalidYaml(yaml_err) => {
            let loc = yaml_err.location();
            let line = loc.map(|l| l.line());
            (format!("{yaml_err}"), line)
        }
        pipeline::ParseError::MissingField(field) => {
            (format!("missing required field: {field}"), None)
        }
        pipeline::ParseError::UndeclaredWhenField {
            node_id,
            port,
            field,
        } => (
            format!("switch node '{node_id}' output '{port}': when-clause field '{field}' not found in upstream schema"),
            None,
        ),
    }
}

async fn save_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
    Query(scope_q): Query<ScopeQuery>,
    Json(req): Json<SavePipelineRequest>,
) -> Response {
    // A library-scoped save writes back into the entry's own library YAML so a
    // round-trip edit of a `scope: "library"` tab never overwrites a same-named
    // repo/user file (#216).
    if scope_q.scope.as_deref() == Some("library") {
        return save_library_pipeline_response(&state.repo_root, &pipeline_id, &req);
    }

    let path =
        resolve_pipeline_path_scoped(&state.repo_root, &pipeline_id, scope_q.scope.as_deref());
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
    }

    if let Err(e) = pipeline::parse_pipeline(&req.yaml) {
        let (message, line) = parse_error_to_structured(&e);
        let mut body =
            serde_json::json!({ "error": format!("invalid YAML: {e}"), "message": message });
        if let Some(l) = line {
            body["line"] = serde_json::json!(l);
        }
        return (StatusCode::BAD_REQUEST, Json(body)).into_response();
    }

    mark_self_write(&state.recent_writes, &path);
    if let Err(e) = std::fs::write(&path, &req.yaml) {
        let msg = format!("write failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg, "message": msg })),
        )
            .into_response();
    }

    for (node_id, content) in &req.prompts {
        let prompt_path = pipeline::canonical_prompt_path(&path, node_id);
        if let Some(parent) = prompt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        mark_self_write(&state.recent_writes, &prompt_path);
        if let Err(e) = std::fs::write(&prompt_path, content) {
            warn!("failed to write prompt for {node_id}: {e}");
        }
    }

    info!("Pipeline {pipeline_id} saved");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn create_pipeline(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePipelineRequest>,
) -> Response {
    let safe_name = req
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();

    let dir = if req.scope == "user" {
        match dirs_next_home() {
            Some(home) => home.join(".pdo").join("pipelines"),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "cannot determine home directory",
                )
                    .into_response();
            }
        }
    } else {
        state.repo_root.join(".pdo").join("pipelines")
    };

    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{safe_name}.yaml"));

    if path.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "pipeline already exists" })),
        )
            .into_response();
    }

    let scaffold = format!(
        "name: {safe_name}\nversion: \"1.0\"\n\nvariables: {{}}\n\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\nedges: []\n"
    );
    if let Err(e) = std::fs::write(&path, &scaffold) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {e}") })),
        )
            .into_response();
    }

    info!("Created pipeline {safe_name} at {}", path.display());
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": safe_name,
            "scope": req.scope,
            "path": path.to_string_lossy(),
        })),
    )
        .into_response()
}

// --- Library API handlers ---

async fn list_library() -> Response {
    Json(library_store::list()).into_response()
}

#[derive(Deserialize)]
struct SaveToLibraryRequest {
    name: String,
    #[serde(rename = "type")]
    node_type: pipeline::NodeType,
    #[serde(default)]
    inputs: Vec<pipeline::Port>,
    #[serde(default)]
    outputs: Vec<pipeline::Port>,
    #[serde(default)]
    interactive: bool,
    #[serde(default)]
    prompt: String,
}

async fn save_to_library(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<SaveToLibraryRequest>,
) -> Response {
    let entry = library_store::LibraryEntry {
        name: req.name,
        node_type: req.node_type,
        inputs: req.inputs,
        outputs: req.outputs,
        interactive: req.interactive,
        max_iter: None,
        branches: None,
        prompt: req.prompt,
    };

    if let Err(e) = library_store::save(&entry) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    (StatusCode::CREATED, Json(&entry)).into_response()
}

async fn delete_from_library(AxumPath(name): AxumPath<String>) -> Response {
    match library_store::delete(&name) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "entry not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

async fn instantiate_from_library(AxumPath(name): AxumPath<String>) -> Response {
    match library_store::get(&name) {
        Some(entry) => {
            let prompt = entry.prompt.clone();
            Json(serde_json::json!({
                "spec": {
                    "name": entry.name,
                    "type": entry.node_type,
                    "inputs": entry.inputs,
                    "outputs": entry.outputs,
                    "interactive": entry.interactive,
                },
                "prompt": prompt,
            }))
            .into_response()
        }
        None => (StatusCode::NOT_FOUND, "entry not found").into_response(),
    }
}

// --- Library pipeline endpoints ---

async fn list_library_pipelines(State(state): State<Arc<AppState>>) -> Response {
    Json(library_store::pipelines::list(&state.repo_root)).into_response()
}

#[derive(Deserialize)]
struct SaveLibraryPipelineRequest {
    name: String,
    yaml: String,
    #[serde(default)]
    prompts: HashMap<String, String>,
    /// When set, save in-place at this id even if `name` differs (rename path).
    #[serde(default)]
    id: Option<String>,
    /// Defaults to `"repo"` when starring fresh — the user is working in a
    /// concrete repo so the most useful default is to keep the template with it.
    #[serde(default)]
    scope: Option<String>,
}

async fn save_library_pipeline(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveLibraryPipelineRequest>,
) -> Response {
    let scope_str = req.scope.as_deref().unwrap_or("repo");
    let Some(scope) = library_store::pipelines::Scope::parse(scope_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown scope: {scope_str}") })),
        )
            .into_response();
    };
    match library_store::pipelines::save(
        &state.repo_root,
        req.id.as_deref(),
        &req.name,
        &req.yaml,
        &req.prompts,
        scope,
    ) {
        Ok(id) => {
            let entry_list = library_store::pipelines::list(&state.repo_root);
            let entry = entry_list.into_iter().find(|e| e.id == id);
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "id": id, "scope": scope.as_str(), "entry": entry })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

async fn delete_library_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match library_store::pipelines::delete(&state.repo_root, &id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "pipeline template not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// Read a library pipeline back into the same JSON shape `get_pipeline` returns,
/// resolved from the disk-first library store rather than the repo/user stores.
/// Keeps a `scope: "library"` entry openable from its own YAML (#216).
fn library_pipeline_detail_response(repo_root: &std::path::Path, id: &str) -> Response {
    let Some(path) = library_store::pipelines::get_path(repo_root, id) else {
        return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
    };
    let yaml = match std::fs::read_to_string(&path) {
        Ok(y) => y,
        Err(_) => return (StatusCode::NOT_FOUND, "pipeline not found").into_response(),
    };

    let parse_result = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("parse error: {e}") })),
            )
                .into_response();
        }
    };

    let mut prompts: HashMap<String, String> = HashMap::new();
    for node in &parse_result.pipeline.nodes {
        if let Ok(c) = std::fs::read_to_string(pipeline::canonical_prompt_path(&path, &node.id)) {
            prompts.insert(node.id.clone(), c);
        }
    }

    let diagnostics: Vec<String> = parse_result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();

    Json(serde_json::json!({
        "id": id,
        "scope": "library",
        "path": path.to_string_lossy(),
        "yaml": yaml,
        "pipeline": parse_result.pipeline,
        "prompts": prompts,
        "diagnostics": diagnostics,
    }))
    .into_response()
}

/// Save a library pipeline in place from `PUT /pipelines/{id}?scope=library`,
/// keeping the edit inside the library store. Returns the same 200/`{ok:true}`
/// (or structured BAD_REQUEST on invalid YAML) shape as `save_pipeline` (#216).
fn save_library_pipeline_response(
    repo_root: &std::path::Path,
    id: &str,
    req: &SavePipelineRequest,
) -> Response {
    let parsed = match pipeline::parse_pipeline(&req.yaml) {
        Ok(r) => r,
        Err(e) => {
            let (message, line) = parse_error_to_structured(&e);
            let mut body =
                serde_json::json!({ "error": format!("invalid YAML: {e}"), "message": message });
            if let Some(l) = line {
                body["line"] = serde_json::json!(l);
            }
            return (StatusCode::BAD_REQUEST, Json(body)).into_response();
        }
    };

    if library_store::pipelines::get_path(repo_root, id).is_none() {
        return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
    }

    // Save in place at the same id (rename-in-place), in whichever library store
    // scope the entry currently lives — default to User, the scope surfaced as
    // `library` by `list_pipelines`.
    let store_scope = library_store::pipelines::get_scope(repo_root, id)
        .unwrap_or(library_store::pipelines::Scope::User);
    match library_store::pipelines::save(
        repo_root,
        Some(id),
        &parsed.pipeline.name,
        &req.yaml,
        &req.prompts,
        store_scope,
    ) {
        Ok(_) => {
            info!("Library pipeline {id} saved");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Err(e) => {
            let msg = format!("write failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": msg.clone(), "message": msg })),
            )
                .into_response()
        }
    }
}

/// Delete a library pipeline (YAML + prompts + meta sidecar) from the library
/// store, in the 200/`{ok:true}` shape `delete_pipeline` uses. Routed here when
/// `DELETE /pipelines/{id}?scope=library` so a library delete never resolves to
/// a same-named repo file (#216).
fn delete_library_pipeline_response(repo_root: &std::path::Path, id: &str) -> Response {
    match library_store::pipelines::delete(repo_root, id) {
        Ok(true) => {
            info!("Deleted library pipeline {id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "pipeline not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("delete failed: {e}") })),
        )
            .into_response(),
    }
}

async fn delete_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
    Query(scope_q): Query<ScopeQuery>,
) -> Response {
    // A library-scoped delete operates on the independent library store — never
    // the repo/user pipeline file that happens to share the id (#216).
    if scope_q.scope.as_deref() == Some("library") {
        return delete_library_pipeline_response(&state.repo_root, &pipeline_id);
    }

    let path =
        resolve_pipeline_path_scoped(&state.repo_root, &pipeline_id, scope_q.scope.as_deref());
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
    }

    if let Ok(run_ids) = load_all_run_ids(&state.db).await {
        let mut active_count: usize = 0;
        for run_id in &run_ids {
            let Ok(events) = load_events(&state.db, run_id).await else {
                continue;
            };
            let Some(run_state) = event_log::project(&events) else {
                continue;
            };
            if run_state.pipeline_name == pipeline_id
                && run_state.status != event_log::RunStatus::Completed
                && run_state.status != event_log::RunStatus::Archived
            {
                active_count += 1;
            }
        }
        if active_count > 0 {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!("Cannot delete: {active_count} active run(s)")
                })),
            )
                .into_response();
        }
    }

    let prompts_dir = path.with_extension("prompts");
    if prompts_dir.is_dir() {
        if let Err(e) = std::fs::remove_dir_all(&prompts_dir) {
            warn!("failed to remove prompts dir for {pipeline_id}: {e}");
        }
    }

    if let Err(e) = std::fs::remove_file(&path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("delete failed: {e}") })),
        )
            .into_response();
    }

    info!("Deleted pipeline {pipeline_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn promote_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
) -> Response {
    match library_store::pipelines::promote(&state.repo_root, &pipeline_id) {
        Ok(id) => {
            let drifted = library_store::pipelines::check_drift(&id);
            Json(serde_json::json!({
                "id": id,
                "drifted": drifted.unwrap_or(false),
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

// --- API handlers ---

struct SpawnContext<'a> {
    pipeline: &'a pipeline::PipelineDef,
    run_id: &'a str,
    pipeline_path: &'a std::path::Path,
    worktree_dir: &'a std::path::Path,
    artifacts_dir: &'a std::path::Path,
    resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    repo_root: &'a std::path::Path,
}

fn deposit_foreach_items(
    artifacts_dir: &std::path::Path,
    foreach_node_id: &str,
    items: &[serde_yaml::Value],
) {
    for (i, item) in items.iter().enumerate() {
        let iter_num = (i + 1) as i64;
        let item_dir = artifacts_dir
            .join(foreach_node_id)
            .join(format!("iter-{iter_num}"));
        let _ = std::fs::create_dir_all(&item_dir);
        let item_str = serde_yaml::to_string(item).unwrap_or_else(|_| format!("{item:?}"));
        let content = format!(
            "---\nitem: {}\niter: {}\ntotal: {}\n---\n\n{}",
            item_str.trim(),
            iter_num,
            items.len(),
            item_str.trim()
        );
        let _ = std::fs::write(item_dir.join("_item.md"), content);
    }
}

fn find_foreach_context(
    spawn_ctx: &SpawnContext<'_>,
    node_id: &str,
    iter: i64,
) -> Option<prompt_augmenter::ForEachContext> {
    for edge in &spawn_ctx.pipeline.edges {
        if edge.target.node != *node_id || edge.source.port != "body" {
            continue;
        }
        let source = &edge.source.node;
        let is_foreach = spawn_ctx
            .pipeline
            .nodes
            .iter()
            .any(|n| n.id == *source && n.node_type == pipeline::NodeType::ForEach);
        if !is_foreach {
            continue;
        }
        let item_path = spawn_ctx
            .artifacts_dir
            .join(source.as_str())
            .join(format!("iter-{iter}"))
            .join("_item.md");
        let item_content = std::fs::read_to_string(&item_path).unwrap_or_default();
        let total_path = spawn_ctx.artifacts_dir.join(source.as_str());
        let total = std::fs::read_dir(&total_path)
            .map(|entries| {
                entries
                    .filter(|e| e.as_ref().is_ok_and(|e| e.path().is_dir()))
                    .count()
            })
            .unwrap_or(0) as i64;
        let current_item = item_content
            .split("---")
            .nth(2)
            .unwrap_or("")
            .trim()
            .to_string();
        return Some(prompt_augmenter::ForEachContext {
            current_item,
            current_iter: iter,
            total,
        });
    }
    None
}

async fn spawn_node(
    state: &AppState,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) {
    let run_id = spawn_ctx.run_id;

    // Transition guard (#212): refuse an illegal NodeStarted BEFORE any side
    // effect (sub-worktree creation, tmux session spawn) — never after. This
    // covers every caller: scheduler dispatch, resume re-evaluation,
    // restart_node, waiting-node retries.
    let started_probe = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some(node.id.clone()),
        iter: Some(iter),
        payload: None,
    };
    let projected = reload_run_state(state, run_id).await.map(|(_, s)| s);
    match transition_guard::validate_transition(projected.as_ref(), &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason }
        | transition_guard::Verdict::Reject { reason } => {
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return;
        }
    }

    // Admission control (#159 / #213): bound the number of live NodeRun
    // sessions daemon-wide. The check is an ATOMIC check-and-reserve — the
    // `admission_lock` is held from the count until the reservation event
    // (`NodeStarted` / `NodeWaiting`) is appended, so concurrent spawns can
    // never all observe the same free slot and overshoot the cap. If admitting
    // one more would exceed the cap, the node enters `waiting` and holds no
    // session; `retry_waiting_nodes` re-drives it once a slot frees. Checked
    // first so a throttled node creates no worktree.
    let admission_guard = state.admission_lock.lock().await;
    let cap = admission::configured_cap();
    let live = count_global_live_sessions(&state.db).await;
    if !admission::can_admit(live, cap) {
        let waiting = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeWaiting,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: Some(serde_json::json!({ "live_sessions": live, "cap": cap })),
        };
        if let Err(e) = append_event(state, &waiting).await {
            error!("failed to append node_waiting for {}: {e}", node.id);
        }
        info!(
            "node {} throttled into waiting ({live}/{cap} sessions live)",
            node.id
        );
        return;
    }

    let canonical_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let foreach_context = find_foreach_context(spawn_ctx, &node.id, iter);

    let has_sub_worktree = node.node_type == pipeline::NodeType::CodeMutating
        || node.node_type == pipeline::NodeType::Merge;

    let working_dir = if has_sub_worktree {
        let sub_wt_dir = sub_worktree_path(spawn_ctx.repo_root, run_id, &node.id, iter);
        let sub_branch = sub_worktree_branch(run_id, &node.id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");

        if let Err(e) = create_sub_worktree(
            spawn_ctx.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
        ) {
            error!("failed to create sub-worktree for {}: {e}", node.id);
            return;
        }
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

    let is_entry_node = spawn_ctx.pipeline.edges.iter().any(|e| {
        e.target.node == node.id
            && spawn_ctx
                .pipeline
                .nodes
                .iter()
                .any(|n| n.id == e.source.node && n.node_type == pipeline::NodeType::Start)
    });
    let input_images = if is_entry_node {
        prompt_augmenter::discover_input_images(spawn_ctx.artifacts_dir)
    } else {
        Vec::new()
    };

    // Canonical input resolution (#194 / #210): re-project the run state at
    // spawn time so each input path follows its source's latest COMPLETED
    // iteration — a failed iteration's artifacts are never consumed, and an
    // external feeder keeps serving its completed iter at any lap.
    let source_iters = match reload_run_state(state, run_id).await {
        Some((_, fresh_state)) => input_resolution::resolved_source_iters(
            spawn_ctx.pipeline,
            &fresh_state,
            &node.id,
            iter,
        ),
        None => HashMap::new(),
    };

    let aug_ctx = prompt_augmenter::AugmentContext {
        pipeline: spawn_ctx.pipeline,
        node,
        run_id,
        iter,
        artifacts_dir: spawn_ctx.artifacts_dir,
        variables: spawn_ctx.resolved_vars,
        daemon_url: &format!("http://localhost:{}", state.port),
        foreach_context,
        source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
        input_images,
        source_iters,
    };

    let full_prompt = prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

    let node_started = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeStarted,
        node_id: Some(node.id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({
            "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
            "node_type": match node.node_type {
                pipeline::NodeType::DocOnly => "doc-only",
                pipeline::NodeType::CodeMutating => "code-mutating",
                pipeline::NodeType::Start => "start",
                pipeline::NodeType::End => "end",
                pipeline::NodeType::Switch => "switch",
                pipeline::NodeType::Loop => "loop",
                pipeline::NodeType::ForEach => "for-each",
                pipeline::NodeType::Merge => "merge",
            },
        })),
    };
    if let Err(e) = append_event(state, &node_started).await {
        error!("failed to append node_started: {e}");
    }
    // Reservation recorded: the projected state now counts this session, so the
    // next spawn's admission count sees it. Release the admission lock before
    // the (potentially slow) tmux spawn — the slot is already held.
    drop(admission_guard);

    let session_name = tmux_session_manager::node_session_name(run_id, &node.id, iter);
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &full_prompt,
        &working_dir,
        run_id,
        &node.id,
        iter,
        state.port,
        state.tmux_cmd_override.as_deref(),
    ) {
        error!("failed to spawn tmux session: {e}");
    }

    if node.interactive {
        let awaiting = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeAwaitingUser,
            node_id: Some(node.id.clone()),
            iter: Some(iter),
            payload: None,
        };
        if let Err(e) = append_event(state, &awaiting).await {
            error!("failed to append node_awaiting_user: {e}");
        }
    }
}

async fn handle_node_completion(
    state: &AppState,
    run_state: &event_log::RunState,
    run_id: &str,
    completed_node_id: &str,
    events: &[event_log::Event],
) {
    let repo_root = effective_repo_root(state, run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };

    let pipeline = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let resolved_vars = resolve_run_variables(&pipeline, events);

    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);
    let frontmatter_fields =
        resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

    // Per-node frontmatter for every completed producer, so convergence
    // suppression (ADR-0011) can re-evaluate the conditional edges of upstream
    // producers (e.g. a classifier whose `else` branch was suppressed) and avoid
    // a silent stall at a `Merge` fed by that suppressed branch.
    let frontmatter_by_node = resolve_completed_frontmatter(&pipeline, run_state, &artifacts_dir);

    let actions = scheduler::evaluate_outgoing_edges_full(
        &pipeline,
        run_state,
        completed_node_id,
        &resolved_vars,
        &frontmatter_fields,
        &frontmatter_by_node,
    );

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    for action in &actions {
        match action {
            scheduler::SchedulerAction::Spawn { node_id, iter } => {
                if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                    spawn_node(state, &spawn_ctx, node, *iter).await;
                }
            }
            scheduler::SchedulerAction::Halt { message } => {
                emit_run_event(
                    state,
                    run_id,
                    event_log::EventKind::RunHalted,
                    Some(serde_json::json!({ "message": message })),
                )
                .await;
                return;
            }
            scheduler::SchedulerAction::Complete => {
                emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                return;
            }
            scheduler::SchedulerAction::SwitchRouted {
                node_id,
                chosen_branch,
            } => {
                emit_run_event(
                    state,
                    run_id,
                    event_log::EventKind::SwitchRouted,
                    Some(serde_json::json!({
                        "node_id": node_id,
                        "chosen_branch": chosen_branch,
                    })),
                )
                .await;
                passthrough_switch_artifact(&spawn_ctx, node_id, chosen_branch, source_iter);
            }
            scheduler::SchedulerAction::LoopIterStarted { .. }
            | scheduler::SchedulerAction::LoopBreakReceived { .. }
            | scheduler::SchedulerAction::LoopMaxReached { .. }
            | scheduler::SchedulerAction::LoopDone { .. } => {
                emit_loop_action(state, run_id, action).await;
            }
            scheduler::SchedulerAction::ForEachStarted {
                foreach_node_id,
                items,
                ..
            } => {
                emit_foreach_action(state, run_id, action).await;
                deposit_foreach_items(&artifacts_dir, foreach_node_id, items);
            }
            scheduler::SchedulerAction::ForEachEmpty { .. }
            | scheduler::SchedulerAction::ForEachBreakReceived { .. }
            | scheduler::SchedulerAction::ForEachDone { .. } => {
                emit_foreach_action(state, run_id, action).await;
            }
        }
    }

    // Pass 1 above may have appended events (e.g. LoopBreakReceived from an
    // edge into a loop). Re-project so pass 2 sees the fresh state — without
    // this, evaluate_loop_body_completion races against its own predecessors
    // and advances the loop one extra iteration after a break.
    let Some((fresh_events, fresh_run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    let fresh_resolved_vars = resolve_run_variables(&pipeline, &fresh_events);
    let fresh_spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &fresh_resolved_vars,
        repo_root: &repo_root,
    };

    // Check loop body completion for all loop nodes
    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Loop)
    {
        let loop_actions = scheduler::evaluate_loop_body_completion(
            &pipeline,
            &fresh_run_state,
            &loop_node.id,
            &fresh_resolved_vars,
        );
        for action in &loop_actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                        spawn_node(state, &fresh_spawn_ctx, node, *iter).await;
                    }
                }
                scheduler::SchedulerAction::Complete => {
                    emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                    return;
                }
                _ => emit_loop_action(state, run_id, action).await,
            }
        }
    }

    // Check foreach body completion for all foreach nodes
    for foreach_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::ForEach)
    {
        let foreach_actions = scheduler::evaluate_foreach_body_completion(
            &pipeline,
            &fresh_run_state,
            &foreach_node.id,
        );
        for action in &foreach_actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                        spawn_node(state, &fresh_spawn_ctx, node, *iter).await;
                    }
                }
                scheduler::SchedulerAction::Complete => {
                    emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                    return;
                }
                _ => emit_foreach_action(state, run_id, action).await,
            }
        }
    }
}

/// Reload events and re-project the run state.
///
/// Use after appending events inside a multi-pass dispatch so the next pass
/// observes the projection it would produce on the next tick. Without this,
/// flags like `loop_states[id].break_received` stay stale within the function
/// and downstream evaluators take decisions on stale state.
async fn reload_run_state(
    state: &AppState,
    run_id: &str,
) -> Option<(Vec<event_log::Event>, event_log::RunState)> {
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("reload_run_state: failed to load events for {run_id}: {e}");
            return None;
        }
    };
    let run_state = event_log::project(&events)?;
    Some((events, run_state))
}

async fn spawn_ready_after_event(state: &AppState, run_id: &str) {
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("spawn_ready_after_event: failed to load events for {run_id}: {e}");
            return;
        }
    };
    let Some(run_state) = event_log::project(&events) else {
        return;
    };

    if run_state.status != event_log::RunStatus::Running
        && run_state.status != event_log::RunStatus::AwaitingUser
    {
        return;
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipeline = parse_result.pipeline;

    let resolved_vars = resolve_run_variables(&pipeline, &events);
    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    let loop_seed_actions = scheduler::seed_pending_loops(&pipeline, &run_state, &resolved_vars);

    if ready.is_empty() && loop_seed_actions.is_empty() {
        // Pipeline was modified but no new nodes need spawning. If all current
        // pipeline nodes are completed, re-complete the run so it doesn't stay
        // dangling in Running state after a trivial YAML edit.
        maybe_complete_run(state, run_id, &pipeline, &run_state).await;
        return;
    }

    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    for rs in &ready {
        if let Some(node) = pipeline.nodes.iter().find(|n| n.id == rs.node_id) {
            spawn_node(state, &spawn_ctx, node, rs.iter).await;
        }
    }

    for action in &loop_seed_actions {
        match action {
            scheduler::SchedulerAction::LoopIterStarted { .. } => {
                emit_loop_action(state, run_id, action).await;
            }
            scheduler::SchedulerAction::Spawn { node_id, iter } => {
                if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                    spawn_node(state, &spawn_ctx, node, *iter).await;
                }
            }
            _ => {}
        }
    }

    info!(
        "spawn_ready_after_event: spawned {} node(s) and seeded {} loop action(s) for run {run_id}",
        ready.len(),
        loop_seed_actions.len()
    );
}

/// Re-drive nodes throttled into `waiting` by the session cap, across *all*
/// runs (admission control, #159).
///
/// Called whenever a slot may have freed (a node completed/failed/stopped, or a
/// run ended). Because the cap is daemon-wide, a slot freed in one Run can let a
/// `waiting` node in a *different* Run start, so this scans every run.
/// `spawn_node` re-checks admission per node, so a node that still can't get a
/// slot simply stays `waiting`.
async fn retry_waiting_nodes(state: &AppState) {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("retry_waiting_nodes: failed to load run ids: {e}");
            return;
        }
    };
    for run_id in run_ids {
        let Some((events, run_state)) = reload_run_state(state, &run_id).await else {
            continue;
        };
        if run_state.status != event_log::RunStatus::Running
            && run_state.status != event_log::RunStatus::AwaitingUser
        {
            continue;
        }
        let waiting = scheduler_dispatcher::waiting_nodes(&run_state);
        if waiting.is_empty() {
            continue;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let pipeline_path =
            resolve_run_pipeline_path(&repo_root, &run_id, &run_state.pipeline_name);
        let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
            continue;
        };
        let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
            continue;
        };
        let pipeline = parse_result.pipeline;
        let resolved_vars = resolve_run_variables(&pipeline, &events);
        let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let spawn_ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: &run_id,
            pipeline_path: &pipeline_path,
            worktree_dir: &worktree_dir,
            artifacts_dir: &artifacts_dir,
            resolved_vars: &resolved_vars,
            repo_root: &repo_root,
        };
        for rs in &waiting {
            if let Some(node) = pipeline.nodes.iter().find(|n| n.id == rs.node_id) {
                spawn_node(state, &spawn_ctx, node, rs.iter).await;
            }
        }
    }
}

async fn maybe_complete_run(
    state: &AppState,
    run_id: &str,
    pipeline: &pipeline::PipelineDef,
    run_state: &event_log::RunState,
) {
    if run_state.status != event_log::RunStatus::Running {
        return;
    }
    let all_done = !pipeline.nodes.is_empty()
        && pipeline.nodes.iter().all(|n| {
            run_state
                .nodes
                .get(&n.id)
                .is_some_and(|ns| ns.status == event_log::NodeStatus::Completed)
        });
    if all_done {
        let run_completed = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        };
        if let Err(e) = append_event(state, &run_completed).await {
            error!("failed to append run_completed: {e}");
        }
    }
}

/// Reconcile a run that is silently stalled at the **run level** (#214): no
/// live node, nothing the scheduler can spawn, yet still `Running`. Loads the
/// pipeline and the real scheduler outputs, runs [`run_stall_reason`], and on a
/// stall appends a `RunFailed` with the run-level cause. A no-op for runs that
/// can still make progress (or are already terminal).
///
/// Called from the periodic stale sweep and from boot recovery — the two paths
/// where a run can be observed wedged after a node turned terminal with no
/// downstream to drive (a Failed/Stale entry, a crash before downstream spawn).
async fn reconcile_run_level_stall(state: &AppState, run_id: &str) {
    let Some((events, run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    if run_state.status != event_log::RunStatus::Running {
        return;
    }

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipeline = parse_result.pipeline;
    let resolved_vars = resolve_run_variables(&pipeline, &events);

    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &resolved_vars);

    let Some(reason) = run_stall_reason(&pipeline, &run_state, &ready, &loop_seed) else {
        return;
    };

    let run_failed = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunFailed,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "reason": reason })),
    };
    // Through the guard: if the run turned terminal organically since the
    // snapshot above, the failure is dropped as a no-op.
    if let Err(e) = append_event(state, &run_failed).await {
        error!("reconcile_run_level_stall: failed to fail run {run_id}: {e}");
    } else {
        warn!("Run {run_id} reconciled to Failed — {reason}");
        // Freed slots: re-drive throttled `waiting` nodes in other runs (#159).
        retry_waiting_nodes(state).await;
    }
}

/// Decide whether a `Running` run is **stuck in a terminal-but-unreconciled
/// state** (#214, sprint invariant). A run is stuck when it has no node that
/// can drive it forward and the scheduler can produce no new work, yet it is
/// not `all completed` (that case is `maybe_complete_run`'s job). The remaining
/// possibility is a run wedged behind a Failed/Stale node whose downstream can
/// never be scheduled — a silent run-level stall the invariant forbids.
///
/// Returns `Some(reason)` to be recorded as the `RunFailed` cause, or `None`
/// when the run still has a path forward (a live node, a schedulable node, a
/// loop to seed) or is legitimately awaiting a human (`AwaitingUser`).
///
/// Pure over the already-computed scheduler outputs (`ready` from
/// [`scheduler_dispatcher::compute_ready_to_spawn`], `loop_seed` from
/// [`scheduler::seed_pending_loops`]) so the schedulability oracle is the real
/// scheduler and this never drifts from it.
fn run_stall_reason(
    pipeline: &pipeline::PipelineDef,
    run_state: &event_log::RunState,
    ready: &[scheduler_dispatcher::ReadySpawn],
    loop_seed: &[scheduler::SchedulerAction],
) -> Option<String> {
    // Only a Running run can silently stall. AwaitingUser is a legitimate live
    // state (waiting on a human); Paused/Halted/terminal need no reconciliation.
    if run_state.status != event_log::RunStatus::Running {
        return None;
    }

    // A live node (or an in-flight merge resolver) means the run can still
    // advance organically — never reconcile under it.
    let has_live_node = run_state.nodes.values().any(|n| {
        matches!(
            n.status,
            event_log::NodeStatus::Running
                | event_log::NodeStatus::Waiting
                | event_log::NodeStatus::AwaitingUser
        )
    });
    let resolver_active = run_state
        .merge_resolver
        .as_ref()
        .is_some_and(|mr| mr.status == event_log::NodeStatus::Running);
    if has_live_node || resolver_active {
        return None;
    }

    // The scheduler can still produce work (a ready node to spawn, a loop to
    // seed): the run is not stuck, it just hasn't been driven yet.
    if !ready.is_empty() || !loop_seed.is_empty() {
        return None;
    }

    // An open (not-`done`) loop/foreach region is the Pipeline Manager's domain:
    // an exhausted-unrouted region is surfaced as a Halt to be routed by id
    // (manager-unstick-loop), not a fail-fast stall. Never auto-fail under one —
    // that would steal the manager's recovery path. (Our event-driven Halt is
    // not re-derivable from the cold projection here, so we defer rather than
    // risk a false RunFailed on a routable region.)
    let open_region = run_state.loop_states.values().any(|ls| !ls.done)
        || run_state.foreach_states.values().any(|fs| !fs.done);
    if open_region {
        return None;
    }

    // All pipeline nodes Completed is the success case handled by
    // `maybe_complete_run`; do not steal it here.
    let all_completed = !pipeline.nodes.is_empty()
        && pipeline.nodes.iter().all(|n| {
            run_state
                .nodes
                .get(&n.id)
                .is_some_and(|ns| ns.status == event_log::NodeStatus::Completed)
        });
    if all_completed {
        return None;
    }

    // Fail-fast only on a concrete blocker: at least one node in a terminal but
    // non-Completed state (Failed/Stale/Stopped) whose downstream can never be
    // scheduled. Without such a blocker we cannot attribute a cause, so we defer
    // rather than guess — a run with only Pending/Completed nodes and nothing
    // schedulable is a scheduler-shape we have not characterised, not a known
    // unrecoverable stall.
    let mut blockers: Vec<&str> = run_state
        .nodes
        .values()
        .filter(|n| {
            matches!(
                n.status,
                event_log::NodeStatus::Failed
                    | event_log::NodeStatus::Stale
                    | event_log::NodeStatus::Stopped
            )
        })
        .map(|n| n.node_id.as_str())
        .collect();
    if blockers.is_empty() {
        return None;
    }
    blockers.sort_unstable();

    Some(format!(
        "run_stalled: no live node and nothing schedulable; blocked behind: {}",
        blockers.join(", ")
    ))
}

fn resolve_run_variables(
    pipeline: &pipeline::PipelineDef,
    events: &[event_log::Event],
) -> HashMap<String, serde_yaml::Value> {
    let mut resolved_vars = pipeline.variable_defaults();
    if let Some(payload) = events.first().and_then(|e| e.payload.as_ref()) {
        if let Some(vars) = payload.get("variables") {
            if let Ok(overrides) =
                serde_json::from_value::<HashMap<String, serde_yaml::Value>>(vars.clone())
            {
                for (k, v) in overrides {
                    resolved_vars.insert(k, v);
                }
            }
        }
    }
    resolved_vars
}

fn passthrough_switch_artifact(
    ctx: &SpawnContext<'_>,
    switch_node_id: &str,
    chosen_branch: &str,
    source_iter: i64,
) {
    let in_edge = ctx
        .pipeline
        .edges
        .iter()
        .find(|e| e.target.node == switch_node_id && e.target.port == "in");
    let Some(in_edge) = in_edge else { return };

    let src_path = blackboard::artifact_path(
        ctx.artifacts_dir,
        &in_edge.source.node,
        source_iter,
        &in_edge.source.port,
    );
    if !src_path.exists() {
        return;
    }

    let dst_path = blackboard::artifact_path(ctx.artifacts_dir, switch_node_id, 1, chosen_branch);
    if let Some(parent) = dst_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(&src_path, &dst_path);
}

fn resolve_source_frontmatter(
    pipeline: &pipeline::PipelineDef,
    completed_node_id: &str,
    iter: i64,
    artifacts_dir: &std::path::Path,
) -> HashMap<String, serde_yaml::Value> {
    let node = match pipeline.nodes.iter().find(|n| n.id == completed_node_id) {
        Some(n) => n,
        None => return HashMap::new(),
    };

    let mut fields = HashMap::new();
    for port in &node.outputs {
        let artifact_path =
            blackboard::artifact_path(artifacts_dir, completed_node_id, iter, &port.name);
        if let Ok(port_fields) = frontmatter_parser::parse_frontmatter_from_file(&artifact_path) {
            for (k, v) in port_fields {
                fields.insert(k, v);
            }
        }
    }
    fields
}

/// Resolves the output frontmatter of every Completed node in `run_state`,
/// keyed by node id. Used to feed convergence suppression (ADR-0011) so a
/// `Merge` does not stall on a branch that an upstream conditional/`else` edge
/// permanently suppressed.
fn resolve_completed_frontmatter(
    pipeline: &pipeline::PipelineDef,
    run_state: &event_log::RunState,
    artifacts_dir: &std::path::Path,
) -> HashMap<String, HashMap<String, serde_yaml::Value>> {
    let mut by_node = HashMap::new();
    for (node_id, node_state) in &run_state.nodes {
        if node_state.status != event_log::NodeStatus::Completed {
            continue;
        }
        let fm = resolve_source_frontmatter(pipeline, node_id, node_state.iter, artifacts_dir);
        by_node.insert(node_id.clone(), fm);
    }
    by_node
}

struct ImageFile {
    filename: String,
    data: Vec<u8>,
}

pub(crate) const ALLOWED_IMAGE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"];

fn sanitize_image_filename(raw: &str) -> Option<String> {
    let name = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(raw);
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(name.to_string())
    } else {
        None
    }
}

async fn parse_multipart_create_run(
    mut multipart: Multipart,
) -> std::result::Result<(CreateRunRequest, Vec<ImageFile>), String> {
    let mut pipeline = None;
    let mut input = None;
    let mut variables: HashMap<String, serde_yaml::Value> = HashMap::new();
    let mut pipeline_id = None;
    let mut target_repo = None;
    let mut source_branch = None;
    let mut name = None;
    let mut images = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "pipeline" => {
                pipeline = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| format!("bad field pipeline: {e}"))?,
                );
            }
            "input" => {
                input = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| format!("bad field input: {e}"))?,
                );
            }
            "variables" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| format!("bad field variables: {e}"))?;
                if !text.is_empty() {
                    variables = serde_json::from_str(&text)
                        .map_err(|e| format!("invalid variables JSON: {e}"))?;
                }
            }
            "pipeline_id" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| format!("bad field pipeline_id: {e}"))?;
                if !v.is_empty() {
                    pipeline_id = Some(v);
                }
            }
            "target_repo" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| format!("bad field target_repo: {e}"))?;
                if !v.is_empty() {
                    target_repo = Some(v);
                }
            }
            "source_branch" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| format!("bad field source_branch: {e}"))?;
                if !v.is_empty() {
                    source_branch = Some(v);
                }
            }
            "name" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| format!("bad field name: {e}"))?;
                if !v.is_empty() {
                    name = Some(v);
                }
            }
            "images" => {
                let raw_filename = field.file_name().unwrap_or("image.png").to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| format!("failed to read image: {e}"))?;
                if data.is_empty() {
                    continue;
                }
                let filename = sanitize_image_filename(&raw_filename)
                    .ok_or_else(|| format!("unsupported image type: {raw_filename}"))?;
                images.push(ImageFile {
                    filename,
                    data: data.to_vec(),
                });
            }
            _ => {}
        }
    }

    let req = CreateRunRequest {
        pipeline: pipeline.ok_or("missing field: pipeline")?,
        input: input.ok_or("missing field: input")?,
        variables,
        pipeline_id,
        target_repo,
        source_branch,
        name,
        triggered_by: None,
    };
    Ok((req, images))
}

async fn create_run(State(state): State<Arc<AppState>>, req: axum::extract::Request) -> Response {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (parsed_req, images) = if content_type.starts_with("multipart/form-data") {
        let multipart = match Multipart::from_request(req, &()).await {
            Ok(m) => m,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("multipart parse error: {e}") })),
                )
                    .into_response();
            }
        };
        match parse_multipart_create_run(multipart).await {
            Ok(r) => r,
            Err(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
        }
    } else {
        let body = match axum::body::to_bytes(req.into_body(), 10_000_000).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("failed to read body: {e}") })),
                )
                    .into_response();
            }
        };
        let parsed: CreateRunRequest = match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid JSON: {e}") })),
                )
                    .into_response();
            }
        };
        (parsed, Vec::new())
    };

    create_run_core(&state, parsed_req, images).await
}

/// Validate the user input against the pipeline's `prompt_required` flag (#158).
///
/// A prompt-required pipeline (the default) rejects whitespace-only input; a
/// prompt-optional pipeline accepts empty input (the entry node sources its own
/// work) and treats a provided prompt as additional info.
fn validate_run_input(prompt_required: bool, input: &str) -> Result<(), String> {
    if prompt_required && input.trim().is_empty() {
        return Err("this pipeline requires a prompt: input must not be empty".to_string());
    }
    Ok(())
}

async fn create_run_core(
    state: &AppState,
    req: CreateRunRequest,
    images: Vec<ImageFile>,
) -> Response {
    match create_run_inner(state, req, images).await {
        Ok(run_id) => (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response(),
        Err((status, body)) => (status, Json(body)).into_response(),
    }
}

/// The Run-creation logic, returning the new `run_id` on success or a
/// `(status, body)` pair on failure. `create_run_core` wraps this into an HTTP
/// `Response`; the trigger scheduler calls it directly to learn the run id for
/// `triggered_by` provenance.
async fn create_run_inner(
    state: &AppState,
    req: CreateRunRequest,
    images: Vec<ImageFile>,
) -> Result<String, (StatusCode, serde_json::Value)> {
    // Validate target_repo if provided
    let run_repo_root = if let Some(ref target) = req.target_repo {
        match validate_target_repo(target) {
            Ok(p) => p,
            Err(msg) => {
                return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
            }
        }
    } else {
        state.repo_root.clone()
    };

    // Validate source_branch if provided
    let source_ref = if let Some(ref branch) = req.source_branch {
        if let Err(msg) = validate_source_branch(&run_repo_root, branch) {
            return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
        }
        branch.as_str()
    } else {
        "HEAD"
    };

    let (yaml, pipeline_path) = if let Some(ref lib_id) = req.pipeline_id {
        match library_store::pipelines::get_yaml(&state.repo_root, lib_id) {
            Some(y) => {
                let path = library_store::pipelines::get_path(&state.repo_root, lib_id)
                    .unwrap_or_else(|| resolve_pipeline_path(&state.repo_root, &req.pipeline));
                (y, path)
            }
            None => {
                // Not in library store — fall back to non-library pipeline dirs
                let path = resolve_pipeline_path(&state.repo_root, lib_id);
                match std::fs::read_to_string(&path) {
                    Ok(y) => (y, path),
                    Err(_) => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            serde_json::json!({ "error": format!("pipeline template not found: {lib_id}") }),
                        ));
                    }
                }
            }
        }
    } else {
        let path = resolve_pipeline_path(&state.repo_root, &req.pipeline);
        match std::fs::read_to_string(&path) {
            Ok(y) => (y, path),
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({ "error": format!("cannot read pipeline: {e}") }),
                ));
            }
        }
    };

    let parse_result = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => r,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": format!("pipeline parse error: {e}") }),
            ));
        }
    };

    for diag in &parse_result.diagnostics {
        warn!("pipeline {}: {}", req.pipeline, diag.message);
    }

    let pipeline = parse_result.pipeline;

    // Refuse the launch on dangling edge references (#211 / #206). At edit time
    // these are info-only warnings (ADR-0001); at launch they are runtime-
    // coherence invariants — a run started over a dangling port is guaranteed
    // to stall silently mid-run. No run is created.
    let dangling = pipeline::dangling_edge_references(&pipeline);
    if !dangling.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": format!(
                    "cannot launch run: pipeline has dangling edge reference(s): {}",
                    dangling.join("; ")
                ),
            }),
        ));
    }

    // Empty input is allowed only for prompt-optional pipelines (#158).
    if let Err(msg) = validate_run_input(pipeline.prompt_required, &req.input) {
        return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
    }

    let run_id = event_log::generate_run_id();

    let edge_infos: Vec<event_log::EdgeInfo> =
        pipeline.edges.iter().map(edge_info_from_pipeline).collect();

    let node_def_infos: Vec<event_log::NodeDefInfo> =
        pipeline.nodes.iter().map(node_def_from_pipeline).collect();

    let variables_json = if req.variables.is_empty() {
        None
    } else {
        serde_json::to_value(&req.variables).ok()
    };

    let mut run_payload = serde_json::json!({
        "pipeline_name": pipeline.name,
        "input": req.input,
        "edges": edge_infos,
        "node_defs": node_def_infos,
    });
    if let Some(vars) = variables_json {
        run_payload["variables"] = vars;
    }
    if let Some(ref target) = req.target_repo {
        run_payload["target_repo"] = serde_json::json!(target);
    }
    if let Some(ref branch) = req.source_branch {
        run_payload["source_branch"] = serde_json::json!(branch);
    }
    if let Some(ref name) = req.name {
        if !name.is_empty() {
            run_payload["name"] = serde_json::json!(name);
        }
    }
    if let Some(ref trigger_id) = req.triggered_by {
        if !trigger_id.is_empty() {
            run_payload["triggered_by"] = serde_json::json!(trigger_id);
        }
    }
    if !images.is_empty() {
        let image_names: Vec<&str> = images.iter().map(|i| i.filename.as_str()).collect();
        run_payload["image_filenames"] = serde_json::json!(image_names);
    }

    let run_started = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(run_payload),
    };

    if let Err(e) = append_event(state, &run_started).await {
        error!("failed to append run_started: {e}");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "event log error" }),
        ));
    }

    // Create worktree — artifacts live under <target_repo>/.pdo/runs/<run-id>/
    let worktree_dir = worktree_dir_for_run(&run_repo_root, &run_id);
    let branch_name = format!("pdo/run-{run_id}");

    if let Err(e) = create_worktree(&run_repo_root, &worktree_dir, &branch_name, source_ref) {
        error!("failed to create worktree: {e}");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": format!("worktree creation failed: {e}") }),
        ));
    }

    // Copy pipeline YAML + prompts to run-scoped location (always in target repo)
    if let Err(e) = copy_pipeline_to_run(&run_repo_root, &pipeline_path, &run_id) {
        error!("failed to copy pipeline to run dir: {e}");
    } else if run_repo_root == state.repo_root {
        // Register the new run dir with the file watcher (run dirs are watched
        // individually and non-recursively). Runs created in another target
        // repo live outside this daemon's watch root and stay unwatched — the
        // watcher's run-id detection is rooted at `state.repo_root` anyway.
        let mut guard = state.run_watcher.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(debouncer) = guard.as_mut() {
            if let Some(run_dir) = run_scoped_pipeline_path(&run_repo_root, &run_id).parent() {
                pipeline_watcher::watch_run_dir(debouncer, run_dir);
            }
        }
    }

    // Write _input/output.md (directory-based artifact, ADR-0010)
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let input_dir = artifacts_dir.join("_input");
    if let Err(e) = std::fs::create_dir_all(&input_dir) {
        error!("failed to create _input artifact dir: {e}");
    }
    let input_path = input_dir.join("output.md");
    if let Err(e) = std::fs::write(&input_path, &req.input) {
        error!("failed to write _input/output.md: {e}");
    }

    // Write uploaded images to _input/ alongside output.md
    for image in &images {
        let image_path = input_dir.join(&image.filename);
        if let Err(e) = std::fs::write(&image_path, &image.data) {
            error!("failed to write image {}: {e}", image.filename);
        }
    }

    spawn_ready_after_event(state, &run_id).await;

    let needs_name = req.name.as_ref().is_none_or(|n| n.is_empty());
    spawn_manager_session(state, &run_id, &worktree_dir, needs_name);

    info!("Run {run_id} started for pipeline {}", pipeline.name);

    Ok(run_id)
}

fn spawn_manager_session(
    state: &AppState,
    run_id: &str,
    worktree_dir: &std::path::Path,
    needs_name: bool,
) {
    let daemon_url = format!("http://localhost:{}", state.port);

    let static_prompt = std::fs::read_to_string(
        state
            .repo_root
            .join("prompts")
            .join("builtin")
            .join("manager.md"),
    )
    .unwrap_or_default();

    let full_prompt =
        prompt_augmenter::build_manager_prompt(run_id, &daemon_url, &static_prompt, needs_name);

    let session_name = tmux_session_manager::manager_session_name(run_id);
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &full_prompt,
        worktree_dir,
        run_id,
        "__manager__",
        0,
        state.port,
        state.tmux_cmd_override.as_deref(),
    ) {
        error!("failed to spawn manager tmux session: {e}");
    } else {
        info!("Spawned manager session: {session_name}");
    }
}

fn yaml_value_to_json(val: &serde_yaml::Value) -> serde_json::Value {
    match val {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                let key = k.as_str().unwrap_or("").to_string();
                obj.insert(key, yaml_value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_value_to_json(&tagged.value),
    }
}

/// `GET /sessions` — the live NodeRun-session count, the configured cap, and
/// the daemon version, for the bottom status bar (admission control #159,
/// version display #139). Manager sessions are excluded by construction (they
/// are not nodes).
async fn sessions(State(state): State<Arc<AppState>>) -> Response {
    let live = count_global_live_sessions(&state.db).await;
    let cap = admission::configured_cap();
    Json(serde_json::json!({
        "live": live,
        "cap": cap,
        "version": env!("CARGO_PKG_VERSION"),
    }))
    .into_response()
}

async fn list_runs(State(state): State<Arc<AppState>>) -> Response {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let mut runs = Vec::new();
    for run_id in run_ids {
        let events = match load_events(&state.db, &run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(run_state) = event_log::project(&events) {
            let stalled = event_log::is_stalled(&run_state);
            runs.push(RunListEntry {
                run_id: run_state.run_id,
                pipeline_name: run_state.pipeline_name,
                status: run_state.status,
                stalled,
                started_at: run_state.started_at,
                name: run_state.name,
                triggered_by: run_state.triggered_by,
            });
        }
    }

    Json(runs).into_response()
}

async fn get_run(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    match event_log::project(&events) {
        Some(mut run_state) => {
            let repo_root = effective_repo_root(&state, &run_state);
            augment_run_state_from_disk(&mut run_state, &repo_root);
            Json(run_state).into_response()
        }
        None => (StatusCode::NOT_FOUND, "run not found").into_response(),
    }
}

async fn get_run_events(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    Json(events).into_response()
}

// --- Diff endpoints ---

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

    if event_log::project(&events).is_none() {
        return (StatusCode::NOT_FOUND, "run not found").into_response();
    }

    let pipeline_branch = format!("pdo/run-{run_id}");
    let output = match std::process::Command::new("git")
        .args(["diff", "HEAD", &pipeline_branch])
        .current_dir(&state.repo_root)
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

async fn node_diff(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
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

    let node = match run_state.nodes.get(&node_id) {
        Some(n) => n,
        None => return (StatusCode::NOT_FOUND, "node not found").into_response(),
    };

    let pipeline_branch = format!("pdo/run-{run_id}");
    let sub_branch = sub_worktree_branch(&run_id, &node_id, node.iter);

    let output = match std::process::Command::new("git")
        .args(["diff", &pipeline_branch, &sub_branch])
        .current_dir(&state.repo_root)
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
            return (StatusCode::NOT_FOUND, "node branch not found").into_response();
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

// --- Run-scoped pipeline modification handler ---

async fn handle_run_pipeline_modifications(
    state: Arc<AppState>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<pipeline_watcher::RunPipelineModified>,
) {
    while let Some(modified) = rx.recv().await {
        let event = event_log::Event {
            id: None,
            run_id: modified.run_id.clone(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::PipelineModified,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "kind": modified.kind,
                "path": modified.path.to_string_lossy(),
            })),
        };
        if let Err(e) = append_event(&state, &event).await {
            error!("failed to append pipeline_modified: {e}");
            continue;
        }

        spawn_ready_after_event(&state, &modified.run_id).await;
    }
}

// --- Run-scoped pipeline GET / PUT ---

async fn get_run_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let repo_root = match load_events(&state.db, &run_id).await {
        Ok(events) => match event_log::project(&events) {
            Some(run_state) => effective_repo_root(&state, &run_state),
            None => state.repo_root.clone(),
        },
        Err(_) => state.repo_root.clone(),
    };
    let yaml_path = run_scoped_pipeline_path(&repo_root, &run_id);
    let yaml = match std::fs::read_to_string(&yaml_path) {
        Ok(y) => y,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "run-scoped pipeline not found").into_response();
        }
    };

    let parse_result = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("parse error: {e}") })),
            )
                .into_response();
        }
    };

    let prompts_dir = run_scoped_prompts_dir(&repo_root, &run_id);
    let mut prompts: HashMap<String, String> = HashMap::new();
    if prompts_dir.is_dir() {
        for entry in std::fs::read_dir(&prompts_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("md") {
                if let (Some(stem), Ok(content)) = (
                    p.file_stem().and_then(|s| s.to_str()),
                    std::fs::read_to_string(&p),
                ) {
                    prompts.insert(stem.to_string(), content);
                }
            }
        }
    }

    Json(serde_json::json!({
        "id": run_id,
        "scope": "run",
        "path": yaml_path.to_string_lossy(),
        "yaml": yaml,
        "pipeline": parse_result.pipeline,
        "prompts": prompts,
        "diagnostics": parse_result.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>(),
    }))
    .into_response()
}

async fn save_run_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<SavePipelineRequest>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("load events: {e}") })),
            )
                .into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    let repo_root = effective_repo_root(&state, &run_state);
    let yaml_path = run_scoped_pipeline_path(&repo_root, &run_id);
    if !yaml_path.exists() {
        return (StatusCode::NOT_FOUND, "run-scoped pipeline not found").into_response();
    }

    let new_pipeline = match pipeline::parse_pipeline(&req.yaml) {
        Ok(r) => r.pipeline,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid YAML: {e}") })),
            )
                .into_response();
        }
    };

    let old_yaml = match std::fs::read_to_string(&yaml_path) {
        Ok(y) => y,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("read old pipeline: {e}") })),
            )
                .into_response();
        }
    };
    let old_pipeline = match pipeline::parse_pipeline(&old_yaml) {
        Ok(r) => r.pipeline,
        Err(_) => {
            // Old pipeline unparseable — skip validation, allow overwrite
            new_pipeline.clone()
        }
    };

    let rejections =
        mutation_validator::validate_run_mutation(&old_pipeline, &new_pipeline, &run_state);
    if !rejections.is_empty() {
        let details: Vec<_> = rejections
            .iter()
            .map(|r| serde_json::json!({ "node_id": r.node_id, "reason": r.reason }))
            .collect();
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "mutation rejected", "rejections": details })),
        )
            .into_response();
    }

    mark_self_write(&state.recent_writes, &yaml_path);
    if let Err(e) = std::fs::write(&yaml_path, &req.yaml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {e}") })),
        )
            .into_response();
    }

    let prompts_dir = run_scoped_prompts_dir(&repo_root, &run_id);
    for (node_id, content) in &req.prompts {
        let prompt_path = prompts_dir.join(format!("{node_id}.md"));
        if let Some(parent) = prompt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        mark_self_write(&state.recent_writes, &prompt_path);
        if let Err(e) = std::fs::write(&prompt_path, content) {
            warn!("failed to write run prompt for {node_id}: {e}");
        }
    }

    sync_run_pipeline_to_template(
        &state,
        &run_id,
        &run_state.pipeline_name,
        &req.yaml,
        &req.prompts,
    );

    info!("Run-scoped pipeline for {run_id} saved (validated + synced to template)");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

fn sync_run_pipeline_to_template(
    state: &AppState,
    run_id: &str,
    pipeline_name: &str,
    yaml: &str,
    prompts: &HashMap<String, String>,
) {
    let template_path = resolve_pipeline_path(&state.repo_root, pipeline_name);
    let tmp_path = template_path.with_extension("yaml.tmp");

    if let Some(parent) = template_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    mark_self_write(&state.recent_writes, &template_path);

    if let Err(e) = std::fs::write(&tmp_path, yaml) {
        warn!("sync_run_pipeline_to_template: write tmp failed: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &template_path) {
        warn!("sync_run_pipeline_to_template: rename failed: {e}");
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }

    let template_prompts_dir = template_path
        .parent()
        .unwrap_or(&state.repo_root)
        .join("prompts");
    for (node_id, content) in prompts {
        let prompt_path = template_prompts_dir.join(format!("{node_id}.md"));
        if let Some(parent) = prompt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        mark_self_write(&state.recent_writes, &prompt_path);
        if let Err(e) = std::fs::write(&prompt_path, content) {
            warn!("sync_run_pipeline_to_template: prompt sync failed for {node_id}: {e}");
        }
    }

    info!(
        "Synced run {run_id} pipeline to template at {}",
        template_path.display()
    );
}

// --- Boot recovery (#213) ---

/// Reconcile persisted run state against the live process world at daemon boot.
///
/// Posture: fail-fast, never silent auto-repair. After a daemon restart the
/// event log may claim nodes are `Running`/`AwaitingUser` whose tmux sessions
/// died with the previous process (or whose whole tmux server collapsed). Such
/// a node would otherwise stay `Running` forever, burning an admission slot
/// (#202). At boot we detect each one — its session is absent on our socket —
/// and transition it to `Failed` with a cause naming the orphaned session,
/// through the transition guard (#212, via [`append_event`]).
///
/// A second divergence class — a sub-worktree branch merged into the pipeline
/// branch with no corresponding `NodeCompleted` event — is detected and
/// surfaced (logged) so the operator sees the inconsistency; it is not
/// silently completed (that would fabricate a transition the agent never made).
async fn run_boot_recovery(state: &AppState) {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("Boot recovery: failed to load run ids: {e}");
            return;
        }
    };

    let socket = state.tmux_socket();

    for run_id in &run_ids {
        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let run_state = match event_log::project(&events) {
            Some(s) => s,
            None => continue,
        };

        // (0) Terminal run still projecting a session-holding node (#215).
        // Fail-fast can mark the whole run Failed while a sibling node is still
        // Running, so a terminal run can survive a restart with a node the
        // projection shows as Running/AwaitingUser. Phase 1 already excludes it
        // from the session cap, but the projection stays inconsistent until we
        // reconcile it. Fail each dangling node at its current iter, routed
        // through the guard (so a second boot pass is a clean no-op), then skip
        // the live-run handling below — the run is terminal and must stay so.
        let run_terminal = matches!(
            run_state.status,
            event_log::RunStatus::Completed
                | event_log::RunStatus::Failed
                | event_log::RunStatus::Halted
                | event_log::RunStatus::Archived
        );
        if run_terminal {
            let dangling: Vec<(String, i64, event_log::NodeStatus)> = run_state
                .nodes
                .iter()
                .filter(|(_, ns)| admission::node_holds_session(&ns.status))
                .map(|(id, ns)| (id.clone(), ns.iter, ns.status.clone()))
                .collect();
            for (node_id, iter, node_status) in &dangling {
                let session = tmux_session_manager::node_session_name(run_id, node_id, *iter);
                let fail = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeFailed,
                    node_id: Some(node_id.clone()),
                    iter: Some(*iter),
                    payload: Some(serde_json::json!({
                        "reason": format!(
                            "boot_recovery: run is {:?} (terminal) but node left \
                             session-holding ({:?}) across a daemon restart \
                             (session {session})",
                            run_state.status, node_status
                        )
                    })),
                };
                // Through the guard: idempotent across reboots. validate_fail
                // returns NoOp once the iteration is already terminal, so a
                // second pass appends nothing.
                if let Err(e) = append_event(state, &fail).await {
                    error!(
                        "Boot recovery: failed to reconcile dangling {node_id} iter {iter} \
                         in terminal run {run_id}: {e}"
                    );
                } else {
                    warn!(
                        "Boot recovery: node {node_id} iter {iter} in terminal run {run_id} \
                         left session-holding ({node_status:?}) — marked Failed"
                    );
                }
            }
            continue; // terminal run: orphan/stall handling below does not apply
        }

        if run_state.status != event_log::RunStatus::Running
            && run_state.status != event_log::RunStatus::AwaitingUser
        {
            continue;
        }

        // (1) Orphaned live nodes: Running/AwaitingUser with no tmux session.
        let orphaned: Vec<(String, i64)> = run_state
            .nodes
            .iter()
            .filter(|(_, ns)| {
                matches!(
                    ns.status,
                    event_log::NodeStatus::Running | event_log::NodeStatus::AwaitingUser
                )
            })
            .filter_map(|(id, ns)| {
                let session = tmux_session_manager::node_session_name(run_id, id, ns.iter);
                (!tmux_session_manager::session_exists(&socket, &session))
                    .then(|| (id.clone(), ns.iter))
            })
            .collect();

        for (node_id, iter) in &orphaned {
            let session = tmux_session_manager::node_session_name(run_id, node_id, *iter);
            let fail = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeFailed,
                node_id: Some(node_id.clone()),
                iter: Some(*iter),
                payload: Some(serde_json::json!({
                    "reason": format!(
                        "boot_recovery: tmux session {session} no longer exists \
                         (node was Running across a daemon restart)"
                    )
                })),
            };
            // Through the guard: if the node turned terminal organically before
            // this pass, the failure is dropped as a no-op.
            if let Err(e) = append_event(state, &fail).await {
                error!("Boot recovery: failed to fail orphaned {node_id} iter {iter}: {e}");
            } else {
                warn!(
                    "Boot recovery: node {node_id} iter {iter} in run {run_id} \
                     orphaned (session {session} gone) — marked Failed"
                );
            }
        }

        // (2) Merged-without-event divergence: a sub-worktree branch merged into
        // the pipeline branch whose node has no NodeCompleted. Surface it.
        let repo_root = effective_repo_root(state, &run_state);
        detect_merged_without_event(&repo_root, run_id, &run_state);

        // (3) #214: run-level stall. A run can survive a crash as `Running` with
        // no live node and nothing schedulable — either no node ever spawned, or
        // (1) just failed an orphan whose downstream can never run. Boot recovery
        // for nodes (1) does not cover this run-level case; reconcile it terminal
        // here so the run never stays Running forever. Re-reads fresh state so it
        // sees any orphan failure appended in (1).
        reconcile_run_level_stall(state, run_id).await;
    }
}

/// Detect sub-worktree branches whose work was merged into the pipeline branch
/// but for which no `NodeCompleted` was recorded (event log / git divergence,
/// #213 AC3). Logged as a fail-fast warning — never silently reconciled.
fn detect_merged_without_event(
    repo_root: &std::path::Path,
    run_id: &str,
    run_state: &event_log::RunState,
) {
    let pipeline_branch = format!("pdo/run-{run_id}");
    let divergent = merged_without_event_nodes(run_id, run_state, |sub_branch| {
        branch_is_merged_into(repo_root, sub_branch, &pipeline_branch)
    });
    for (node_id, sub_branch, status) in divergent {
        warn!(
            "Boot recovery: sub-worktree branch {sub_branch} is merged into \
             {pipeline_branch} but node {node_id} has no NodeCompleted \
             (status {status:?}) — git/event-log divergence in run {run_id}"
        );
    }
}

/// Pure detection of the git/event-log divergence in #213 AC3: a node owning a
/// sub-worktree branch (`code-mutating` / `merge`) that is **not** marked
/// `Completed` in the event log, yet whose branch `is_merged` reports as merged
/// into the pipeline branch. Returns `(node_id, sub_branch, status)` triples.
///
/// `is_merged` is injected so this is testable without a real git repo.
fn merged_without_event_nodes<F>(
    run_id: &str,
    run_state: &event_log::RunState,
    is_merged: F,
) -> Vec<(String, String, event_log::NodeStatus)>
where
    F: Fn(&str) -> bool,
{
    let mut out = Vec::new();
    for (node_id, ns) in &run_state.nodes {
        let node_type = find_node_type(run_state, node_id);
        if !matches!(node_type, Some("code-mutating") | Some("merge")) {
            continue;
        }
        if matches!(ns.status, event_log::NodeStatus::Completed) {
            continue;
        }
        let sub_branch = sub_worktree_branch(run_id, node_id, ns.iter);
        if is_merged(&sub_branch) {
            out.push((node_id.clone(), sub_branch, ns.status.clone()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Whether `branch` has been merged into `into` (i.e. `branch`'s tip is an
/// ancestor of `into`). Best-effort: a missing branch / non-repo returns false.
fn branch_is_merged_into(repo_root: &std::path::Path, branch: &str, into: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", branch, into])
        .current_dir(repo_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// --- Stale detection ---

async fn run_stale_detection(state: &AppState) {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("Stale detector: failed to load run ids: {e}");
            return;
        }
    };

    let socket = state.tmux_socket();
    let now = std::time::SystemTime::now();

    for run_id in &run_ids {
        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let run_state = match event_log::project(&events) {
            Some(s) => s,
            None => continue,
        };

        if run_state.status != event_log::RunStatus::Running
            && run_state.status != event_log::RunStatus::AwaitingUser
        {
            continue;
        }

        let repo_root = effective_repo_root(state, &run_state);
        let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let pipeline_path = resolve_run_pipeline_path(&repo_root, run_id, &run_state.pipeline_name);

        let running = stale_detector::running_nodes(&run_state);
        for (node_id, iter) in &running {
            let node_type = find_node_type(&run_state, node_id);
            let is_cm = matches!(node_type, Some("code-mutating") | Some("merge"));

            let working_dir = if is_cm {
                sub_worktree_path(&repo_root, run_id, node_id, *iter)
            } else {
                worktree_dir.clone()
            };

            let session_name = tmux_session_manager::node_session_name(run_id, node_id, *iter);
            let session_alive = tmux_session_manager::session_exists(&socket, &session_name);

            let jsonl_mtime = stale_detector::find_session_jsonl(&working_dir)
                .and_then(|p| std::fs::metadata(&p).ok())
                .and_then(|m| m.modified().ok());

            let artifacts_valid = if jsonl_mtime
                .and_then(|mt| now.duration_since(mt).ok())
                .is_some_and(|age| age >= stale_detector::STALE_THRESHOLD)
            {
                Some(stale_detector::validate_outputs(
                    &pipeline_path,
                    node_id,
                    *iter,
                    &artifacts_dir,
                ))
            } else {
                None
            };

            let probe = stale_detector::NodeProbe {
                session_alive,
                jsonl_mtime,
                now,
                artifacts_valid,
            };

            let detection = stale_detector::decide(&probe);
            if detection == stale_detector::Detection::Ok {
                continue;
            }

            let events = stale_detector::detection_events(&detection, run_id, node_id, *iter);
            for event in &events {
                // Transition guard (#212): append_event re-validates against
                // the freshly projected state, so a node that terminated
                // organically since this loop's snapshot never receives a
                // late NodeStale / NodeAutoCompleted (dropped as a no-op).
                if let Err(e) = append_event(state, event).await {
                    error!("Stale detector: failed to append event: {e}");
                }
            }

            match detection {
                stale_detector::Detection::SessionDied => {
                    info!("Stale detector: node {node_id} in run {run_id} — session died");
                    // The session is already gone; reaping captures whatever
                    // remains (usually nothing) and is a no-op otherwise. Its
                    // slot freed. Re-drive throttled `waiting` nodes (#159).
                    reap_node_session(state, &repo_root, run_id, node_id, *iter);
                    retry_waiting_nodes(state).await;
                }
                stale_detector::Detection::AutoComplete => {
                    info!(
                        "Stale detector: node {node_id} in run {run_id} — auto-completing (idle + valid outputs)"
                    );
                    // Reap on terminal state (#205): the idle session is still
                    // live — snapshot its pane then kill it so it never lingers
                    // toward the tmux-collapse point (#77/#78).
                    reap_node_session(state, &repo_root, run_id, node_id, *iter);
                    spawn_ready_after_event(state, run_id).await;
                    // The node completed: its slot freed. Re-drive throttled
                    // `waiting` nodes across all runs (#159).
                    retry_waiting_nodes(state).await;
                }
                stale_detector::Detection::Stale => {
                    info!(
                        "Stale detector: node {node_id} in run {run_id} — stale (idle + incomplete outputs)"
                    );
                }
                stale_detector::Detection::Ok => {}
            }
        }

        // #214: after node-level detection, a run may now have no live node and
        // nothing schedulable (e.g. a node just turned Stale/Failed with no
        // downstream to drive). Reconcile such a run-level stall to terminal so
        // it never sits Running forever (sprint invariant). Re-reads fresh state
        // so it observes any failure just appended above.
        reconcile_run_level_stall(state, run_id).await;
    }
}

// --- Orphan sweep / reaper ---

async fn run_orphan_sweep(db: &sqlx::SqlitePool, socket: &str, ttl: Duration) -> Result<()> {
    let run_ids = load_all_run_ids(db).await?;
    let mut run_states: HashMap<String, event_log::RunState> = HashMap::new();

    for run_id in &run_ids {
        let events = load_events(db, run_id).await?;
        if let Some(state) = event_log::project(&events) {
            run_states.insert(run_id.clone(), state);
        }
    }

    tmux_session_manager::sweep_orphans(
        socket,
        |run_id, node_id, _iter| {
            let run_state = run_states.get(run_id)?;
            let is_archived = run_state.status == event_log::RunStatus::Archived;

            if node_id == "__manager__" {
                return Some(tmux_session_manager::NodeRunInfo {
                    completed_at: run_state
                        .completed_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc)),
                    is_archived,
                });
            }

            let node = run_state.nodes.get(node_id)?;
            let completed_at = node
                .completed_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc));

            Some(tmux_session_manager::NodeRunInfo {
                completed_at,
                is_archived,
            })
        },
        ttl,
    );

    Ok(())
}

// --- Pane endpoint ---

#[derive(Serialize)]
struct PaneResponse {
    content: String,
    session_name: String,
    resumed: bool,
    stale: bool,
    /// Provenance of `content` (#205): `"live"` (captured from a running
    /// session), `"resumed"` (a dead latest-iter session was re-attached), or
    /// `"snapshot"` (the persisted post-mortem pane of a reaped terminal node).
    source: &'static str,
}

async fn node_pane(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    let iter = query.iter;

    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    let node_state = match run_state.nodes.get(&node_id) {
        Some(n) => n,
        None => {
            return (StatusCode::NOT_FOUND, "node not found in run").into_response();
        }
    };

    let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
    let is_latest_iter = node_state.iter == iter;
    let socket = state.tmux_socket();
    let repo_root = effective_repo_root(&state, &run_state);

    if let Some(content) = tmux_session_manager::capture(&socket, &session_name) {
        return Json(PaneResponse {
            content,
            session_name,
            resumed: false,
            stale: !is_latest_iter,
            source: "live",
        })
        .into_response();
    }

    // Reaped terminal node (#205): the session is gone but we persisted a pane
    // snapshot on the terminal transition. Serve it flagged `snapshot` — and
    // never resurrect the session for a terminal iteration (the
    // one-live-iteration invariant must not be violated by a pane request).
    let iter_is_terminal = node_state
        .iterations
        .iter()
        .find(|i| i.iter == iter)
        .map(|i| {
            matches!(
                i.status,
                event_log::NodeStatus::Completed
                    | event_log::NodeStatus::Failed
                    | event_log::NodeStatus::Stopped
                    | event_log::NodeStatus::Stale
            )
        })
        .unwrap_or(false);
    if iter_is_terminal {
        let snapshot_path = pane_snapshot_path(&repo_root, &run_id, &node_id, iter);
        if let Ok(content) = std::fs::read_to_string(&snapshot_path) {
            return Json(PaneResponse {
                content,
                session_name,
                resumed: false,
                stale: !is_latest_iter,
                source: "snapshot",
            })
            .into_response();
        }
    }

    if is_latest_iter && !iter_is_terminal && node_state.status != event_log::NodeStatus::Pending {
        let node_type = find_node_type(&run_state, &node_id).unwrap_or("doc-only");
        let working_dir = tmux_session_manager::working_dir_for_node(
            &repo_root, &run_id, &node_id, iter, node_type,
        );

        if working_dir.exists() {
            if let Err(e) = tmux_session_manager::resume(
                &session_name,
                &working_dir,
                &run_id,
                &node_id,
                iter,
                state.port,
                state.tmux_cmd_override.as_deref(),
            ) {
                warn!("Failed to resume session {session_name}: {e}");
                return Json(PaneResponse {
                    content: "Session no longer available".to_string(),
                    session_name,
                    resumed: false,
                    stale: false,
                    source: "unavailable",
                })
                .into_response();
            }

            // Give the resumed session a moment to initialize
            tokio::time::sleep(Duration::from_millis(500)).await;

            let content = tmux_session_manager::capture(&socket, &session_name)
                .unwrap_or_else(|| "Connecting...".to_string());

            return Json(PaneResponse {
                content,
                session_name,
                resumed: true,
                stale: false,
                source: "resumed",
            })
            .into_response();
        }
    }

    // Not latest iter or working_dir gone — return placeholder
    Json(PaneResponse {
        content: "Session no longer available".to_string(),
        session_name,
        resumed: false,
        stale: !is_latest_iter,
        source: "unavailable",
    })
    .into_response()
}

fn find_node_type<'a>(run_state: &'a event_log::RunState, node_id: &str) -> Option<&'a str> {
    run_state
        .node_defs
        .iter()
        .find(|nd| nd.id == node_id)
        .map(|nd| nd.node_type.as_str())
}

async fn node_prompt(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    let iter = query.iter;

    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    if !run_state.nodes.contains_key(&node_id) {
        return (StatusCode::NOT_FOUND, "node not found in run").into_response();
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let node_type = find_node_type(&run_state, &node_id).unwrap_or("doc-only");
    let working_dir =
        tmux_session_manager::working_dir_for_node(&repo_root, &run_id, &node_id, iter, node_type);

    let prompt_path = working_dir
        .join(".pdo")
        .join("prompts")
        .join(format!("{node_id}-iter-{iter}.md"));

    match std::fs::read_to_string(&prompt_path) {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/markdown")],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "prompt file not found").into_response(),
    }
}

// --- Node IO endpoint ---

async fn node_io(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Query(query): Query<IterQuery>,
) -> Response {
    let iter = query.iter;

    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    if !run_state.nodes.contains_key(&node_id)
        && !run_state.node_defs.iter().any(|nd| nd.id == node_id)
    {
        return (StatusCode::NOT_FOUND, "node not found in run").into_response();
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let yaml_path = run_scoped_pipeline_path(&repo_root, &run_id);
    let pipeline = match std::fs::read_to_string(&yaml_path)
        .ok()
        .and_then(|y| pipeline::parse_pipeline(&y).ok())
    {
        Some(r) => r.pipeline,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not load run pipeline",
            )
                .into_response();
        }
    };

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let io = node_io_resolver::resolve(&pipeline, &artifacts_dir, &node_id, iter);
    Json(io).into_response()
}

// --- Artifact endpoint ---

#[derive(Deserialize)]
struct ArtifactQuery {
    path: String,
}

async fn artifact(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<ArtifactQuery>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    let repo_root = effective_repo_root(&state, &run_state);
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let requested = Path::new(&query.path);
    let resolved = match artifacts_dir.join(requested).canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response();
        }
    };

    let canonical_artifacts = match artifacts_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "artifacts directory not found").into_response();
        }
    };

    if !resolved.starts_with(&canonical_artifacts) {
        return (StatusCode::BAD_REQUEST, "path traversal not allowed").into_response();
    }

    let mime = match resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "text/markdown",
    };

    if mime.starts_with("image/") {
        match std::fs::read(&resolved) {
            Ok(bytes) => (StatusCode::OK, [(header::CONTENT_TYPE, mime)], bytes).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    } else {
        match std::fs::read_to_string(&resolved) {
            Ok(content) => {
                (StatusCode::OK, [(header::CONTENT_TYPE, mime)], content).into_response()
            }
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    }
}

async fn spawn_merge_resolver(
    state: &AppState,
    run_id: &str,
    conflicting_node_id: &str,
    conflicting_iter: i64,
    worktree_dir: &std::path::Path,
) -> Response {
    let prompt = load_merge_resolver_prompt(&state.repo_root);
    let session_name = tmux_session_manager::node_session_name(run_id, MERGE_RESOLVER_NODE_ID, 1);

    let resolver_started = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::MergeResolverStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "conflicting_node_id": conflicting_node_id,
            "iter": conflicting_iter,
            "session_name": session_name,
        })),
    };
    let _ = append_event(state, &resolver_started).await;

    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &prompt,
        worktree_dir,
        run_id,
        MERGE_RESOLVER_NODE_ID,
        1,
        state.port,
        state.tmux_cmd_override.as_deref(),
    ) {
        error!("failed to spawn merge resolver tmux session: {e}");
        let fail_event = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::MergeResolverFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "reason": format!("failed to spawn resolver session: {e}")
            })),
        };
        let _ = append_event(state, &fail_event).await;
        let run_failed = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "reason": "merge resolver spawn failed"
            })),
        };
        let _ = append_event(state, &run_failed).await;

        return (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "merge_resolver_failed" })),
        )
            .into_response();
    }

    info!("Spawned merge resolver for run {run_id} (conflict on {conflicting_node_id})");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "merge_resolver_spawned" })),
    )
        .into_response()
}

async fn handle_merge_resolver_done(
    state: &AppState,
    run_id: &str,
    worktree_dir: &std::path::Path,
    pre_run_state: &event_log::RunState,
) -> Response {
    let problems = match validate_merge_resolution(worktree_dir) {
        Ok(p) => p,
        Err(e) => {
            error!("merge resolution validation error: {e}");
            vec![format!("validation error: {e}")]
        }
    };

    if !problems.is_empty() {
        let reason = problems.join("; ");
        let fail_event = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::MergeResolverFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "reason": reason })),
        };
        let _ = append_event(state, &fail_event).await;

        let run_failed = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "reason": format!("merge resolution failed: {reason}")
            })),
        };
        let _ = append_event(state, &run_failed).await;

        warn!("Merge resolver failed for run {run_id}: {reason}");
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "merge_resolution_failed", "reason": reason })),
        )
            .into_response();
    }

    let completed_event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::MergeResolverCompleted,
        node_id: None,
        iter: None,
        payload: None,
    };
    let _ = append_event(state, &completed_event).await;

    info!("Merge resolver completed for run {run_id}");

    // Re-evaluate the run: the conflicting node's merge is resolved.
    // Emit NodeCompleted for the original conflicting node and continue scheduling.
    if let Some(ref mr) = pre_run_state.merge_resolver {
        let original_node_id = &mr.conflicting_node_id;
        let original_iter = mr.iter;

        let node_completed = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some(original_node_id.clone()),
            iter: Some(original_iter),
            payload: None,
        };
        if let Err(e) = append_event(state, &node_completed).await {
            error!("failed to append node_completed for resolved node: {e}");
        }

        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(e) => {
                error!("failed to reload events: {e}");
                return (StatusCode::OK, "ok").into_response();
            }
        };

        if let Some(run_state) = event_log::project(&events) {
            handle_node_completion(state, &run_state, run_id, original_node_id, &events).await;
        }

        spawn_ready_after_event(state, run_id).await;
        // A node just finished: a session slot may have freed. Re-drive any
        // throttled `waiting` nodes across all runs (#159).
        retry_waiting_nodes(state).await;

        // Check run completion (same logic as node_done)
        let events = match load_events(&state.db, run_id).await {
            Ok(e) => e,
            Err(e) => {
                error!("failed to reload events: {e}");
                return (StatusCode::OK, "ok").into_response();
            }
        };
        if let Some(run_state) = event_log::project(&events) {
            if run_state.status == event_log::RunStatus::Running {
                let expected_node_ids: Vec<String> = if !run_state.node_defs.is_empty() {
                    run_state.node_defs.iter().map(|nd| nd.id.clone()).collect()
                } else {
                    run_state.nodes.keys().cloned().collect()
                };
                let all_done = !expected_node_ids.is_empty()
                    && expected_node_ids.iter().all(|nid| {
                        run_state
                            .nodes
                            .get(nid)
                            .is_some_and(|ns| ns.status == event_log::NodeStatus::Completed)
                    });
                if all_done {
                    let run_completed = event_log::Event {
                        id: None,
                        run_id: run_id.to_string(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::RunCompleted,
                        node_id: None,
                        iter: None,
                        payload: None,
                    };
                    let _ = append_event(state, &run_completed).await;
                }
            }
        }
    }

    (StatusCode::OK, "ok").into_response()
}

async fn node_done(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    body: Option<Json<NodeDoneRequest>>,
) -> Response {
    let iter = body.and_then(|b| b.iter).unwrap_or(1);

    // Per #23: session stays alive for terminal preview. The reaper kills it
    // after the TTL (default 1h), or cleanup_run kills it immediately.

    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let pre_run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    let repo_root = effective_repo_root(&state, &pre_run_state);
    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);

    if node_id == MERGE_RESOLVER_NODE_ID {
        return handle_merge_resolver_done(&state, &run_id, &worktree_dir, &pre_run_state).await;
    }

    // Transition guard (#212): validate the completion against the projected
    // state BEFORE any side effect (sub-worktree merge, doc-only cleanliness
    // check, output validation, downstream dispatch). A duplicate completion
    // is a no-op — it must not merge again nor re-trigger downstream spawns.
    let completion_probe = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: None,
    };
    match transition_guard::validate_transition(Some(&pre_run_state), &completion_probe) {
        transition_guard::Verdict::Reject { reason } => {
            warn!("node_done rejected for {node_id} iter {iter} in run {run_id}: {reason}");
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": reason })),
            )
                .into_response();
        }
        transition_guard::Verdict::NoOp { reason } => {
            info!("node_done no-op for {node_id} iter {iter} in run {run_id}: {reason}");
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
            )
                .into_response();
        }
        transition_guard::Verdict::Allow => {}
    }

    match find_node_type(&pre_run_state, &node_id) {
        Some("code-mutating") | Some("merge") => {
            let sub_wt_dir = sub_worktree_path(&repo_root, &run_id, &node_id, iter);
            let sub_branch = sub_worktree_branch(&run_id, &node_id, iter);

            let _lock = state.merge_lock.lock().await;
            let merge_result = match commit_and_merge_sub_worktree_inner(
                &sub_wt_dir,
                &worktree_dir,
                &sub_branch,
                &node_id,
                iter,
                false,
            ) {
                Ok(r) => r,
                Err(e) => {
                    error!("failed to commit/merge sub-worktree for {node_id}: {e}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            match merge_result {
                MergeResult::Success => {}
                MergeResult::Conflict(detail) => {
                    let conflict_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::MergeConflictDetected,
                        node_id: Some(node_id.clone()),
                        iter: Some(iter),
                        payload: Some(serde_json::json!({
                            "reason": format!("conflict merging {node_id} into pipeline branch"),
                            "detail": detail,
                        })),
                    };
                    let _ = append_event(&state, &conflict_event).await;

                    let run_failed = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::RunFailed,
                        node_id: None,
                        iter: None,
                        payload: Some(serde_json::json!({
                            "reason": format!("merge conflict on {node_id}")
                        })),
                    };
                    let _ = append_event(&state, &run_failed).await;

                    warn!("Merge conflict for node {node_id} in run {run_id}");
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({ "status": "merge_conflict" })),
                    )
                        .into_response();
                }
                MergeResult::ConflictPendingResolution(detail) => {
                    let conflict_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::MergeConflictDetected,
                        node_id: Some(node_id.clone()),
                        iter: Some(iter),
                        payload: Some(serde_json::json!({
                            "reason": format!("conflict merging {node_id} into pipeline branch"),
                            "detail": detail,
                        })),
                    };
                    let _ = append_event(&state, &conflict_event).await;

                    return spawn_merge_resolver(&state, &run_id, &node_id, iter, &worktree_dir)
                        .await;
                }
            }
        }
        Some("doc-only") => match worktree_has_tracked_changes(&worktree_dir) {
            Ok(true) => {
                let fail_event = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeFailed,
                    node_id: Some(node_id.clone()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "reason": "doc_violated_code_immutability"
                    })),
                };
                let _ = append_event(&state, &fail_event).await;

                let run_failed = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::RunFailed,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({
                        "reason": format!("doc-only node {node_id} violated code immutability")
                    })),
                };
                let _ = append_event(&state, &run_failed).await;

                warn!("Doc-only node {node_id} modified tracked files in run {run_id}");
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "doc_violated_code_immutability" })),
                )
                    .into_response();
            }
            Ok(false) => {}
            Err(e) => {
                warn!("Could not check worktree cleanliness for {node_id}: {e}");
            }
        },
        _ => {}
    }

    let pipeline_path =
        resolve_run_pipeline_path(&repo_root, &run_id, &pre_run_state.pipeline_name);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    if let Some(resp) = check_output_validation_with_retry(
        &state,
        &pipeline_path,
        &node_id,
        iter,
        &artifacts_dir,
        &run_id,
        &pre_run_state,
    )
    .await
    {
        return resp;
    }

    let event = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeCompleted,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: None,
    };

    if let Err(e) = append_event(&state, &event).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
    }

    // Reap on terminal state (#205): snapshot the pane then kill the session,
    // so a completed node never holds a live session toward the tmux-collapse
    // point (#77/#78). Post-mortem inspection survives via the snapshot.
    reap_node_session(&state, &repo_root, &run_id, &node_id, iter);

    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    if let Some(run_state) = event_log::project(&events) {
        handle_node_completion(&state, &run_state, &run_id, &node_id, &events).await;
    }

    spawn_ready_after_event(&state, &run_id).await;
    // A node just finished: a session slot may have freed. Re-drive any
    // throttled `waiting` nodes across all runs (#159).
    retry_waiting_nodes(&state).await;

    // Check run completion
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("failed to reload events: {e}");
            return (StatusCode::OK, "ok").into_response();
        }
    };

    if let Some(run_state) = event_log::project(&events) {
        if run_state.status == event_log::RunStatus::Halted {
            info!("Run {run_id} halted");
            return (StatusCode::OK, "ok").into_response();
        }

        let expected_node_ids: Vec<String> = if !run_state.node_defs.is_empty() {
            run_state.node_defs.iter().map(|nd| nd.id.clone()).collect()
        } else {
            run_state.nodes.keys().cloned().collect()
        };

        let all_done = !expected_node_ids.is_empty()
            && expected_node_ids.iter().all(|nid| {
                run_state
                    .nodes
                    .get(nid)
                    .is_some_and(|ns| ns.status == event_log::NodeStatus::Completed)
            });

        if all_done && run_state.status == event_log::RunStatus::Running {
            let run_completed = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunCompleted,
                node_id: None,
                iter: None,
                payload: None,
            };
            if let Err(e) = append_event(&state, &run_completed).await {
                error!("failed to append run_completed: {e}");
            }
        }
    }

    info!("Node {node_id} completed in run {run_id}");
    (StatusCode::OK, "ok").into_response()
}

async fn node_fail(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    Json(req): Json<NodeFailRequest>,
) -> Response {
    let iter = req.iter.unwrap_or(1);

    let event = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeFailed,
        node_id: Some(node_id.clone()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "reason": req.reason })),
    };

    if let Err(e) = append_event(&state, &event).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
    }

    // Reap on terminal state (#205): snapshot the pane then kill the session.
    // Post-mortem inspection of the failed node survives via the snapshot.
    let repo_root = match load_events(&state.db, &run_id).await {
        Ok(evs) => event_log::project(&evs)
            .map(|s| effective_repo_root(&state, &s))
            .unwrap_or_else(|| state.repo_root.clone()),
        Err(_) => state.repo_root.clone(),
    };
    reap_node_session(&state, &repo_root, &run_id, &node_id, iter);

    // Mark the run as failed
    let run_failed = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunFailed,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "reason": req.reason })),
    };
    if let Err(e) = append_event(&state, &run_failed).await {
        error!("failed to append run_failed: {e}");
    }

    // The run failed: its other NodeRun sessions will be reaped, freeing slots.
    // Re-drive throttled `waiting` nodes in other runs (#159).
    retry_waiting_nodes(&state).await;

    info!("Node {node_id} failed in run {run_id}");
    (StatusCode::OK, "ok").into_response()
}

async fn node_start(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    if let Some(ns) = run_state.nodes.get(&node_id) {
        if ns.status == event_log::NodeStatus::Running {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "node is already running" })),
            )
                .into_response();
        }
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot read pipeline").into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot parse pipeline").into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let iter = run_state
        .nodes
        .get(&node_id)
        .map(|ns| ns.iter + 1)
        .unwrap_or(1);

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let resolved_vars = resolve_run_variables(&pipeline_def, &events);

    let params = node_primitives::StartNodeParams {
        run_id: &run_id,
        node_id: &node_id,
        iter,
        overrides: None,
        pipeline: &pipeline_def,
        run_state: &run_state,
        artifacts_dir: &artifacts_dir,
        worktree_dir: &worktree_dir,
        repo_root: &repo_root,
        pipeline_path: &pipeline_path,
        resolved_vars: &resolved_vars,
        daemon_port: state.port,
        tmux_cmd_override: state.tmux_cmd_override.as_deref(),
    };

    let result = node_primitives::start_node(&params);

    for ev in &result.events {
        if let Err(e) = append_event(&state, ev).await {
            error!("failed to append event: {e}");
        }
    }

    match result.outcome {
        node_primitives::PrimitiveOutcome::Executed => {
            info!("node_start: started {node_id} iter {iter} in run {run_id}");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "iter": iter })),
            )
                .into_response()
        }
        node_primitives::PrimitiveOutcome::AlreadyDone => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "already_running": true })),
        )
            .into_response(),
        node_primitives::PrimitiveOutcome::Rejected { reason } => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
    }
}

async fn node_stop(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    let ns = match run_state.nodes.get(&node_id) {
        Some(ns) if ns.status == event_log::NodeStatus::Running => ns,
        Some(_) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "node is not running" })),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "node not found in run state" })),
            )
                .into_response();
        }
    };

    let iter = ns.iter;

    // Reap on terminal state (#205): snapshot the pane BEFORE the session is
    // killed, so the stopped node's post-mortem pane survives. `stop_node`
    // kills the session too (idempotent — reap already killed it).
    let repo_root = effective_repo_root(&state, &run_state);
    reap_node_session(&state, &repo_root, &run_id, &node_id, iter);

    let params = node_primitives::StopNodeParams {
        run_id: &run_id,
        node_id: &node_id,
        iter,
        tmux_socket: &state.tmux_socket(),
    };

    let result = node_primitives::stop_node(&params);

    for ev in &result.events {
        if let Err(e) = append_event(&state, ev).await {
            error!("failed to append event: {e}");
        }
    }

    // Stopping a running node freed its session slot. Re-drive throttled
    // `waiting` nodes across all runs (#159).
    retry_waiting_nodes(&state).await;

    info!("node_stop: stopped {node_id} iter {iter} in run {run_id}");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn node_retry(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    let current_iter = run_state.nodes.get(&node_id).map(|ns| ns.iter).unwrap_or(1);

    let repo_root = effective_repo_root(&state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot read pipeline").into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot parse pipeline").into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    if let Some(ns) = run_state.nodes.get(&node_id) {
        if ns.status == event_log::NodeStatus::Running {
            let stop_params = node_primitives::StopNodeParams {
                run_id: &run_id,
                node_id: &node_id,
                iter: current_iter,
                tmux_socket: &state.tmux_socket(),
            };
            let stop_result = node_primitives::stop_node(&stop_params);
            for ev in &stop_result.events {
                if let Err(e) = append_event(&state, ev).await {
                    error!("failed to append stop event: {e}");
                }
            }
        }
    }

    let downstream: Vec<String> = graph_resolver::downstream_subgraph(&pipeline_def, &node_id)
        .into_iter()
        .collect();

    let inv_params = node_primitives::InvalidateNodesParams {
        run_id: &run_id,
        node_ids: &downstream,
        artifacts_dir: &artifacts_dir,
    };
    let inv_result = node_primitives::invalidate_nodes(&inv_params);
    for ev in &inv_result.events {
        if let Err(e) = append_event(&state, ev).await {
            error!("failed to append invalidation event: {e}");
        }
    }

    // Invalidate self separately so its state resets for the re-start below
    let self_inv = node_primitives::InvalidateNodesParams {
        run_id: &run_id,
        node_ids: std::slice::from_ref(&node_id),
        artifacts_dir: &artifacts_dir,
    };
    let self_inv_result = node_primitives::invalidate_nodes(&self_inv);
    for ev in &self_inv_result.events {
        if let Err(e) = append_event(&state, ev).await {
            error!("failed to append self-invalidation event: {e}");
        }
    }

    // Reload so start_node sees the invalidated state
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "projection failed after invalidation",
            )
                .into_response();
        }
    };
    let resolved_vars = resolve_run_variables(&pipeline_def, &events);

    let next_iter = current_iter + 1;
    let start_params = node_primitives::StartNodeParams {
        run_id: &run_id,
        node_id: &node_id,
        iter: next_iter,
        overrides: None,
        pipeline: &pipeline_def,
        run_state: &run_state,
        artifacts_dir: &artifacts_dir,
        worktree_dir: &worktree_dir,
        repo_root: &repo_root,
        pipeline_path: &pipeline_path,
        resolved_vars: &resolved_vars,
        daemon_port: state.port,
        tmux_cmd_override: state.tmux_cmd_override.as_deref(),
    };

    let start_result = node_primitives::start_node(&start_params);
    for ev in &start_result.events {
        if let Err(e) = append_event(&state, ev).await {
            error!("failed to append start event: {e}");
        }
    }

    let mut invalidated: Vec<String> = downstream;
    invalidated.sort();

    info!(
        "node_retry: retried {node_id} iter {next_iter} in run {run_id}, invalidated {} downstream nodes",
        invalidated.len()
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "iter": next_iter,
            "invalidated": invalidated,
        })),
    )
        .into_response()
}

async fn node_retry_preview(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
) -> Response {
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "run not found" })),
            )
                .into_response();
        }
    };

    let repo_root = effective_repo_root(&state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot read pipeline").into_response();
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "cannot parse pipeline").into_response();
    };
    let pipeline_def = parse_result.pipeline;

    let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");

    let mut downstream: Vec<String> = graph_resolver::downstream_subgraph(&pipeline_def, &node_id)
        .into_iter()
        .collect();
    downstream.sort();

    let with_artifacts: Vec<&String> = downstream
        .iter()
        .filter(|nid| artifacts_dir.join(nid.as_str()).exists())
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "downstream": downstream,
            "affected_count": with_artifacts.len(),
            "with_artifacts": with_artifacts,
        })),
    )
        .into_response()
}

async fn run_command(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<RunCommandRequest>,
) -> Response {
    match req.kind.as_str() {
        "mark_node_done" => {
            let Some(node_id) = req.node_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "node_id required for mark_node_done" })),
                )
                    .into_response();
            };
            let iter = req.iter.unwrap_or(1);

            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = event_log::project(&events);

            // Transition guard (#212): validate the completion against the
            // projected state BEFORE any side effect (output validation,
            // append, downstream dispatch).
            let completion_probe = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: None,
            };
            match transition_guard::validate_transition(run_state.as_ref(), &completion_probe) {
                transition_guard::Verdict::Reject { reason } => {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({ "error": reason })),
                    )
                        .into_response();
                }
                transition_guard::Verdict::NoOp { reason } => {
                    info!("mark_node_done no-op: {reason}");
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({ "ok": true, "noop": true, "reason": reason })),
                    )
                        .into_response();
                }
                transition_guard::Verdict::Allow => {}
            }

            let empty_run_state = event_log::RunState::new(run_id.clone(), String::new());
            let rs_ref = run_state.as_ref().unwrap_or(&empty_run_state);
            let repo_root = effective_repo_root(&state, rs_ref);
            let pipeline_name = run_state
                .as_ref()
                .map(|rs| rs.pipeline_name.as_str())
                .unwrap_or("");
            let pipeline_path = resolve_run_pipeline_path(&repo_root, &run_id, pipeline_name);
            let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
            let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
            if let Some(resp) = check_output_validation_with_retry(
                &state,
                &pipeline_path,
                &node_id,
                iter,
                &artifacts_dir,
                &run_id,
                rs_ref,
            )
            .await
            {
                return resp;
            }

            let event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({ "source": "mark_node_done" })),
            };

            if let Err(e) = append_event(&state, &event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "mark_node_done",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append mark_node_done command event: {e}");
            }

            // Dispatch downstream nodes
            spawn_ready_after_event(&state, &run_id).await;
            // The interactive node completed: its session slot freed. Re-drive
            // throttled `waiting` nodes across all runs (#159).
            retry_waiting_nodes(&state).await;

            // Check run completion
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };

            if let Some(run_state) = event_log::project(&events) {
                handle_node_completion(&state, &run_state, &run_id, &node_id, &events).await;

                let events = match load_events(&state.db, &run_id).await {
                    Ok(e) => e,
                    Err(_) => {
                        return (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
                            .into_response();
                    }
                };
                if let Some(run_state) = event_log::project(&events) {
                    if run_state.status != event_log::RunStatus::Halted {
                        let expected_node_ids: Vec<String> = if !run_state.node_defs.is_empty() {
                            run_state.node_defs.iter().map(|nd| nd.id.clone()).collect()
                        } else {
                            run_state.nodes.keys().cloned().collect()
                        };

                        let all_done = !expected_node_ids.is_empty()
                            && expected_node_ids.iter().all(|nid| {
                                run_state
                                    .nodes
                                    .get(nid)
                                    .is_some_and(|ns| ns.status == event_log::NodeStatus::Completed)
                            });

                        if all_done
                            && (run_state.status == event_log::RunStatus::Running
                                || run_state.status == event_log::RunStatus::AwaitingUser)
                        {
                            let run_completed = event_log::Event {
                                id: None,
                                run_id: run_id.clone(),
                                ts: event_log::now_iso(),
                                kind: event_log::EventKind::RunCompleted,
                                node_id: None,
                                iter: None,
                                payload: None,
                            };
                            if let Err(e) = append_event(&state, &run_completed).await {
                                error!("failed to append run_completed: {e}");
                            }
                        }
                    }
                }
            }

            info!("mark_node_done: node {node_id} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "extend_cycle" => {
            let Some(node_id) = req.node_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "node_id required for extend_cycle" })),
                )
                    .into_response();
            };
            let Some(additional_iter) = req.additional_iter else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({ "error": "additional_iter required for extend_cycle" }),
                    ),
                )
                    .into_response();
            };
            if additional_iter <= 0 {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "additional_iter must be positive" })),
                )
                    .into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "extend_cycle",
                    "node_id": node_id,
                    "additional_iter": additional_iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            if run_state.status == event_log::RunStatus::Halted
                || run_state.status == event_log::RunStatus::Failed
            {
                let resume_event = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::CommandIssued,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "command": "resume_run" })),
                };
                if let Err(e) = append_event(&state, &resume_event).await {
                    error!("failed to append resume_run: {e}");
                }
            }

            // Re-evaluate outgoing edges with the extended cycle
            re_evaluate_after_command(&state, &run_id).await;

            info!("extend_cycle: node {node_id} +{additional_iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        // The Pipeline Manager routes a loop region BY ID (ADR-0011 / #152):
        // `bump_region` runs N more iterations; `end_region` fires its
        // completion. Both append a control-flow `CommandIssued` event and then
        // continue the run (lift an exhausted-unrouted Halt and re-evaluate),
        // so a stalled region is unstuck without restarting the daemon.
        "bump_region" | "end_region" => {
            let Some(region_id) = req.region_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("region_id required for {}", req.kind)
                    })),
                )
                    .into_response();
            };

            let payload = if req.kind == "bump_region" {
                let Some(additional_iter) = req.additional_iter else {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "additional_iter required for bump_region"
                        })),
                    )
                        .into_response();
                };
                if additional_iter <= 0 {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": "additional_iter must be positive" })),
                    )
                        .into_response();
                }
                serde_json::json!({
                    "command": "bump_region",
                    "region_id": region_id,
                    "additional_iter": additional_iter,
                })
            } else {
                serde_json::json!({
                    "command": "end_region",
                    "region_id": region_id,
                })
            };

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(payload),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            // Continue the run: an exhausted-unrouted region halts the run, so
            // lift the Halt/Failed back to Running before re-evaluating.
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };
            if run_state.status == event_log::RunStatus::Halted
                || run_state.status == event_log::RunStatus::Failed
            {
                let resume_event = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::CommandIssued,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "command": "resume_run" })),
                };
                if let Err(e) = append_event(&state, &resume_event).await {
                    error!("failed to append resume_run after region route: {e}");
                }
            }

            re_evaluate_after_command(&state, &run_id).await;

            info!("{}: region {region_id} in run {run_id}", req.kind);
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "pause_run" => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            if !matches!(
                run_state.status,
                event_log::RunStatus::Running | event_log::RunStatus::AwaitingUser
            ) {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": format!("cannot pause run in {:?} state", run_state.status)
                    })),
                )
                    .into_response();
            }

            let pause_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunPaused,
                node_id: None,
                iter: None,
                payload: None,
            };
            if let Err(e) = append_event(&state, &pause_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("pause_run: run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "resume_run" => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            match run_state.status {
                event_log::RunStatus::Paused => {
                    let resume_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::RunResumed,
                        node_id: None,
                        iter: None,
                        payload: None,
                    };
                    if let Err(e) = append_event(&state, &resume_event).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                }
                event_log::RunStatus::Halted | event_log::RunStatus::Failed => {
                    let cmd_event = event_log::Event {
                        id: None,
                        run_id: run_id.clone(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::CommandIssued,
                        node_id: None,
                        iter: None,
                        payload: Some(serde_json::json!({ "command": "resume_run" })),
                    };
                    if let Err(e) = append_event(&state, &cmd_event).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                }
                _ => {}
            }

            re_evaluate_after_command(&state, &run_id).await;

            info!("resume_run: run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "kill_node" => {
            let Some(node_id) = req.node_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "node_id required for kill_node" })),
                )
                    .into_response();
            };
            let iter = req.iter.unwrap_or(1);

            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            tmux_session_manager::kill(&state.tmux_socket(), &session_name);

            let fail_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeFailed,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "reason": "killed via kill_node command",
                    "source": "kill_node",
                })),
            };
            if let Err(e) = append_event(&state, &fail_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "kill_node",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append kill_node command event: {e}");
            }

            info!("kill_node: node {node_id} iter {iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "restart_node" => {
            let Some(node_id) = req.node_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "node_id required for restart_node" })),
                )
                    .into_response();
            };
            let iter = req.iter.unwrap_or(1);

            // Transition guard (#212 / #196): restart_node is mutually
            // exclusive with the scheduler's own re-fire — validate against
            // the projected state BEFORE killing anything, so a stale-view
            // restart of an old iter never races a newer live iteration.
            {
                let events = match load_events(&state.db, &run_id).await {
                    Ok(e) => e,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                            .into_response();
                    }
                };
                let run_state = event_log::project(&events);
                let restart_probe = event_log::Event {
                    id: None,
                    run_id: run_id.clone(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeStarted,
                    node_id: Some(node_id.clone()),
                    iter: Some(iter),
                    payload: None,
                };
                if let transition_guard::Verdict::Reject { reason } =
                    transition_guard::validate_transition(run_state.as_ref(), &restart_probe)
                {
                    warn!("restart_node rejected for {node_id} iter {iter} in {run_id}: {reason}");
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({ "error": reason })),
                    )
                        .into_response();
                }
            }

            // Kill existing session
            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            tmux_session_manager::kill(&state.tmux_socket(), &session_name);

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: Some(node_id.clone()),
                iter: Some(iter),
                payload: Some(serde_json::json!({
                    "command": "restart_node",
                    "node_id": node_id,
                    "iter": iter,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                error!("failed to append restart_node command event: {e}");
            }

            // Re-spawn the node
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            let repo_root = effective_repo_root(&state, &run_state);
            let pipeline_path = {
                let run_scoped = run_scoped_pipeline_path(&repo_root, &run_id);
                if run_scoped.exists() {
                    run_scoped
                } else {
                    resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
                }
            };
            let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "cannot read pipeline").into_response();
            };
            let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "cannot parse pipeline")
                    .into_response();
            };

            let pipeline = parse_result.pipeline;
            if let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) {
                let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
                let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
                let resolved_vars = resolve_run_variables(&pipeline, &events);

                let spawn_ctx = SpawnContext {
                    pipeline: &pipeline,
                    run_id: &run_id,
                    pipeline_path: &pipeline_path,
                    worktree_dir: &worktree_dir,
                    artifacts_dir: &artifacts_dir,
                    resolved_vars: &resolved_vars,
                    repo_root: &repo_root,
                };

                spawn_node(&state, &spawn_ctx, node, iter).await;
            }

            info!("restart_node: node {node_id} iter {iter} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "inject_artifact" => {
            let Some(path) = req.path else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "path required for inject_artifact" })),
                )
                    .into_response();
            };
            let Some(content) = req.content else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "content required for inject_artifact" })),
                )
                    .into_response();
            };

            let requested = std::path::Path::new(&path);
            if requested.is_absolute()
                || requested
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "path traversal not allowed" })),
                )
                    .into_response();
            }

            let repo_root = match load_events(&state.db, &run_id).await {
                Ok(events) => match event_log::project(&events) {
                    Some(run_state) => effective_repo_root(&state, &run_state),
                    None => state.repo_root.clone(),
                },
                Err(_) => state.repo_root.clone(),
            };
            let worktree_dir = worktree_dir_for_run(&repo_root, &run_id);
            let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
            let full_path = artifacts_dir.join(requested);

            if let Some(parent) = full_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("failed to create dir: {e}") })),
                    )
                        .into_response();
                }
            }
            if let Err(e) = std::fs::write(&full_path, &content) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("failed to write artifact: {e}") })),
                )
                    .into_response();
            }

            let cmd_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::CommandIssued,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "command": "inject_artifact",
                    "path": path,
                })),
            };
            if let Err(e) = append_event(&state, &cmd_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("inject_artifact: {path} in run {run_id}");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "rename_run" => {
            let new_name = req.name.unwrap_or_default();
            let rename_event = event_log::Event {
                id: None,
                run_id: run_id.clone(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunRenamed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "name": new_name })),
            };
            if let Err(e) = append_event(&state, &rename_event).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
            }

            info!("rename_run: run {run_id} renamed to {:?}", new_name);
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        "cleanup_run" => cleanup_run(&state, &run_id).await,
        "retry_all" => {
            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };
            let run_state = match event_log::project(&events) {
                Some(s) => s,
                None => {
                    return (StatusCode::NOT_FOUND, "run not found").into_response();
                }
            };

            let is_terminal = matches!(
                run_state.status,
                event_log::RunStatus::Completed
                    | event_log::RunStatus::Failed
                    | event_log::RunStatus::Halted
            );
            if !is_terminal {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": format!("retry_all requires a terminal state, run is {:?}", run_state.status)
                    })),
                )
                    .into_response();
            }

            let run_started_event = events
                .iter()
                .find(|e| e.kind == event_log::EventKind::RunStarted);
            let run_started_payload = run_started_event.and_then(|e| e.payload.as_ref());

            let pipeline_name = run_state.pipeline_name.clone();
            let input = run_state.input.clone().unwrap_or_default();
            let variables: HashMap<String, serde_yaml::Value> = run_started_payload
                .and_then(|p| p.get("variables"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let target_repo = run_state.target_repo.clone();
            let source_branch = run_state.source_branch.clone();

            // Archive the current run (cleanup disk resources, keep events)
            let archive_resp = cleanup_run(&state, &run_id).await;
            let archive_status = archive_resp.into_response().status();
            if !archive_status.is_success() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "failed to archive original run" })),
                )
                    .into_response();
            }

            let new_run_req = CreateRunRequest {
                pipeline: pipeline_name,
                input,
                variables,
                pipeline_id: None,
                target_repo,
                source_branch,
                name: None,
                triggered_by: None,
            };
            let new_run_resp = create_run_core(&state, new_run_req, Vec::new()).await;

            info!("retry_all: archived run {run_id}, created new run");
            new_run_resp
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown command: {other}") })),
        )
            .into_response(),
    }
}

/// Re-evaluate the scheduler after a command (resume_run, extend_cycle).
/// Loads the pipeline and run state, resolves variables (including cycle extensions),
/// then re-evaluates outgoing edges of all completed nodes to find newly ready spawns.
async fn re_evaluate_after_command(state: &AppState, run_id: &str) {
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            error!("re_evaluate_after_command: failed to load events: {e}");
            return;
        }
    };
    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => return,
    };

    let repo_root = effective_repo_root(state, &run_state);
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };

    let pipeline = parse_result.pipeline;
    let worktree_dir = worktree_dir_for_run(&repo_root, run_id);
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let mut resolved_vars = resolve_run_variables(&pipeline, &events);

    // Apply cycle extensions to variables: for each extend_cycle command,
    // find variable references in outgoing edges of the target node and bump them.
    let extensions = event_log::collect_cycle_extensions(&events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }

    // Apply manager loop-region routes (ADR-0011 / #152). A `bump_region` raises
    // the region's effective `max_iter` by the bumped amount; when that cap is a
    // `$var` reference, bumping the variable lifts the `iter >= max` exit guard
    // so the region runs the extra laps after `resume_run`. (A literal cap is the
    // region engine's bound — #148 — and reads the recorded route directly.)
    let region_routes = event_log::collect_region_routes(&events);
    for (region_id, route) in &region_routes {
        if route.bumped_by <= 0 {
            continue;
        }
        if let Some(region) = pipeline.loops.iter().find(|r| &r.id == region_id) {
            if let Some(serde_yaml::Value::String(s)) = &region.max_iter {
                if let Some(var_name) = s.strip_prefix('$') {
                    if let Some(val) = resolved_vars.get_mut(var_name) {
                        if let Some(n) = val.as_i64() {
                            *val = serde_yaml::Value::Number(serde_yaml::Number::from(
                                n + route.bumped_by,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Find completed nodes whose outgoing edges might now fire with updated vars
    let completed_node_ids: Vec<String> = run_state
        .nodes
        .values()
        .filter(|n| n.status == event_log::NodeStatus::Completed)
        .map(|n| n.node_id.clone())
        .collect();

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
        repo_root: &repo_root,
    };

    let frontmatter_by_node = resolve_completed_frontmatter(&pipeline, &run_state, &artifacts_dir);

    for completed_node_id in &completed_node_ids {
        let source_iter = run_state
            .nodes
            .get(completed_node_id)
            .map(|n| n.iter)
            .unwrap_or(1);

        let frontmatter_fields =
            resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

        let actions = scheduler::evaluate_outgoing_edges_full(
            &pipeline,
            &run_state,
            completed_node_id,
            &resolved_vars,
            &frontmatter_fields,
            &frontmatter_by_node,
        );

        for action in &actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    // Transition guard (#212, ex-#201 dead already_active
                    // check): a re-evaluation only schedules MISSING work —
                    // never a node with a live iteration, never a completed
                    // iteration.
                    if let Some(reason) =
                        transition_guard::spawn_superfluous(&run_state, node_id, *iter)
                    {
                        info!("re_evaluate_after_command: skip spawn — {reason}");
                    } else if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                        spawn_node(state, &spawn_ctx, node, *iter).await;
                    }
                }
                scheduler::SchedulerAction::Halt { message } => {
                    emit_run_event(
                        state,
                        run_id,
                        event_log::EventKind::RunHalted,
                        Some(serde_json::json!({ "message": message })),
                    )
                    .await;
                    return;
                }
                scheduler::SchedulerAction::Complete => {
                    emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                    return;
                }
                scheduler::SchedulerAction::SwitchRouted {
                    node_id,
                    chosen_branch,
                } => {
                    emit_run_event(
                        state,
                        run_id,
                        event_log::EventKind::SwitchRouted,
                        Some(serde_json::json!({
                            "node_id": node_id,
                            "chosen_branch": chosen_branch,
                        })),
                    )
                    .await;
                    passthrough_switch_artifact(&spawn_ctx, node_id, chosen_branch, source_iter);
                }
                scheduler::SchedulerAction::LoopIterStarted { .. }
                | scheduler::SchedulerAction::LoopBreakReceived { .. }
                | scheduler::SchedulerAction::LoopMaxReached { .. }
                | scheduler::SchedulerAction::LoopDone { .. } => {
                    emit_loop_action(state, run_id, action).await;
                }
                scheduler::SchedulerAction::ForEachStarted {
                    foreach_node_id,
                    items,
                    ..
                } => {
                    emit_foreach_action(state, run_id, action).await;
                    deposit_foreach_items(&artifacts_dir, foreach_node_id, items);
                }
                scheduler::SchedulerAction::ForEachEmpty { .. }
                | scheduler::SchedulerAction::ForEachBreakReceived { .. }
                | scheduler::SchedulerAction::ForEachDone { .. } => {
                    emit_foreach_action(state, run_id, action).await;
                }
            }
        }
    }

    // Pass 1 may have appended events; re-project so pass 2 sees fresh state
    // (same race fix as handle_node_completion).
    let Some((fresh_events, fresh_run_state)) = reload_run_state(state, run_id).await else {
        return;
    };
    let mut fresh_resolved_vars = resolve_run_variables(&pipeline, &fresh_events);
    for (ext_node_id, additional) in &extensions {
        let var_refs = extract_variable_refs_from_outgoing_edges(&pipeline, ext_node_id);
        for var_name in var_refs {
            if let Some(val) = fresh_resolved_vars.get_mut(&var_name) {
                if let Some(n) = val.as_i64() {
                    *val = serde_yaml::Value::Number(serde_yaml::Number::from(n + additional));
                }
            }
        }
    }
    let fresh_spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &fresh_resolved_vars,
        repo_root: &repo_root,
    };

    // Check loop body completion for all loop nodes
    for loop_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Loop)
    {
        let loop_actions = scheduler::evaluate_loop_body_completion(
            &pipeline,
            &fresh_run_state,
            &loop_node.id,
            &fresh_resolved_vars,
        );
        for action in &loop_actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    // Transition guard (#212): schedule only missing work.
                    if let Some(reason) =
                        transition_guard::spawn_superfluous(&fresh_run_state, node_id, *iter)
                    {
                        info!("re_evaluate_after_command(loop): skip spawn — {reason}");
                    } else if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                        spawn_node(state, &fresh_spawn_ctx, node, *iter).await;
                    }
                }
                scheduler::SchedulerAction::Complete => {
                    emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                    return;
                }
                _ => emit_loop_action(state, run_id, action).await,
            }
        }
    }

    // Check foreach body completion for all foreach nodes
    for foreach_node in pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::ForEach)
    {
        let foreach_actions = scheduler::evaluate_foreach_body_completion(
            &pipeline,
            &fresh_run_state,
            &foreach_node.id,
        );
        for action in &foreach_actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    // Transition guard (#212): schedule only missing work.
                    if let Some(reason) =
                        transition_guard::spawn_superfluous(&fresh_run_state, node_id, *iter)
                    {
                        info!("re_evaluate_after_command(foreach): skip spawn — {reason}");
                    } else if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                        spawn_node(state, &fresh_spawn_ctx, node, *iter).await;
                    }
                }
                scheduler::SchedulerAction::Complete => {
                    emit_run_event(state, run_id, event_log::EventKind::RunCompleted, None).await;
                    return;
                }
                _ => emit_foreach_action(state, run_id, action).await,
            }
        }
    }
}

/// Extract variable references ($name) from when clauses on Switch output ports
/// reachable from outgoing edges of a node.
fn extract_variable_refs_from_outgoing_edges(
    pipeline: &pipeline::PipelineDef,
    node_id: &str,
) -> Vec<String> {
    let mut refs = Vec::new();
    for edge in &pipeline.edges {
        if edge.source.node != node_id {
            continue;
        }
        let target_node = pipeline.nodes.iter().find(|n| n.id == edge.target.node);
        if let Some(node) = target_node {
            if node.node_type == pipeline::NodeType::Switch {
                for port in &node.outputs {
                    if let Some(ref when) = port.when {
                        collect_yaml_var_refs(when, &mut refs);
                    }
                }
            }
            if let Some(ref max_iter) = node.max_iter {
                collect_yaml_var_refs(max_iter, &mut refs);
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

fn collect_yaml_var_refs(val: &serde_yaml::Value, refs: &mut Vec<String>) {
    match val {
        serde_yaml::Value::String(s) if s.starts_with('$') => {
            refs.push(s[1..].to_string());
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map {
                collect_yaml_var_refs(v, refs);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for v in seq {
                collect_yaml_var_refs(v, refs);
            }
        }
        _ => {}
    }
}

// --- Session attach ---

#[derive(Serialize)]
struct AttachResponse {
    ok: bool,
    session: String,
    terminal: String,
}

async fn session_attach(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    let terminal = detect_terminal();

    match spawn_terminal_attach(&terminal, &state.tmux_socket(), &session_id) {
        Ok(()) => {
            info!("Attached terminal {terminal} to session {session_id}");
            (
                StatusCode::OK,
                Json(AttachResponse {
                    ok: true,
                    session: session_id,
                    terminal,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to attach: {e}") })),
        )
            .into_response(),
    }
}

async fn manager_attach(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let session_name = tmux_session_manager::manager_session_name(&run_id);
    let socket = state.tmux_socket();

    if !tmux_session_manager::session_exists(&socket, &session_name) {
        return (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": format!("manager session {session_name} not found") }),
            ),
        )
            .into_response();
    }

    let terminal = detect_terminal();
    match spawn_terminal_attach(&terminal, &socket, &session_name) {
        Ok(()) => {
            info!("Attached terminal {terminal} to manager session {session_name}");
            (
                StatusCode::OK,
                Json(AttachResponse {
                    ok: true,
                    session: session_name,
                    terminal,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("failed to attach: {e}") })),
        )
            .into_response(),
    }
}

fn detect_terminal() -> String {
    // PDO_TERMINAL env var overrides
    if let Ok(t) = std::env::var("PDO_TERMINAL") {
        if !t.is_empty() {
            return t;
        }
    }

    // Heuristic: check TERM_PROGRAM
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        let tp_lower = tp.to_lowercase();
        if tp_lower.contains("kitty") {
            return "kitty".into();
        }
        if tp_lower.contains("alacritty") {
            return "alacritty".into();
        }
        if tp_lower.contains("iterm") {
            return "open -a iTerm".into();
        }
    }

    // OS heuristic
    if cfg!(target_os = "macos") {
        return "open -a Terminal".into();
    }

    // Linux: check for common terminals in PATH
    for candidate in &["kitty", "alacritty", "gnome-terminal", "konsole", "xterm"] {
        if which_exists(candidate) {
            return (*candidate).into();
        }
    }

    "xterm".into()
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn spawn_terminal_attach(terminal: &str, socket: &str, session_name: &str) -> Result<()> {
    let parts: Vec<&str> = terminal.split_whitespace().collect();
    let (cmd, prefix_args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty terminal command"))?;

    let escaped_name = shell_escape(session_name);
    let escaped_socket = shell_escape(socket);
    // Always attach via the daemon's private socket so the user's terminal
    // pop-out reaches the right tmux server, even when other daemons run.
    let tmux_cmd = format!("tmux -L {escaped_socket} attach -t {escaped_name}");

    let mut command = std::process::Command::new(cmd);
    command.args(prefix_args);

    match *cmd {
        "gnome-terminal" => {
            command.args(["--", "bash", "-c", &tmux_cmd]);
        }
        "konsole" => {
            command.args(["-e", "bash", "-c", &tmux_cmd]);
        }
        "kitty" => {
            command.args(["bash", "-c", &tmux_cmd]);
        }
        "alacritty" => {
            command.args(["-e", "bash", "-c", &tmux_cmd]);
        }
        "xterm" => {
            command.args(["-e", &tmux_cmd]);
        }
        "open" => {
            // macOS: open -a Terminal <script>
            // We create a temp script that attaches
            let script = format!("#!/bin/bash\n{tmux_cmd}\n");
            let script_path = std::env::temp_dir().join(format!("pdo-attach-{session_name}.sh"));
            std::fs::write(&script_path, &script)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
            }
            command.arg(script_path);
        }
        _ => {
            command.args(["-e", "bash", "-c", &tmux_cmd]);
        }
    }

    let child = command.spawn().context("failed to spawn terminal")?;
    std::mem::forget(child);

    Ok(())
}

async fn cleanup_run(state: &AppState, run_id: &str) -> Response {
    let events = match load_events(&state.db, run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let run_state = match event_log::project(&events) {
        Some(s) => s,
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    if run_state.status == event_log::RunStatus::Archived {
        return (StatusCode::CONFLICT, "run is already archived").into_response();
    }

    let socket = state.tmux_socket();
    for node in run_state.nodes.values() {
        let session_name =
            tmux_session_manager::node_session_name(run_id, &node.node_id, node.iter);
        tmux_session_manager::kill(&socket, &session_name);
    }
    let mgr_session = tmux_session_manager::manager_session_name(run_id);
    tmux_session_manager::kill(&socket, &mgr_session);

    let repo_root = effective_repo_root(state, &run_state);
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);

    // Remove sub-worktrees (nodes/) before the main worktree
    let nodes_dir = run_dir.join("nodes");
    if nodes_dir.exists() {
        for node_entry in std::fs::read_dir(&nodes_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            for iter_entry in std::fs::read_dir(node_entry.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let sub_wt = iter_entry.path();
                if sub_wt.is_dir() {
                    let _ = std::process::Command::new("git")
                        .args(["worktree", "remove", "--force"])
                        .arg(&sub_wt)
                        .current_dir(&repo_root)
                        .output();
                }
            }
        }
    }

    let worktree_dir = run_dir.join("worktree");
    if worktree_dir.exists() {
        let output = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_dir)
            .current_dir(&repo_root)
            .output();
        if let Ok(o) = output {
            if !o.status.success() {
                warn!(
                    "git worktree remove failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
    }

    // Remove all branches for this run (pipeline branch + sub-worktree branches)
    for pattern in [format!("pdo/run-{run_id}"), format!("pdo/sub-{run_id}*")] {
        let branch_output = std::process::Command::new("git")
            .args(["branch", "--list", &pattern])
            .current_dir(&repo_root)
            .output();
        if let Ok(o) = branch_output {
            let branches = String::from_utf8_lossy(&o.stdout);
            for branch in branches.lines() {
                let branch = branch.trim().trim_start_matches("* ");
                if !branch.is_empty() {
                    let _ = std::process::Command::new("git")
                        .args(["branch", "-D", branch])
                        .current_dir(&repo_root)
                        .output();
                }
            }
        }
    }

    if run_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&run_dir) {
            warn!("failed to remove run dir {}: {e}", run_dir.display());
        }
    }

    let archived_event = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunArchived,
        node_id: None,
        iter: None,
        payload: None,
    };
    if let Err(e) = append_event(state, &archived_event).await {
        error!("failed to append run_archived: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response();
    }

    info!("Run {run_id} archived (cleanup complete)");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "archived" })),
    )
        .into_response()
}

/// DELETE /runs/{run_id} — permanently forget an archived run.
///
/// Removes every event log row for the run. The run will no longer appear in
/// listings, projections, or post-mortem queries. Only allowed once the run
/// is `Archived` (i.e. `cleanup_run` has already torn down its worktrees and
/// branches), so we never strand on-disk state by deleting events for a live
/// run.
async fn forget_run(
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
        None => {
            return (StatusCode::NOT_FOUND, "run not found").into_response();
        }
    };

    if run_state.status != event_log::RunStatus::Archived {
        return (
            StatusCode::CONFLICT,
            "run must be archived (cleanup_run) before it can be forgotten",
        )
            .into_response();
    }

    if let Err(e) = sqlx::query("DELETE FROM events WHERE run_id = ?")
        .bind(&run_id)
        .execute(&state.db)
        .await
    {
        error!("failed to delete events for {run_id}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response();
    }

    info!("Run {run_id} forgotten (event log purged)");

    // Notify connected websocket clients so the runs list refreshes. We piggy-back
    // on the pipeline-broadcast channel because the event channel only carries
    // typed `event_log::Event` records — and we just deleted every event for
    // this run, so there's nothing meaningful to project.
    let _ = state.pipeline_tx.send(serde_json::json!({
        "type": "run_forgotten",
        "run_id": run_id,
    }));

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "forgotten" })),
    )
        .into_response()
}

// --- WebSocket handler with event broadcasting ---

async fn ws_handler(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    info!("WebSocket client connected");
    let ready = serde_json::json!({ "type": "ready" });
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut event_rx = state.event_tx.subscribe();
    let mut pipeline_rx = state.pipeline_tx.subscribe();
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let msg = serde_json::json!({
                    "type": "heartbeat",
                    "ts": unix_timestamp_secs(),
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            event = event_rx.recv() => {
                match event {
                    Ok(ev) => {
                        let msg = serde_json::json!({
                            "type": "event",
                            "event": ev,
                        });
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WebSocket client lagged by {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            pipeline_event = pipeline_rx.recv() => {
                match pipeline_event {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    info!("WebSocket client disconnected");
}

fn unix_timestamp_secs() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    format!("{secs}.{millis:03}")
}

// --- Target repo validation ---

fn validate_target_repo(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err("target_repo must be an absolute path".into());
    }
    if !p.is_dir() {
        return Err(format!(
            "target_repo does not exist or is not a directory: {path}"
        ));
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&p)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(p),
        _ => Err(format!("target_repo is not a git repository: {path}")),
    }
}

fn validate_source_branch(repo: &Path, branch: &str) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--list", branch])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.trim().is_empty() {
                Err(format!(
                    "branch '{branch}' does not exist in {}",
                    repo.display()
                ))
            } else {
                Ok(())
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("git branch --list failed: {stderr}"))
        }
        Err(e) => Err(format!("failed to run git: {e}")),
    }
}

fn list_branches(repo: &Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            Ok(stdout.lines().map(|l| l.to_string()).collect())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("git branch failed: {stderr}"))
        }
        Err(e) => Err(format!("failed to run git: {e}")),
    }
}

fn effective_repo_root(state: &AppState, run_state: &event_log::RunState) -> PathBuf {
    run_state
        .target_repo
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| state.repo_root.clone())
}

fn worktree_dir_for_run(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("worktree")
}

// --- Pipeline path resolution ---

fn run_scoped_pipeline_path(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("pipeline.yaml")
}

fn run_scoped_prompts_dir(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("pipeline.prompts")
}

fn copy_pipeline_to_run(
    repo_root: &std::path::Path,
    pipeline_path: &std::path::Path,
    run_id: &str,
) -> Result<()> {
    let dest_yaml = run_scoped_pipeline_path(repo_root, run_id);
    if let Some(parent) = dest_yaml.parent() {
        std::fs::create_dir_all(parent).context("create run dir")?;
    }
    std::fs::copy(pipeline_path, &dest_yaml).context("copy pipeline yaml")?;

    let stem = pipeline_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pipeline");
    let src_prompts = pipeline_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(format!("{stem}.prompts"));
    // Always create the prompts dir — even when the template has none — so the
    // file watcher can attach to it at run creation (it is watched
    // non-recursively and there is no later hook to pick it up).
    let dest_prompts = run_scoped_prompts_dir(repo_root, run_id);
    std::fs::create_dir_all(&dest_prompts).context("create prompts dir")?;
    if src_prompts.is_dir() {
        for entry in std::fs::read_dir(&src_prompts)
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name() {
                    std::fs::copy(&p, dest_prompts.join(name))?;
                }
            }
        }
    }
    Ok(())
}

fn augment_run_state_from_disk(run_state: &mut event_log::RunState, repo_root: &std::path::Path) {
    let yaml_path = run_scoped_pipeline_path(repo_root, &run_state.run_id);
    let Ok(yaml) = std::fs::read_to_string(&yaml_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipe = &parse_result.pipeline;

    run_state.node_defs = pipe.nodes.iter().map(node_def_from_pipeline).collect();
    run_state.edges = pipe.edges.iter().map(edge_info_from_pipeline).collect();
}

fn port_brief(p: &pipeline::Port, default_side: &str) -> event_log::PortBrief {
    event_log::PortBrief {
        name: p.name.clone(),
        side: p
            .side
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_side.into()),
        description: p.description.clone(),
    }
}

fn node_def_from_pipeline(n: &pipeline::NodeDef) -> event_log::NodeDefInfo {
    event_log::NodeDefInfo {
        id: n.id.clone(),
        name: Some(n.name.clone()),
        node_type: match n.node_type {
            pipeline::NodeType::DocOnly => "doc-only".into(),
            pipeline::NodeType::CodeMutating => "code-mutating".into(),
            pipeline::NodeType::Start => "start".into(),
            pipeline::NodeType::End => "end".into(),
            pipeline::NodeType::Switch => "switch".into(),
            pipeline::NodeType::Loop => "loop".into(),
            pipeline::NodeType::ForEach => "for-each".into(),
            pipeline::NodeType::Merge => "merge".into(),
        },
        view_x: n.view.as_ref().map(|v| v.x),
        view_y: n.view.as_ref().map(|v| v.y),
        inputs: n.inputs.iter().map(|p| port_brief(p, "left")).collect(),
        outputs: n.outputs.iter().map(|p| port_brief(p, "right")).collect(),
    }
}

fn edge_info_from_pipeline(e: &pipeline::EdgeDef) -> event_log::EdgeInfo {
    event_log::EdgeInfo {
        source_node: e.source.node.clone(),
        source_port: e.source.port.clone(),
        target_node: e.target.node.clone(),
        target_port: e.target.port.clone(),
        halt_message: e.reason.clone(),
        when_clause: None,
    }
}

fn resolve_pipeline_path(repo_root: &std::path::Path, pipeline_name: &str) -> PathBuf {
    // Check repo-scoped pipelines first, then user-scoped
    let repo_path = repo_root
        .join(".pdo")
        .join("pipelines")
        .join(format!("{pipeline_name}.yaml"));
    if repo_path.exists() {
        return repo_path;
    }

    if let Some(home) = dirs_next_home() {
        let user_path = home
            .join(".pdo")
            .join("pipelines")
            .join(format!("{pipeline_name}.yaml"));
        if user_path.exists() {
            return user_path;
        }
    }

    repo_path
}

fn repo_pipeline_path(repo_root: &std::path::Path, pipeline_name: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("pipelines")
        .join(format!("{pipeline_name}.yaml"))
}

/// Resolve a pipeline YAML path honoring an explicit `scope` when one is given.
///
/// `Some("repo")` / `Some("user")` resolve *strictly* to that store: the
/// operation never falls through to a same-named file in another store, which
/// is the root cause of #216 (a `user`/`library` delete destroying a `repo`
/// file). `None` keeps the historical best-effort behavior (repo first, then
/// user, defaulting to repo). `Some("library")` lives in a different on-disk
/// store and is handled by callers via `library_store::pipelines`, so it is
/// treated here as "unknown" and falls back to the default — callers must
/// branch on `library` *before* calling this.
fn resolve_pipeline_path_scoped(
    repo_root: &std::path::Path,
    pipeline_name: &str,
    scope: Option<&str>,
) -> PathBuf {
    match scope {
        Some("repo") => repo_pipeline_path(repo_root, pipeline_name),
        Some("user") => dirs_next_home()
            .map(|home| {
                home.join(".pdo")
                    .join("pipelines")
                    .join(format!("{pipeline_name}.yaml"))
            })
            // No HOME → keep a well-formed path rather than panicking; the
            // subsequent `exists()` check turns it into a clean 404.
            .unwrap_or_else(|| repo_pipeline_path(repo_root, pipeline_name)),
        _ => resolve_pipeline_path(repo_root, pipeline_name),
    }
}

fn resolve_run_pipeline_path(
    repo_root: &std::path::Path,
    run_id: &str,
    pipeline_name: &str,
) -> PathBuf {
    let run_scoped = run_scoped_pipeline_path(repo_root, run_id);
    if run_scoped.exists() {
        run_scoped
    } else {
        resolve_pipeline_path(repo_root, pipeline_name)
    }
}

async fn check_output_validation_with_retry(
    state: &AppState,
    pipeline_path: &std::path::Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &std::path::Path,
    run_id: &str,
    run_state: &event_log::RunState,
) -> Option<Response> {
    let yaml = std::fs::read_to_string(pipeline_path).ok()?;
    let parse_result = pipeline::parse_pipeline(&yaml).ok()?;
    let Err(validation_error) =
        outputs_validator::validate(&parse_result.pipeline, node_id, iter, artifacts_dir)
    else {
        return None;
    };

    match validation_error {
        outputs_validator::ValidationError::MissingOutputs(missing) => Some(
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "missing_outputs",
                    "missing": missing,
                })),
            )
                .into_response(),
        ),
        outputs_validator::ValidationError::FrontmatterMismatch(violations) => {
            let retries = run_state
                .nodes
                .get(node_id)
                .map(|n| n.frontmatter_retries)
                .unwrap_or(0);

            let violation_details: Vec<serde_json::Value> = violations
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "port": v.port,
                        "field": v.field,
                        "reason": v.reason,
                    })
                })
                .collect();

            if retries >= 1 {
                let fail_event = event_log::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::NodeFailed,
                    node_id: Some(node_id.to_string()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "reason": "output validation failed",
                        "violations": violation_details,
                    })),
                };
                let _ = append_event(state, &fail_event).await;

                let run_failed = event_log::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::RunFailed,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({
                        "reason": format!("node {node_id} failed output validation after retry")
                    })),
                };
                let _ = append_event(state, &run_failed).await;

                warn!("Node {node_id} failed output validation after retry in run {run_id}");
                Some(
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": "frontmatter_retry_exhausted",
                            "violations": violation_details,
                        })),
                    )
                        .into_response(),
                )
            } else {
                let msg = outputs_validator::corrective_message(&violations);
                let session_name = tmux_session_manager::node_session_name(run_id, node_id, iter);
                tmux_session_manager::send_keys(&state.tmux_socket(), &session_name, &msg);

                let retry_event = event_log::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::FrontmatterRetryPending,
                    node_id: Some(node_id.to_string()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "violations": violation_details,
                    })),
                };
                let _ = append_event(state, &retry_event).await;

                info!("Frontmatter mismatch for node {node_id} in run {run_id} — corrective message sent, awaiting retry");
                Some(
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": "frontmatter_retry_pending",
                            "violations": violation_details,
                        })),
                    )
                        .into_response(),
                )
            }
        }
    }
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// --- Git worktree ---

fn sub_worktree_path(
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("iter-{iter}"))
}

fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("pdo/sub-{run_id}-{node_id}-iter-{iter}")
}

/// Path to the persisted pane snapshot for a terminal NodeRun iteration (#205).
///
/// Lives in the run's node dir — NOT inside the per-iter sub-worktree — so the
/// post-mortem pane survives worktree removal and the session reap.
fn pane_snapshot_path(
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("pane-iter-{iter}.snapshot"))
}

/// Reap a NodeRun's tmux session on its terminal transition (#205).
///
/// Persists a pane snapshot under the run's node dir (so post-mortem inspection
/// — a PDO differentiator — keeps working after the session is gone), then
/// kills the session. Best-effort: a missing session (already reaped, never
/// spawned) is a silent no-op; a capture/write failure still proceeds to kill
/// so a terminal node never leaks a live session toward the tmux-collapse
/// point (#77/#78). Honours the one-live-iteration invariant by freeing the
/// session the moment the iteration is terminal.
fn reap_node_session(
    state: &AppState,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
) {
    let socket = state.tmux_socket();
    let session = tmux_session_manager::node_session_name(run_id, node_id, iter);

    if let Some(content) = tmux_session_manager::capture(&socket, &session) {
        let snapshot_path = pane_snapshot_path(repo_root, run_id, node_id, iter);
        if let Some(parent) = snapshot_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&snapshot_path, content) {
            warn!("reap: failed to persist pane snapshot for {session}: {e}");
        }
    }

    tmux_session_manager::kill(&socket, &session);
    info!("Reaped tmux session {session} on terminal transition");
}

fn create_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    sub_branch: &str,
    base_branch: &str,
) -> Result<()> {
    std::fs::create_dir_all(
        sub_worktree_dir
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", sub_branch])
        .arg(sub_worktree_dir)
        .arg(base_branch)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add for sub-worktree")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add (sub) failed: {stderr}");
    }

    info!("Created sub-worktree at {}", sub_worktree_dir.display());
    Ok(())
}

enum MergeResult {
    Success,
    Conflict(String),
    ConflictPendingResolution(String),
}

#[cfg(test)]
fn commit_and_merge_sub_worktree(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
) -> Result<MergeResult> {
    commit_and_merge_sub_worktree_inner(
        sub_worktree_dir,
        pipeline_worktree_dir,
        sub_branch,
        node_id,
        iter,
        false,
    )
}

fn commit_and_merge_sub_worktree_inner(
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
    keep_conflict: bool,
) -> Result<MergeResult> {
    let _ = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git add failed in sub-worktree")?;

    let status_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(sub_worktree_dir)
        .output()
        .context("git diff --cached failed")?;

    if !status_output.status.success() {
        let commit_msg = format!("{node_id} iter-{iter}: completed");
        let output = std::process::Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(sub_worktree_dir)
            .output()
            .context("git commit failed in sub-worktree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git commit in sub-worktree failed: {stderr}");
        }
    }

    let output = std::process::Command::new("git")
        .args(["merge", sub_branch, "--no-edit"])
        .current_dir(pipeline_worktree_dir)
        .output()
        .context("git merge failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if keep_conflict {
            return Ok(MergeResult::ConflictPendingResolution(stderr.to_string()));
        }
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(pipeline_worktree_dir)
            .output();
        return Ok(MergeResult::Conflict(stderr.to_string()));
    }

    // Sub-worktree and branch are intentionally kept alive (refs #32).
    // They survive until cleanup_run removes them, allowing prompt/artifact
    // inspection and tmux re-attach for completed iterations.

    info!("Merged sub-worktree {sub_branch} into pipeline branch");
    Ok(MergeResult::Success)
}

fn worktree_has_tracked_changes(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_dir)
        .output()
        .context("git status failed")?;

    let status = String::from_utf8_lossy(&output.stdout);
    Ok(status.lines().any(|line| !line.starts_with("??")))
}

/// Check that no conflict markers remain in any tracked file.
fn has_conflict_markers(worktree_dir: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new("git")
        .args(["grep", "-rlE", "^<{7} |^={7}$|^>{7} "])
        .current_dir(worktree_dir)
        .output()
        .context("git grep failed")?;

    Ok(output.status.success() && !output.stdout.is_empty())
}

/// Validate merge resolution: no conflict markers, clean working tree.
fn validate_merge_resolution(worktree_dir: &std::path::Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();

    if has_conflict_markers(worktree_dir)? {
        problems.push("conflict markers remain in tracked files".to_string());
    }

    if worktree_has_tracked_changes(worktree_dir)? {
        problems.push("working tree is not clean (uncommitted changes)".to_string());
    }

    Ok(problems)
}

const MERGE_RESOLVER_NODE_ID: &str = "__merge_resolver__";

const FALLBACK_MERGE_RESOLVER_PROMPT: &str = "\
You are the Merge Resolver. A git merge conflict occurred. \
Resolve all conflicts, remove all conflict markers, and commit the merge.";

fn load_merge_resolver_prompt(repo_root: &std::path::Path) -> String {
    let path = repo_root.join("prompts/builtin/merge-resolver.md");
    std::fs::read_to_string(&path).unwrap_or_else(|_| FALLBACK_MERGE_RESOLVER_PROMPT.to_string())
}

fn create_worktree(
    repo_root: &std::path::Path,
    worktree_dir: &std::path::Path,
    branch_name: &str,
    source_ref: &str,
) -> Result<()> {
    std::fs::create_dir_all(worktree_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name])
        .arg(worktree_dir)
        .arg(source_ref)
        .current_dir(repo_root)
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    info!("Created worktree at {}", worktree_dir.display());
    Ok(())
}

// Re-export tmux_session_manager public items that existing tests reference.
pub use guard_runner::GUARD_TIMEOUT_MS_OVERRIDE_ENV;
pub use tmux_session_manager::{build_tmux_script, TMUX_CMD_OVERRIDE_ENV};

// --- Static file serving ---

async fn static_handler(uri: Uri) -> Response {
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
<p>In dev mode, run the Vite frontend separately:<br><code>cd frontend && npm run dev</code></p>
</div>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    async fn test_state() -> Arc<AppState> {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (event_tx, _) = broadcast::channel(64);
        let (pipeline_tx, _) = broadcast::channel(16);
        Arc::new(AppState {
            db,
            event_tx,
            pipeline_tx,
            repo_root: std::env::current_dir().unwrap(),
            port: 0,
            merge_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
            trigger_tick_lock: tokio::sync::Mutex::new(()),
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(Mutex::new(None)),
            // Self-destructing no-op: should any test POST /runs and reach the
            // spawn path, the node session runs `true` and exits immediately —
            // never real claude, never a lingering session (#181).
            tmux_cmd_override: Some("exec true".to_string()),
        })
    }

    fn legacy_app() -> Router {
        // Backwards-compatible app without state for static-serving tests
        Router::new()
            .route(
                "/ws",
                get(|ws: WebSocketUpgrade| async { ws.on_upgrade(|_| async {}) }),
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
    async fn ws_connects_and_receives_ready() {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let state = test_state().await;
        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{addr}/ws");
        let (mut ws, _) = connect_async(&url).await.unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let text = msg.into_text().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["type"], "ready");
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

    #[tokio::test]
    async fn list_runs_empty() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn sessions_endpoint_reports_live_count_and_cap() {
        // The status-bar counter (#159) reads `GET /sessions`: the live
        // NodeRun-session count and the configured cap. A single running node
        // counts as one live session.
        let state = test_state().await;
        let run_id = "sessions-run";
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
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
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
        )
        .await
        .unwrap();

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["live"], 1, "one running node = one live session");
        assert!(
            json["cap"].as_u64().unwrap() >= 1,
            "cap should be a positive integer"
        );
        assert_eq!(
            json["version"],
            env!("CARGO_PKG_VERSION"),
            "the status-bar payload carries the daemon version (#139)"
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
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": "fail-fast" })),
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["live"], 0,
            "a session-holding node in a terminal run must not count (#215)"
        );
    }

    #[tokio::test]
    async fn get_nonexistent_run_returns_404() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_done_and_run_lifecycle() {
        let state = test_state().await;

        // Manually append events to simulate a run
        let run_id = "test-run-1";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        };
        append_event(&state, &run_started).await.unwrap();

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        };
        append_event(&state, &node_started).await.unwrap();

        // Call node_done endpoint
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run-1/nodes/worker/done")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Check the run is now completed
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Completed);
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Completed
        );
    }

    #[tokio::test]
    async fn node_fail_marks_run_failed() {
        let state = test_state().await;

        let run_id = "test-fail-run";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        };
        append_event(&state, &run_started).await.unwrap();

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: None,
        };
        append_event(&state, &node_started).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-fail-run/nodes/worker/fail")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"reason": "something broke"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Failed);
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Failed
        );
    }

    async fn seed_completed_run(state: &Arc<AppState>, run_id: &str) {
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
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

    #[tokio::test]
    async fn cleanup_run_archives_and_preserves_events() {
        let state = test_state().await;
        let run_id = "cleanup-test-1";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
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
        assert_eq!(body["status"], "archived");

        // Events are preserved — run projects as Archived
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(!events.is_empty());
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Archived);
        // Node state is preserved
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Completed
        );
    }

    #[tokio::test]
    async fn cleanup_run_already_archived_returns_conflict() {
        let state = test_state().await;
        let run_id = "cleanup-conflict";
        seed_completed_run(&state, run_id).await;

        // First cleanup
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second cleanup — should conflict
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn forget_run_purges_events_when_archived() {
        let state = test_state().await;
        let run_id = "forget-test-1";
        seed_completed_run(&state, run_id).await;

        // Archive first
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Now forget
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/runs/{run_id}"))
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
        assert_eq!(body["status"], "forgotten");

        // Event log is empty for this run — projection now returns None
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(events.is_empty(), "event log must be empty after forget");
        assert!(event_log::project(&events).is_none());
    }

    #[tokio::test]
    async fn forget_run_rejects_live_run() {
        let state = test_state().await;
        let run_id = "forget-live";
        seed_completed_run(&state, run_id).await;
        // No cleanup — run is still in `Completed` status, not `Archived`.

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        // Events are NOT touched
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn forget_nonexistent_run_returns_404() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/runs/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn cleanup_nonexistent_run_returns_404() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/nonexistent/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_command_returns_bad_request() {
        let state = test_state().await;
        let run_id = "cmd-unknown";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "bogus_command"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rename_run_updates_display_name() {
        let state = test_state().await;
        let run_id = "rename-test";
        seed_completed_run(&state, run_id).await;

        // Rename the run
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "rename_run", "name": "My Feature"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify GET /runs/:id returns the name
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}"))
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
        assert_eq!(body["name"], "My Feature");
    }

    #[tokio::test]
    async fn rename_run_appears_in_list() {
        let state = test_state().await;
        let run_id = "rename-list-test";
        seed_completed_run(&state, run_id).await;

        // Rename the run
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "rename_run", "name": "Listed Name"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify GET /runs returns the name
        let app = build_router(state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Vec<serde_json::Value> = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let entry = body.iter().find(|r| r["run_id"] == run_id).unwrap();
        assert_eq!(entry["name"], "Listed Name");
    }

    #[tokio::test]
    async fn run_without_name_has_null_name_in_list() {
        let state = test_state().await;
        let run_id = "no-name-test";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Vec<serde_json::Value> = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let entry = body.iter().find(|r| r["run_id"] == run_id).unwrap();
        assert!(entry.get("name").is_none() || entry["name"].is_null());
    }

    #[tokio::test]
    async fn create_run_with_name() {
        let state = test_state().await;
        let run_id = "named-create";

        // Seed a run with a name in the RunStarted payload
        let event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "hello",
                "name": "Named Run"
            })),
        };
        append_event(&state, &event).await.unwrap();

        // Verify GET /runs/:id returns the name
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}"))
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
        assert_eq!(body["name"], "Named Run");
    }

    #[tokio::test]
    async fn archived_run_still_appears_in_list() {
        let state = test_state().await;
        let run_id = "list-archive-test";
        seed_completed_run(&state, run_id).await;

        // Archive it
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // List runs — should still include the archived run
        let app = build_router(state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["status"], "archived");

        // GET /runs/:id/events still returns full history
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/events"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(events.len() >= 5); // run_started + node_started + node_completed + run_completed + run_archived
    }

    #[tokio::test]
    async fn cleanup_run_removes_surviving_sub_worktrees_and_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "cleanup-sub-wt";
        let state = test_state_with_dir(repo).await;

        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;
        for ev in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("impl-1".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunCompleted,
                node_id: None,
                iter: None,
                payload: None,
            },
        ] {
            append_event(&state, &ev).await.unwrap();
        }

        // Create real worktrees on disk (simulating what the daemon would do)
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();
        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Sub-worktree must exist before cleanup (refs #32)
        assert!(sub_wt_dir.exists());

        // Run cleanup_run via the HTTP endpoint
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Sub-worktree directory removed
        assert!(
            !sub_wt_dir.exists(),
            "cleanup_run must remove sub-worktree directory"
        );

        // Pipeline worktree directory removed
        assert!(
            !wt_dir.exists(),
            "cleanup_run must remove pipeline worktree directory"
        );

        // Sub-branch removed
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branch_check.stdout);
        assert!(
            !branches.contains(&sub_branch),
            "cleanup_run must remove sub-branch; got: {branches}"
        );

        // Pipeline branch removed
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", &pipeline_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branch_check.stdout);
        assert!(
            !branches.contains(&pipeline_branch),
            "cleanup_run must remove pipeline branch; got: {branches}"
        );

        // Events are preserved
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(!events.is_empty());
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Archived);
    }

    #[tokio::test]
    async fn mark_node_done_command_completes_awaiting_node() {
        let state = test_state().await;

        let run_id = "test-interactive-run";
        // Simulate: run_started → node_started → node_awaiting_user
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "interactive" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeAwaitingUser,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        // Verify run is awaiting_user
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::AwaitingUser);

        // Call mark_node_done
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify run is completed (single-node pipeline)
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Completed);
        assert_eq!(
            run_state.nodes["griller"].status,
            event_log::NodeStatus::Completed
        );
    }

    #[tokio::test]
    async fn mark_node_done_emits_command_issued_event() {
        let state = test_state().await;

        let run_id = "test-mnd-cmd-event";
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "interactive" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeAwaitingUser,
                node_id: Some("griller".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "griller", "iter": 1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        let cmd_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == event_log::EventKind::CommandIssued)
            .collect();
        assert_eq!(cmd_events.len(), 1);
        let payload = cmd_events[0].payload.as_ref().unwrap();
        assert_eq!(payload["command"], "mark_node_done");
        assert_eq!(payload["node_id"], "griller");
    }

    #[tokio::test]
    async fn mark_node_done_rejects_unknown_command() {
        let state = test_state().await;

        let run_id = "test-unknown-cmd";
        let event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        };
        append_event(&state, &event).await.unwrap();

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "nope"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mark_node_done_requires_node_id() {
        let state = test_state().await;

        let run_id = "test-no-node-id";
        let event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test" })),
        };
        append_event(&state, &event).await.unwrap();

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "mark_node_done"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn detect_terminal_respects_env_override() {
        // Save and set PDO_TERMINAL
        let prev = std::env::var("PDO_TERMINAL").ok();
        std::env::set_var("PDO_TERMINAL", "my-custom-terminal");
        let result = detect_terminal();
        // Restore
        match prev {
            Some(v) => std::env::set_var("PDO_TERMINAL", v),
            None => std::env::remove_var("PDO_TERMINAL"),
        }
        assert_eq!(result, "my-custom-terminal");
    }

    #[tokio::test]
    async fn halted_run_appears_in_list_with_correct_status() {
        let state = test_state().await;
        let run_id = "halt-test-1";

        // Seed: run_started → node_started → node_completed → run_halted
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "halt-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("reviewer".into()),
                iter: Some(3),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("reviewer".into()),
                iter: Some(3),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunHalted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "message": "Blocked after 3 iterations" })),
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        // Verify run state is Halted
        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Halted);

        // Verify it appears in the list
        let app = build_router(state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let runs: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["status"], "halted");
    }

    #[tokio::test]
    async fn websocket_receives_events() {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let state = test_state().await;
        let app = build_router(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{addr}/ws");
        let (mut ws, _) = connect_async(&url).await.unwrap();

        // Consume the "ready" message
        let msg = ws.next().await.unwrap().unwrap();
        let text = msg.into_text().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["type"], "ready");

        // Append an event and check it arrives on the WebSocket
        let event = event_log::Event {
            id: None,
            run_id: "ws-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "ws-pipe" })),
        };
        append_event(&state, &event).await.unwrap();

        // Read the next message — should be the event (skip heartbeats)
        let deadline = time::Instant::now() + Duration::from_secs(10);
        loop {
            let msg = tokio::time::timeout_at(deadline, ws.next())
                .await
                .expect("timeout waiting for event")
                .unwrap()
                .unwrap();
            let text = msg.into_text().unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
            if parsed["type"] == "event" {
                assert_eq!(parsed["event"]["kind"], "run_started");
                break;
            }
        }
    }

    #[test]
    fn sub_worktree_path_follows_canonical_schema() {
        let path = sub_worktree_path(
            std::path::Path::new("/repo"),
            "20260101-120000-abc",
            "impl-1",
            1,
        );
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/runs/20260101-120000-abc/nodes/impl-1/iter-1")
        );
    }

    #[test]
    fn sub_worktree_branch_name() {
        let branch = sub_worktree_branch("20260101-120000-abc", "impl-1", 1);
        assert_eq!(branch, "pdo/sub-20260101-120000-abc-impl-1-iter-1");
    }

    fn run_state_with_node(
        run_id: &str,
        node_id: &str,
        node_type: &str,
        status: event_log::NodeStatus,
        iter: i64,
    ) -> event_log::RunState {
        let mut rs = event_log::RunState::new(run_id.into(), "test".into());
        rs.node_defs.push(event_log::NodeDefInfo {
            id: node_id.into(),
            name: None,
            node_type: node_type.into(),
            view_x: None,
            view_y: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
        });
        rs.nodes.insert(
            node_id.into(),
            event_log::NodeState {
                node_id: node_id.into(),
                status,
                iter,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
            },
        );
        rs
    }

    #[test]
    fn merged_without_event_flags_a_merged_uncompleted_code_node() {
        // #213 AC3: a code-mutating node whose sub-worktree branch is merged but
        // which never recorded a NodeCompleted is a git/event-log divergence.
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
            event_log::NodeStatus::Running,
            1,
        );
        let divergent = merged_without_event_nodes("20260101-120000-abc", &rs, |_branch| true);
        assert_eq!(
            divergent.len(),
            1,
            "the merged uncompleted node must be flagged"
        );
        assert_eq!(divergent[0].0, "impl");
        assert_eq!(divergent[0].1, "pdo/sub-20260101-120000-abc-impl-iter-1");
    }

    #[test]
    fn merged_without_event_ignores_completed_node() {
        // A merged branch WITH a NodeCompleted is the normal, consistent case.
        let rs = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
            event_log::NodeStatus::Completed,
            1,
        );
        let divergent = merged_without_event_nodes("20260101-120000-abc", &rs, |_branch| true);
        assert!(divergent.is_empty(), "a completed node is not a divergence");
    }

    #[test]
    fn merged_without_event_ignores_unmerged_and_doc_only() {
        // Doc-only nodes own no sub-worktree branch; an unmerged branch is fine.
        let doc = run_state_with_node(
            "20260101-120000-abc",
            "doc",
            "doc-only",
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &doc, |_| true).is_empty());

        let cm = run_state_with_node(
            "20260101-120000-abc",
            "impl",
            "code-mutating",
            event_log::NodeStatus::Running,
            1,
        );
        assert!(merged_without_event_nodes("20260101-120000-abc", &cm, |_| false).is_empty());
    }

    #[test]
    fn pane_snapshot_path_lives_outside_the_iter_worktree() {
        // The snapshot must survive worktree removal (it is the post-mortem
        // pane for a reaped terminal node, #205), so it lives in the node dir,
        // NOT inside the per-iter sub-worktree.
        let path = pane_snapshot_path(
            std::path::Path::new("/repo"),
            "20260101-120000-abc",
            "impl-1",
            2,
        );
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/runs/20260101-120000-abc/nodes/impl-1/pane-iter-2.snapshot")
        );
    }

    // --- run_stall_reason (run-level stall reconciliation, #214) ---

    fn doc_node_def(id: &str) -> pipeline::NodeDef {
        pipeline::NodeDef {
            id: id.into(),
            name: id.into(),
            node_type: pipeline::NodeType::DocOnly,
            inputs: Vec::new(),
            outputs: Vec::new(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn edge(src: &str, tgt: &str) -> pipeline::EdgeDef {
        pipeline::EdgeDef {
            source: pipeline::EdgeEndpoint {
                node: src.into(),
                port: "out".into(),
            },
            target: pipeline::EdgeEndpoint {
                node: tgt.into(),
                port: "in".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    /// Two doc-only nodes wired `a -> b`. `a` is the entry node (no incoming
    /// edges), `b` only spawns once `a` completes.
    fn linear_two_node_pipeline() -> pipeline::PipelineDef {
        pipeline::PipelineDef {
            name: "linear".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![doc_node_def("a"), doc_node_def("b")],
            edges: vec![edge("a", "b")],
            loops: Vec::new(),
            prompt_required: false,
        }
    }

    fn run_state_failed_node(node_id: &str) -> event_log::RunState {
        run_state_with_node(
            "20260613-012555-stall",
            node_id,
            "doc-only",
            event_log::NodeStatus::Failed,
            1,
        )
    }

    #[test]
    fn run_stall_reason_flags_a_running_run_with_a_failed_entry_and_nothing_schedulable() {
        // #214 added scope: a Running run whose only progress hinge (entry node
        // `a`) is Failed has no live node and nothing the scheduler can spawn —
        // `b` depends on `a` completing, which will never happen. The run would
        // otherwise sit Running forever (silent stall). Reconcile it terminal
        // with a cause that explains why nothing can advance.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_failed_node("a");

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(ready.is_empty(), "precondition: nothing schedulable");
        assert!(loop_seed.is_empty(), "precondition: no loop to seed");

        let reason = run_stall_reason(&pipeline, &run_state, &ready, &loop_seed)
            .expect("a Running run with no live node and nothing schedulable is stuck");
        assert!(
            reason.contains("a"),
            "stall cause {reason:?} must name the failed node blocking progress"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_fresh_run_with_a_schedulable_entry() {
        // A Running run where no node has started yet is the normal initial
        // state: the entry node is schedulable. Reconciling it would kill every
        // run the instant it is created. Must return None.
        let pipeline = linear_two_node_pipeline();
        let run_state = event_log::RunState::new("20260613-fresh".into(), "linear".into());

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());
        assert!(!ready.is_empty(), "precondition: entry node is schedulable");

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed).is_none(),
            "a run with a schedulable node has a path forward — never reconcile it"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_with_a_live_node() {
        // A node actively Running keeps the run alive even if the scheduler has
        // nothing else queued. Reconciling here would race the live work to a
        // false RunFailed. Must return None regardless of empty scheduler output.
        let pipeline = linear_two_node_pipeline();
        let run_state = run_state_with_node(
            "20260613-live",
            "a",
            "doc-only",
            event_log::NodeStatus::Running,
            1,
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed).is_none(),
            "a live Running node means the run can still finish — never reconcile it"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_awaiting_a_human() {
        // AwaitingUser is a legitimate live state (interactive node waiting on a
        // person). It is not a silent stall and must never be auto-failed.
        let pipeline = linear_two_node_pipeline();
        let mut run_state = run_state_with_node(
            "20260613-await",
            "a",
            "doc-only",
            event_log::NodeStatus::AwaitingUser,
            1,
        );
        run_state.status = event_log::RunStatus::AwaitingUser;

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed).is_none(),
            "an AwaitingUser run is waiting on a human, not stalled"
        );
    }

    #[test]
    fn run_stall_reason_never_flags_a_run_with_an_open_loop_region_awaiting_a_route() {
        // A bounded region that exhausted "unrouted" is surfaced as a Halt
        // (RunStatus::Halted) the Pipeline Manager can route by id
        // (manager-unstick-loop scenario). But if such a run is observed while
        // still Running with no live node — e.g. before its Halt event lands, or
        // a region awaiting a route with no failed node — it must NOT be
        // auto-failed: that would steal the manager's recovery path. A run with
        // an open (not-done) loop region and no terminal-failed node is not a
        // fail-fast stall.
        // `a -> b` where the edge is CONDITIONAL, so `b` is never an entry/ready
        // node (it only spawns on the producer's edge evaluation). With `a`
        // Completed and `b` not started, NOTHING is schedulable and NOT all
        // nodes are completed — neither the `ready` nor the `all_completed` guard
        // can mask the loop-awareness gap this test pins.
        let mut conditional_edge = edge("a", "b");
        conditional_edge.when = Some(serde_yaml::from_str("iter: { gte: 99 }").unwrap());
        let pipeline = pipeline::PipelineDef {
            name: "cond".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![doc_node_def("a"), doc_node_def("b")],
            edges: vec![conditional_edge],
            loops: Vec::new(),
            prompt_required: false,
        };
        let mut run_state = event_log::RunState::new("20260613-loop".into(), "cond".into());
        // Entry node `a` completed; nothing else live or schedulable.
        run_state.nodes.insert(
            "a".into(),
            event_log::NodeState {
                node_id: "a".into(),
                status: event_log::NodeStatus::Completed,
                iter: 2,
                started_at: None,
                completed_at: None,
                failure_reason: None,
                iterations: Vec::new(),
                frontmatter_retries: 0,
                frontmatter_violations: Vec::new(),
            },
        );
        // An open loop region (exhausted at max_iter, not done) awaiting a route.
        run_state.loop_states.insert(
            "review_loop".into(),
            event_log::LoopState {
                loop_node_id: "review_loop".into(),
                current_iter: 2,
                max_iter: 2,
                break_received: false,
                done: false,
            },
        );

        let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
        let loop_seed = scheduler::seed_pending_loops(&pipeline, &run_state, &HashMap::new());

        assert!(
            run_stall_reason(&pipeline, &run_state, &ready, &loop_seed).is_none(),
            "a run with an open loop region awaiting a manager route must not be \
             auto-failed — that is the manager's recovery path, not a fail-fast stall"
        );
    }

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

    #[test]
    fn cm_sub_worktree_creates_and_merges() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-run";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        assert!(sub_wt_dir.exists());

        // Make a code change in the sub-worktree
        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Verify the file is present in the pipeline worktree
        assert!(wt_dir.join("foo.rs").exists());
    }

    #[test]
    fn cm_sub_worktree_survives_after_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-cm-survive";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();

        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Sub-worktree directory must still exist after merge (refs #32)
        assert!(
            sub_wt_dir.exists(),
            "sub-worktree directory must survive merge for inspection"
        );

        // Sub-worktree branch must still exist after merge
        let branch_check = std::process::Command::new("git")
            .args(["branch", "--list", &sub_branch])
            .current_dir(repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&branch_check.stdout);
        assert!(
            branches.contains(&sub_branch),
            "sub-branch must survive merge; got: {branches}"
        );
    }

    #[test]
    fn cm_merge_conflict_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-conflict";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Create two sub-worktrees that will conflict
        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        // Both modify the same file with different content
        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        // Merge first succeeds
        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        // Merge second → conflict
        let r2 =
            commit_and_merge_sub_worktree(&sub_wt_2, &wt_dir, &sub_branch_2, "impl-2", 1).unwrap();
        assert!(matches!(r2, MergeResult::Conflict(_)));
    }

    #[test]
    fn doc_only_clean_worktree_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-clean";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_dirty_worktree_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-dirty";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Modify a tracked file
        std::fs::write(wt_dir.join("README.md"), "# modified\n").unwrap();

        assert!(worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_untracked_files_not_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-untracked";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Add an untracked file (like artifacts)
        let port_dir = wt_dir.join(".pdo/artifacts/planner/iter-1/plan");
        std::fs::create_dir_all(&port_dir).unwrap();
        std::fs::write(port_dir.join("output.md"), "# plan\n").unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[tokio::test]
    async fn node_done_with_cm_node_def_in_events() {
        let state = test_state().await;
        let run_id = "test-cm-node-done";

        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "cm-test",
                "input": "test",
                "node_defs": [
                    { "id": "impl-1", "node_type": "code-mutating", "inputs": [], "outputs": [] }
                ],
                "edges": []
            })),
        };
        append_event(&state, &run_started).await.unwrap();

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some("impl-1".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "node_type": "code-mutating" })),
        };
        append_event(&state, &node_started).await.unwrap();

        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(find_node_type(&run_state, "impl-1"), Some("code-mutating"));
    }

    async fn test_state_with_dir(dir: &std::path::Path) -> Arc<AppState> {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (event_tx, _) = broadcast::channel(64);
        let (pipeline_tx, _) = broadcast::channel(16);
        Arc::new(AppState {
            db,
            event_tx,
            pipeline_tx,
            repo_root: dir.to_path_buf(),
            port: 0,
            merge_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
            trigger_tick_lock: tokio::sync::Mutex::new(()),
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(Mutex::new(None)),
            // Self-destructing no-op: should any test POST /runs and reach the
            // spawn path, the node session runs `true` and exits immediately —
            // never real claude, never a lingering session (#181).
            tmux_cmd_override: Some("exec true".to_string()),
        })
    }

    const START_END_YAML: &str = "  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

    fn write_test_pipeline(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    /// RAII guard that sets HOME to a temporary directory and restores it on drop.
    /// Holds HOME_TEST_LOCK to prevent concurrent env var mutation.
    struct FakeHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: tempfile::TempDir,
        prev: Option<String>,
    }

    impl FakeHome {
        fn new() -> Self {
            // Poison-tolerant: the lock only serializes HOME mutation; a panic
            // in another test must not cascade here.
            let lock = library_store::HOME_TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let dir = tempfile::tempdir().unwrap();
            let prev = std::env::var("HOME").ok();
            std::env::set_var("HOME", dir.path());
            Self {
                _lock: lock,
                dir,
                prev,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for FakeHome {
        fn drop(&mut self) {
            if let Some(ref h) = self.prev {
                std::env::set_var("HOME", h);
            }
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn list_pipelines_scans_repo_dir() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();

        write_test_pipeline(tmp.path(), "test-pipe");
        write_test_pipeline(tmp.path(), "another-pipe");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|p| p["id"] == "test-pipe"));
        assert!(list.iter().any(|p| p["id"] == "another-pipe"));
        assert!(list.iter().all(|p| p["scope"] == "repo"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn list_pipelines_exposes_prompt_required_flag() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();

        // Default pipeline (prompt_required absent => true).
        write_test_pipeline(tmp.path(), "required-pipe");
        // Prompt-optional pipeline.
        let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
        let yaml = format!(
            "name: optional-pipe\nversion: \"1.0\"\nprompt_required: false\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join("optional-pipe.yaml"), yaml).unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        let required = list.iter().find(|p| p["id"] == "required-pipe").unwrap();
        assert_eq!(required["prompt_required"], serde_json::json!(true));
        let optional = list.iter().find(|p| p["id"] == "optional-pipe").unwrap();
        assert_eq!(optional["prompt_required"], serde_json::json!(false));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn list_pipelines_empty_when_no_dir() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn get_pipeline_returns_parsed_definition() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "my-pipe");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/my-pipe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(detail["id"], "my-pipe");
        assert_eq!(detail["pipeline"]["name"], "my-pipe");
        assert_eq!(detail["pipeline"]["nodes"].as_array().unwrap().len(), 3);
        assert!(detail["pipeline"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "worker"));
        assert!(detail["yaml"].as_str().unwrap().contains("name: my-pipe"));
    }

    #[tokio::test]
    async fn get_pipeline_returns_loop_switch_fixture() {
        let tmp = tempfile::tempdir().unwrap();
        let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let fixture = include_str!("../../../.pdo/pipelines/review-loop.yaml");
        std::fs::write(pipelines_dir.join("review-loop.yaml"), fixture).unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/review-loop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(detail["id"], "review-loop");

        let nodes = detail["pipeline"]["nodes"].as_array().unwrap();
        assert!(nodes.iter().any(|n| n["type"] == "loop"));
        assert!(nodes.iter().any(|n| n["type"] == "switch"));

        let loop_node = nodes.iter().find(|n| n["type"] == "loop").unwrap();
        assert_eq!(loop_node["max_iter"], 3);

        let switch_node = nodes.iter().find(|n| n["type"] == "switch").unwrap();
        let switch_outputs = switch_node["outputs"].as_array().unwrap();
        assert!(switch_outputs
            .iter()
            .any(|o| o["name"] == "pass" && o["when"].is_object()));
        assert!(switch_outputs.iter().any(|o| o["name"] == "default"));
    }

    #[tokio::test]
    async fn get_pipeline_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_pipeline_writes_scaffold() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pipelines")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name": "new-pipeline", "scope": "repo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["id"], "new-pipeline");
        assert_eq!(result["scope"], "repo");

        let yaml_path = tmp
            .path()
            .join(".pdo")
            .join("pipelines")
            .join("new-pipeline.yaml");
        assert!(yaml_path.exists());
        let content = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("name: new-pipeline"));
    }

    #[tokio::test]
    async fn create_pipeline_conflict_on_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "existing");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pipelines")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name": "existing", "scope": "repo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn save_pipeline_updates_yaml_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "editable");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let new_yaml =
            format!("name: editable\nversion: \"2.0\"\nnodes:\n{START_END_YAML}edges: []\n");
        let body = serde_json::json!({
            "yaml": new_yaml,
            "prompts": {}
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/editable")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let yaml_path = tmp
            .path()
            .join(".pdo")
            .join("pipelines")
            .join("editable.yaml");
        let content = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(content.contains("version: \"2.0\""));
    }

    #[tokio::test]
    async fn save_pipeline_rejects_invalid_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "bad-save");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let body = serde_json::json!({
            "yaml": "{{invalid yaml:::",
            "prompts": {}
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/bad-save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn save_pipeline_returns_structured_error_with_line() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "struct-err");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let body = serde_json::json!({
            "yaml": "{{invalid yaml:::",
            "prompts": {}
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/struct-err")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            json.get("message").is_some(),
            "response must have 'message'"
        );
        assert!(
            !json["message"].as_str().unwrap().is_empty(),
            "message must be non-empty"
        );
    }

    #[tokio::test]
    async fn save_pipeline_missing_name_returns_structured_error() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "no-name");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let body = serde_json::json!({
            "yaml": "version: \"1.0\"\nnodes: []\n",
            "prompts": {}
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/no-name")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            json.get("message").is_some(),
            "response must have 'message'"
        );
    }

    #[tokio::test]
    async fn save_pipeline_writes_prompts_under_canonical_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "prompt-save");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let yaml = format!("name: prompt-save\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: ab12cd34\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n");
        let body = serde_json::json!({
            "yaml": yaml,
            "prompts": { "ab12cd34": "You are a worker agent." }
        });

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/prompt-save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let prompt_path = tmp
            .path()
            .join(".pdo/pipelines/prompt-save.prompts/ab12cd34.md");
        assert!(prompt_path.exists(), "canonical prompt file must exist");
        let content = std::fs::read_to_string(&prompt_path).unwrap();
        assert_eq!(content, "You are a worker agent.");
    }

    #[tokio::test]
    async fn get_pipeline_reads_prompts_from_canonical_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "prompt-read");

        let prompts_dir = tmp.path().join(".pdo/pipelines/prompt-read.prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(prompts_dir.join("worker.md"), "Role prompt here.").unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/prompt-read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["prompts"]["worker"], "Role prompt here.");
    }

    #[tokio::test]
    async fn pipeline_list_entry_includes_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "meta-pipe");

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        let entry = &list[0];
        assert_eq!(entry["name"], "meta-pipe");
        assert_eq!(entry["scope"], "repo");
        assert_eq!(entry["node_count"], 3);
        assert!(entry["modified"].as_str().is_some());
    }

    #[tokio::test]
    async fn scan_pipeline_dir_ignores_non_yaml_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".pdo").join("pipelines");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.md"), "not a pipeline").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a pipeline").unwrap();
        write_test_pipeline(tmp.path(), "real-pipe");

        let entries = scan_pipeline_dir(&dir, "repo");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "real-pipe");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn promote_pipeline_copies_to_library() {
        let fake_home = FakeHome::new();

        write_test_pipeline(fake_home.path(), "promotable");
        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pipelines/promotable/promote")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["id"], "promotable");
        assert_eq!(result["drifted"], false);

        let lib_dir = library_store::pipelines::user_pipelines_dir().unwrap();
        assert!(lib_dir.join("promotable.yaml").exists());
        assert!(lib_dir.join("promotable.meta.json").exists());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn promote_nonexistent_pipeline_returns_error() {
        let fake_home = FakeHome::new();

        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pipelines/nonexistent/promote")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn list_pipelines_includes_library_with_drift() {
        let fake_home = FakeHome::new();

        write_test_pipeline(fake_home.path(), "repo-pipe");

        library_store::pipelines::promote(fake_home.path(), "repo-pipe").unwrap();

        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

        let repo_entry = list.iter().find(|p| p["scope"] == "repo").unwrap();
        assert_eq!(repo_entry["id"], "repo-pipe");
        assert!(repo_entry.get("drifted").is_none() || repo_entry["drifted"].is_null());

        let lib_entry = list.iter().find(|p| p["scope"] == "library").unwrap();
        assert_eq!(lib_entry["id"], "repo-pipe");
        assert_eq!(lib_entry["drifted"], false);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn list_pipelines_library_shows_drift_after_source_change() {
        let fake_home = FakeHome::new();

        write_test_pipeline(fake_home.path(), "drifting");
        library_store::pipelines::promote(fake_home.path(), "drifting").unwrap();

        let changed_yaml =
            format!("name: drifting-modified\nversion: \"2.0\"\nnodes:\n{START_END_YAML}");
        std::fs::write(
            fake_home.path().join(".pdo/pipelines/drifting.yaml"),
            changed_yaml,
        )
        .unwrap();

        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        let lib_entry = list.iter().find(|p| p["scope"] == "library").unwrap();
        assert_eq!(lib_entry["drifted"], true);
    }

    #[tokio::test]
    async fn delete_pipeline_removes_yaml_and_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "doomed");

        // Also create a prompts directory
        let prompts_dir = tmp.path().join(".pdo/pipelines/doomed.prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(prompts_dir.join("worker.md"), "role prompt").unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/doomed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let yaml_path = tmp.path().join(".pdo/pipelines/doomed.yaml");
        assert!(!yaml_path.exists(), "YAML file should be deleted");
        assert!(!prompts_dir.exists(), "prompts dir should be deleted");
    }

    #[tokio::test]
    async fn delete_pipeline_returns_404_when_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // #216 — the core data-loss regression. A `scope=library` delete of an id
    // that ALSO exists as a repo pipeline (the normal outcome of "promote to
    // library") must remove only the library copy and never the repo YAML or
    // its `.prompts/` sidecar.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn delete_pipeline_scope_library_spares_repo_file() {
        let fake_home = FakeHome::new();
        write_test_pipeline(fake_home.path(), "simple-bugfix");
        // A sidecar prompts dir proves the `remove_dir_all` never reaches the repo.
        let repo_prompts = fake_home
            .path()
            .join(".pdo/pipelines/simple-bugfix.prompts");
        std::fs::create_dir_all(&repo_prompts).unwrap();
        std::fs::write(repo_prompts.join("worker.md"), "repo prompt").unwrap();

        library_store::pipelines::promote(fake_home.path(), "simple-bugfix").unwrap();
        let lib_yaml = library_store::pipelines::user_pipelines_dir()
            .unwrap()
            .join("simple-bugfix.yaml");
        assert!(lib_yaml.exists(), "library copy should exist after promote");

        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/simple-bugfix?scope=library")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        // Library copy is gone — the intended target.
        assert!(!lib_yaml.exists(), "library copy should be deleted");
        // Repo YAML + sidecar are untouched — the file the user never targeted.
        assert!(
            fake_home
                .path()
                .join(".pdo/pipelines/simple-bugfix.yaml")
                .exists(),
            "repo YAML must survive a library-scoped delete"
        );
        assert!(
            repo_prompts.join("worker.md").exists(),
            "repo .prompts/ sidecar must survive a library-scoped delete"
        );
    }

    // #216, fix direction 3 — a promoted library entry stays openable from its
    // own stored YAML even when the source repo file is gone. The bare-id GET
    // (no scope) 404s in that state; the scoped GET resolves the library copy.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn get_pipeline_scope_library_reads_own_yaml_when_repo_absent() {
        let fake_home = FakeHome::new();
        write_test_pipeline(fake_home.path(), "promoted");
        library_store::pipelines::promote(fake_home.path(), "promoted").unwrap();
        // Source repo pipeline disappears (e.g. a prior buggy delete).
        std::fs::remove_file(fake_home.path().join(".pdo/pipelines/promoted.yaml")).unwrap();

        let state = test_state_with_dir(fake_home.path()).await;

        // Bare id no longer resolves — this is the "unclickable" symptom.
        let app = build_router(Arc::clone(&state));
        let bare = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/promoted")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bare.status(), StatusCode::NOT_FOUND);

        // Scoped open reads the library entry's own YAML.
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/promoted?scope=library")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["scope"], "library");
        assert_eq!(val["id"], "promoted");
        assert!(val["yaml"].as_str().unwrap().contains("name: promoted"));
    }

    // #216 — a `scope=library` save writes back into the library store, never
    // the same-named repo file.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn save_pipeline_scope_library_writes_library_not_repo() {
        let fake_home = FakeHome::new();
        write_test_pipeline(fake_home.path(), "shared");
        library_store::pipelines::promote(fake_home.path(), "shared").unwrap();

        let repo_yaml_path = fake_home.path().join(".pdo/pipelines/shared.yaml");
        let repo_before = std::fs::read_to_string(&repo_yaml_path).unwrap();

        let state = test_state_with_dir(fake_home.path()).await;
        let app = build_router(state);

        let edited = format!("name: shared\nversion: \"9.9\"\nnodes:\n{START_END_YAML}");
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pipelines/shared?scope=library")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "yaml": edited, "prompts": {} }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Library copy reflects the edit; repo copy is byte-for-byte unchanged.
        let lib_yaml = library_store::pipelines::get_yaml(fake_home.path(), "shared").unwrap();
        assert!(lib_yaml.contains("9.9"));
        assert_eq!(
            std::fs::read_to_string(&repo_yaml_path).unwrap(),
            repo_before
        );
    }

    // #216 (defense in depth) — an explicit `scope=user` delete resolves
    // strictly to the user store and never falls through to a same-named repo
    // pipeline (repo_root and HOME are kept distinct here so the two stores have
    // genuinely separate paths).
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn delete_pipeline_scope_user_does_not_touch_repo() {
        let fake_home = FakeHome::new();
        let repo = tempfile::tempdir().unwrap();
        write_test_pipeline(repo.path(), "foo");

        let user_dir = fake_home.path().join(".pdo/pipelines");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join("foo.yaml"),
            format!("name: foo\nversion: \"1.0\"\nnodes:\n{START_END_YAML}"),
        )
        .unwrap();

        let state = test_state_with_dir(repo.path()).await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/foo?scope=user")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        assert!(!user_dir.join("foo.yaml").exists(), "user copy deleted");
        assert!(
            repo.path().join(".pdo/pipelines/foo.yaml").exists(),
            "repo copy must survive a user-scoped delete"
        );
    }

    #[tokio::test]
    async fn delete_pipeline_returns_409_when_active_runs_exist() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "busy-pipe");

        let state = test_state_with_dir(tmp.path()).await;

        // Insert a run_started event referencing this pipeline
        let run_started = event_log::Event {
            id: None,
            run_id: "run-001".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "busy-pipe" })),
        };
        append_event(&state, &run_started).await.unwrap();

        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/busy-pipe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(val["error"].as_str().unwrap().contains("active run"));
    }

    #[tokio::test]
    async fn delete_pipeline_succeeds_when_runs_completed() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "done-pipe");

        let state = test_state_with_dir(tmp.path()).await;

        // Insert run_started + run_completed events
        let run_started = event_log::Event {
            id: None,
            run_id: "run-done".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "done-pipe" })),
        };
        append_event(&state, &run_started).await.unwrap();

        let run_completed = event_log::Event {
            id: None,
            run_id: "run-done".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        };
        append_event(&state, &run_completed).await.unwrap();

        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/done-pipe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let yaml_path = tmp.path().join(".pdo/pipelines/done-pipe.yaml");
        assert!(
            !yaml_path.exists(),
            "YAML file should be deleted after completed run"
        );
    }

    #[tokio::test]
    async fn delete_pipeline_then_get_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "vanish");

        let state = test_state_with_dir(tmp.path()).await;

        // Delete
        let app = build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/pipelines/vanish")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // GET should now 404
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/pipelines/vanish")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
            Commands::Daemon { port } => assert_eq!(port, expected),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_port() {
        let cli = Cli::try_parse_from(["pdo", "daemon", "--port", "9999"]).unwrap();
        match cli.command {
            Commands::Daemon { port } => assert_eq!(port, 9999),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_complete_subcommand() {
        let cli = Cli::try_parse_from(["pdo", "complete"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete));
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
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["pdo", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    // --- Layer 2: pane endpoint contract tests ---

    async fn seed_running_run(state: &Arc<AppState>, run_id: &str, node_id: &str) {
        let events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
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

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/nonexistent/nodes/worker/pane?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn pane_returns_404_for_nonexistent_node() {
        let state = test_state().await;
        seed_running_run(&state, "pane-404-node", "worker").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/pane-404-node/nodes/bogus/pane?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn pane_returns_200_with_placeholder_when_no_tmux() {
        let state = test_state().await;
        seed_running_run(&state, "pane-no-tmux", "worker").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/pane-no-tmux/nodes/worker/pane?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["content"].is_string());
        assert!(json["session_name"].is_string());
        assert!(json["resumed"].is_boolean());
    }

    #[tokio::test]
    async fn pane_defaults_iter_to_1() {
        let state = test_state().await;
        seed_running_run(&state, "pane-default-iter", "worker").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/pane-default-iter/nodes/worker/pane")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["session_name"].as_str().unwrap().contains("iter-1"));
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
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
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
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/reviewer/pane?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["content"], "Session no longer available");
        assert_eq!(json["resumed"], false);
        assert_eq!(
            json["stale"], true,
            "non-latest iter should be marked stale"
        );
    }

    #[tokio::test]
    async fn pane_latest_iter_is_not_stale() {
        let state = test_state().await;
        seed_running_run(&state, "pane-latest-stale", "worker").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/pane-latest-stale/nodes/worker/pane?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["stale"], false, "latest iter should not be stale");
    }

    // -----------------------------------------------------------------------
    // node_prompt endpoint — Layer 2 contract tests
    // -----------------------------------------------------------------------

    async fn seed_run_with_node(
        state: &Arc<AppState>,
        run_id: &str,
        node_id: &str,
        node_type: &str,
    ) {
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "test",
                "node_defs": [
                    { "id": node_id, "node_type": node_type, "inputs": [], "outputs": [] }
                ],
                "edges": []
            })),
        };
        append_event(state, &run_started).await.unwrap();

        let node_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node_id.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "node_type": node_type })),
        };
        append_event(state, &node_started).await.unwrap();
    }

    #[tokio::test]
    async fn node_prompt_returns_404_for_nonexistent_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/no-such-run/nodes/worker/prompt?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_prompt_returns_404_for_nonexistent_node() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "prompt-test-no-node";
        seed_run_with_node(&state, run_id, "worker", "doc-only").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/nonexistent/prompt?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_prompt_returns_404_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "prompt-test-no-file";
        seed_run_with_node(&state, run_id, "worker", "doc-only").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/worker/prompt?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_prompt_returns_markdown_when_file_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "prompt-test-ok";
        let wt_dir = tmp.path().join(".pdo/runs").join(run_id).join("worktree");
        let prompt_dir = wt_dir.join(".pdo").join("prompts");
        std::fs::create_dir_all(&prompt_dir).unwrap();
        std::fs::write(
            prompt_dir.join("worker-iter-1.md"),
            "## Inputs\n\n- task: /path/to/task.md\n\n## Outputs\n\n- result\n",
        )
        .unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        seed_run_with_node(&state, run_id, "worker", "doc-only").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/worker/prompt?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/markdown"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("## Inputs"));
        assert!(text.contains("## Outputs"));
    }

    #[tokio::test]
    async fn node_prompt_defaults_iter_to_1() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "prompt-test-default-iter";
        let wt_dir = tmp.path().join(".pdo/runs").join(run_id).join("worktree");
        let prompt_dir = wt_dir.join(".pdo").join("prompts");
        std::fs::create_dir_all(&prompt_dir).unwrap();
        std::fs::write(prompt_dir.join("worker-iter-1.md"), "prompt content").unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        seed_run_with_node(&state, run_id, "worker", "doc-only").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/worker/prompt"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn prompt_endpoint_returns_200_for_completed_cm_node_iter() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "prompt-cm-survive";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;

        // Create real worktrees
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        // Write prompt file in the sub-worktree (as the daemon does at spawn)
        let prompt_dir = sub_wt_dir.join(".pdo").join("prompts");
        std::fs::create_dir_all(&prompt_dir).unwrap();
        std::fs::write(
            prompt_dir.join("impl-1-iter-1.md"),
            "## Inputs\n\n## Outputs\n\nYou are an implementer.\n",
        )
        .unwrap();

        // Merge sub-worktree (simulating node completion)
        std::fs::write(sub_wt_dir.join("foo.rs"), "fn main() {}\n").unwrap();
        let result =
            commit_and_merge_sub_worktree(&sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1).unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Prompt endpoint must return 200 for the completed iter
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/impl-1/prompt?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "prompt endpoint must return 200 for completed code-mutating node iter"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!text.is_empty(), "prompt body must be non-empty");
        assert!(text.contains("## Inputs"));
    }

    // ---- Layer 2 contract tests: /io endpoint ----

    fn seed_io_test(dir: &std::path::Path, run_id: &str) {
        let run_dir = dir.join(".pdo/runs").join(run_id);
        let pipeline_path = run_dir.join("pipeline.yaml");
        std::fs::create_dir_all(run_dir.join("worktree/.pdo/artifacts/planner/iter-1/plan"))
            .unwrap();
        std::fs::create_dir_all(run_dir.join("worktree/.pdo/artifacts/implementer/iter-1/summary"))
            .unwrap();
        std::fs::create_dir_all(pipeline_path.parent().unwrap()).unwrap();
        std::fs::write(
            &pipeline_path,
            format!(
                "name: io-test-pipe\nnodes:\n{START_END_YAML}  - id: planner\n    name: planner\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: plan\n  - id: implementer\n    name: implementer\n    type: code-mutating\n    inputs:\n      - name: plan\n    outputs:\n      - name: summary\nedges:\n  - source: {{ node: planner, port: plan }}\n    target: {{ node: implementer, port: plan }}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            run_dir.join("worktree/.pdo/artifacts/planner/iter-1/plan/output.md"),
            "# Plan\nDo stuff.",
        )
        .unwrap();
        std::fs::write(
            run_dir.join("worktree/.pdo/artifacts/implementer/iter-1/summary/output.md"),
            "---\nverdict: PASS\nscore: 9\n---\n\n## Summary\nAll good.",
        )
        .unwrap();
    }

    async fn seed_io_run_events(state: &Arc<AppState>, run_id: &str) {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "io-test-pipe",
                    "input": "test",
                    "node_defs": [
                        { "id": "planner", "node_type": "doc-only", "inputs": ["task"], "outputs": ["plan"] },
                        { "id": "implementer", "node_type": "code-mutating", "inputs": ["plan"], "outputs": ["summary"] }
                    ],
                    "edges": [
                        { "source_node": "planner", "source_port": "plan", "target_node": "implementer", "target_port": "plan" }
                    ]
                })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("planner".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("planner".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("implementer".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(state, &event).await.unwrap();
        }
    }

    #[tokio::test]
    async fn node_io_returns_404_for_missing_run() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/nope/nodes/worker/io?iter=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_io_returns_404_for_missing_node() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "io-missing-node";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/ghost/io?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_io_returns_correct_payload_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "io-shape-test";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/implementer/io?iter=1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let io: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // inputs array
        let inputs = io["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0]["port"], "plan");
        assert_eq!(inputs[0]["repeated"], false);
        assert!(!inputs[0]["files"].as_array().unwrap().is_empty());
        assert_eq!(inputs[0]["files"][0]["exists"], true);

        // outputs array
        let outputs = io["outputs"].as_array().unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["port"], "summary");
        assert_eq!(outputs[0]["files"][0]["exists"], true);
        // frontmatter parsed
        let fm = &outputs[0]["files"][0]["frontmatter"];
        assert_eq!(fm["verdict"], "PASS");
        assert_eq!(fm["score"], 9);
    }

    #[tokio::test]
    async fn node_io_defaults_iter_to_1() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "io-default-iter";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/implementer/io"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ---- Layer 2 contract tests: /artifact endpoint ----

    #[tokio::test]
    async fn artifact_returns_404_for_missing_run() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/nope/artifact?path=planner/iter-1/plan.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn artifact_returns_content_with_text_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "artifact-content-test";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/artifact?path=planner/iter-1/plan/output.md"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-type").unwrap(), "text/markdown");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("# Plan"));
    }

    #[tokio::test]
    async fn artifact_returns_400_on_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "artifact-traversal";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/artifact?path=../../../../../../etc/passwd"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should be 400 (traversal) or 404 (file not found after canonicalize fails)
        assert!(
            resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
            "expected 400 or 404 for traversal, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn artifact_returns_404_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "artifact-missing-file";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/artifact?path=planner/iter-1/nonexistent.md"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // --- Layer-2: output validation via mark_node_done (refs #36) ---

    fn write_pipeline_with_outputs(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: summary\n      - name: report\n    view: {{ x: 100, y: 100 }}\nedges: []\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    async fn seed_awaiting_run(state: &Arc<AppState>, run_id: &str, pipeline_name: &str) {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeAwaitingUser,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(state, &event).await.unwrap();
        }
    }

    async fn seed_failed_run(state: &Arc<AppState>, run_id: &str, pipeline_name: &str) {
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipeline_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeFailed,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({ "reason": "tool call exited 1" })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": "tool call exited 1" })),
            },
        ] {
            append_event(state, &event).await.unwrap();
        }
    }

    #[tokio::test]
    async fn mark_node_done_returns_409_when_outputs_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "validate-test";
        write_pipeline_with_outputs(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "validate-409";
        seed_awaiting_run(&state, run_id, pipe_name).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
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
        assert_eq!(body["error"], "missing_outputs");
        let missing = body["missing"].as_array().unwrap();
        assert!(missing.iter().any(|v| v == "summary"));
        assert!(missing.iter().any(|v| v == "report"));
    }

    #[tokio::test]
    async fn mark_node_done_accepts_failed_node_with_outputs_after_resume() {
        // #212: a Failed run accepts no lifecycle event — the recovery flow is
        // resume_run first, then mark_node_done on the (hand-fixed) failed iter.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "failed-rescue";
        write_pipeline_with_outputs(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "failed-rescue-1";
        seed_failed_run(&state, run_id, pipe_name).await;

        // Create the required output files (directory-based, ADR-0010)
        let base = tmp
            .path()
            .join(".pdo/runs")
            .join(run_id)
            .join("worktree/.pdo/artifacts/worker/iter-1");
        let summary_dir = base.join("summary");
        std::fs::create_dir_all(&summary_dir).unwrap();
        std::fs::write(summary_dir.join("output.md"), "# Summary\nDone.").unwrap();
        let report_dir = base.join("report");
        std::fs::create_dir_all(&report_dir).unwrap();
        std::fs::write(report_dir.join("output.md"), "# Report\nAll good.").unwrap();

        // While the run is Failed, mark_node_done is rejected with a readable
        // cause (#197 family: no lifecycle event on a non-running run).
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
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
        assert!(
            body["error"].as_str().unwrap().contains("resume"),
            "rejection should point at resume_run, got {body}"
        );

        // resume_run lifts the failure...
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "resume_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // ...then the failed iteration is markable.
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Completed
        );
    }

    // --- Transition guard wiring (#212, closes #195 #196 #197 #198 #201) ---

    /// Pipeline `worker -> consumer` (both doc-only, no declared outputs) so
    /// downstream re-spawn behavior is observable in the event log.
    fn write_pipeline_with_consumer(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n  - id: consumer\n    name: consumer\n    type: doc-only\n    inputs:\n      - name: feed\n    outputs:\n      - name: out\nedges:\n  - source: {{ node: worker, port: result }}\n    target: {{ node: consumer, port: feed }}\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    fn count_events(
        events: &[event_log::Event],
        kind: event_log::EventKind,
        node_id: &str,
    ) -> usize {
        events
            .iter()
            .filter(|e| e.kind == kind && e.node_id.as_deref() == Some(node_id))
            .count()
    }

    /// #215: a terminal run (here `Failed` via fail-fast) that still projects a
    /// session-holding node must be reconciled at boot — the dangling node is
    /// marked `Failed`, the projection becomes consistent, and the pass is
    /// idempotent across restarts.
    #[tokio::test]
    async fn boot_recovery_reconciles_session_holding_node_in_terminal_run() {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "boot-rec-215";
        write_test_pipeline(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "boot-rec-215-1";
        // RunStarted -> NodeStarted(worker, iter 1) -> RunFailed: a terminal run
        // whose worker node is still projected Running. Run* events bypass the
        // transition guard, so this faithfully reproduces fail-fast leaving a
        // sibling session-holding.
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": "fail-fast: sibling failed" })),
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        // Before recovery: the projection still shows worker Running, but the
        // Phase-1 count already excludes it (run is terminal) — the slot no
        // longer leaks.
        let before = event_log::project(&load_events(&state.db, run_id).await.unwrap()).unwrap();
        assert_eq!(before.status, event_log::RunStatus::Failed);
        assert_eq!(
            before.nodes["worker"].status,
            event_log::NodeStatus::Running,
            "precondition: worker still projects Running before recovery"
        );
        assert_eq!(
            admission::count_live_node_sessions([&before]),
            0,
            "Phase 1 already excludes terminal-run nodes from the cap"
        );

        run_boot_recovery(&state).await;

        // After recovery: worker is reconciled to Failed with an explanatory
        // reason, and no terminal run carries a session-holding node.
        let after = event_log::project(&load_events(&state.db, run_id).await.unwrap()).unwrap();
        assert_eq!(after.nodes["worker"].status, event_log::NodeStatus::Failed);
        let reason = after.nodes["worker"]
            .failure_reason
            .as_deref()
            .unwrap_or_default();
        assert!(
            reason.contains("terminal") && reason.contains("session-holding"),
            "reason should explain the terminal/session-holding reconciliation, got: {reason}"
        );
        assert!(
            !after.status.is_live()
                && !after.nodes.values().any(|ns| {
                    matches!(
                        ns.status,
                        event_log::NodeStatus::Running | event_log::NodeStatus::AwaitingUser
                    )
                }),
            "no terminal run may carry a Running/AwaitingUser node after recovery"
        );

        // Idempotency: a second pass (e.g. another reboot) appends no duplicate
        // NodeFailed — validate_fail returns NoOp on the already-terminal iter.
        run_boot_recovery(&state).await;
        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeFailed, "worker"),
            1,
            "boot recovery must be idempotent: exactly one NodeFailed for worker"
        );
        let again = event_log::project(&events).unwrap();
        assert_eq!(again.nodes["worker"].status, event_log::NodeStatus::Failed);
    }

    #[tokio::test]
    async fn mark_node_done_on_completed_iter_is_noop_without_downstream_spawn() {
        // #198: the duplicate completion must neither re-emit node_completed
        // nor re-trigger downstream spawns.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-dup-mark";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "guard-dup-mark-1";
        // worker completed organically; consumer already running iter 1.
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("consumer".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
                    ))
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
        assert_eq!(body["noop"], true, "duplicate completion must be a no-op");

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
            1,
            "no duplicate node_completed"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "consumer"),
            1,
            "no downstream re-spawn"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeWaiting, "consumer"),
            0
        );
    }

    #[tokio::test]
    async fn mark_node_done_on_never_started_iter_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-ghost-mark";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "guard-ghost-mark-1";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
        };
        append_event(&state, &run_started).await.unwrap();

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "mark_node_done", "node_id": "worker", "iter": 1}"#,
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
        assert!(
            body["error"].as_str().unwrap().contains("never started"),
            "got {body}"
        );

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
            0,
            "rejection emits no completion"
        );
    }

    /// Seed a `blocker` run holding enough Running nodes to saturate the
    /// global session cap, so any legitimate spawn in the run under test lands
    /// as an observable `node_waiting` event instead of a real tmux session.
    async fn saturate_session_cap(state: &Arc<AppState>) {
        let run_id = "cap-blocker";
        let run_started = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
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
                ts: event_log::now_iso(),
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
            ts: event_log::now_iso(),
            kind,
            node_id: node_id.map(String::from),
            iter,
            payload,
        }
    }

    #[tokio::test]
    async fn stale_detector_terminal_events_on_terminal_node_are_dropped() {
        // #212: the stale detector probes a snapshot; if the node completed
        // organically in between, its NodeStale / NodeAutoCompleted must be
        // dropped by the guard at append time (re-checked against the freshly
        // projected state).
        let state = test_state().await;
        let run_id = "guard-stale-terminal";
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
                event_log::EventKind::NodeCompleted,
                Some("worker"),
                Some(1),
                None,
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        for detection in [
            stale_detector::Detection::Stale,
            stale_detector::Detection::AutoComplete,
        ] {
            for event in stale_detector::detection_events(&detection, run_id, "worker", 1) {
                // The guard turns these into no-ops, not errors.
                append_event(&state, &event).await.unwrap();
            }
        }

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStale, "worker"),
            0
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
            1,
            "no duplicate completion from the auto-complete path"
        );
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Completed
        );
    }

    #[tokio::test]
    async fn restart_node_rejected_while_newer_iteration_is_live() {
        // #196: restart_node on a stale iter must not race the scheduler's
        // newer live iteration — reject with a readable cause, spawn nothing.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-restart-race";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "guard-restart-race-1";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": pipe_name })),
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
                event_log::EventKind::NodeCompleted,
                Some("worker"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("worker"),
                Some(2),
                None,
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "restart_node", "node_id": "worker", "iter": 1}"#,
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
        assert!(
            body["error"].as_str().unwrap().contains("live"),
            "got {body}"
        );

        let events = load_events(&state.db, run_id).await.unwrap();
        // iter 2 stays the only live iteration; iter 1 was not re-spawned.
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "worker"),
            2,
            "no extra node_started"
        );
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Running
        );
        assert_eq!(run_state.nodes["worker"].iter, 2);
    }

    #[tokio::test]
    async fn resume_run_schedules_only_missing_work() {
        // #195: chain a -> b -> c with a, b completed and c never started.
        // resume_run schedules c only; a and b are not re-iterated.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-resume-missing";
        let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {pipe_name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: a\n    name: a\n    type: doc-only\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: b\n    name: b\n    type: doc-only\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: c\n    name: c\n    type: doc-only\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\nedges:\n  - source: {{ node: a, port: out }}\n    target: {{ node: b, port: in }}\n  - source: {{ node: b, port: out }}\n    target: {{ node: c, port: in }}\n"
        );
        std::fs::write(pipelines_dir.join(format!("{pipe_name}.yaml")), yaml).unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        saturate_session_cap(&state).await;
        let run_id = "guard-resume-missing-1";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": pipe_name })),
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("a"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeCompleted,
                Some("a"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("b"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeCompleted,
                Some("b"),
                Some(1),
                None,
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "resume_run"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        // c is scheduled (waiting, because the cap is saturated)...
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeWaiting, "c")
                + count_events(&events, event_log::EventKind::NodeStarted, "c"),
            1,
            "the never-started node is scheduled exactly once"
        );
        // ...and the completed nodes are not re-iterated.
        for nid in ["a", "b"] {
            assert_eq!(
                count_events(&events, event_log::EventKind::NodeStarted, nid),
                1,
                "completed node {nid} must not be re-spawned"
            );
            assert_eq!(
                count_events(&events, event_log::EventKind::NodeWaiting, nid),
                0,
                "completed node {nid} must not be re-scheduled"
            );
        }
    }

    #[tokio::test]
    async fn resume_run_refuses_concurrent_iteration_and_is_idempotent() {
        // #201: a -> b with a back-edge b -> a (emergent cycle). a completed
        // iter 1, b still running iter 1: resume_run must NOT spawn b iter 2,
        // and a second resume_run must be a no-op too.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-resume-live";
        let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {pipe_name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: a\n    name: a\n    type: doc-only\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\n  - id: b\n    name: b\n    type: doc-only\n    inputs: [{{ name: in }}]\n    outputs: [{{ name: out }}]\nedges:\n  - source: {{ node: a, port: out }}\n    target: {{ node: b, port: in }}\n  - source: {{ node: b, port: out }}\n    target: {{ node: a, port: in }}\n    when: \"iter < 3\"\n"
        );
        std::fs::write(pipelines_dir.join(format!("{pipe_name}.yaml")), yaml).unwrap();

        let state = test_state_with_dir(tmp.path()).await;
        saturate_session_cap(&state).await;
        let run_id = "guard-resume-live-1";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": pipe_name })),
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("a"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeCompleted,
                Some("a"),
                Some(1),
                None,
            ),
            seed_event(
                run_id,
                event_log::EventKind::NodeStarted,
                Some("b"),
                Some(1),
                None,
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        for _ in 0..2 {
            let resp = build_router(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/runs/{run_id}/commands"))
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"kind": "resume_run"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);

            let events = load_events(&state.db, run_id).await.unwrap();
            assert_eq!(
                count_events(&events, event_log::EventKind::NodeStarted, "b"),
                1,
                "no concurrent second iteration of b"
            );
            assert_eq!(
                count_events(&events, event_log::EventKind::NodeWaiting, "b"),
                0,
                "no scheduled second iteration of b"
            );
            assert_eq!(
                count_events(&events, event_log::EventKind::NodeStarted, "a"),
                1,
                "completed a is not redone"
            );
        }
    }

    #[tokio::test]
    async fn failed_run_accepts_no_completion_and_schedules_nothing() {
        // #197: once the run is failed, node_done is rejected and the
        // scheduler does not advance until resume_run.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-failed-sched";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "guard-failed-sched-1";
        for event in [
            seed_event(
                run_id,
                event_log::EventKind::RunStarted,
                None,
                None,
                Some(serde_json::json!({ "pipeline_name": pipe_name })),
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
                event_log::EventKind::RunFailed,
                None,
                None,
                Some(serde_json::json!({ "reason": "boom" })),
            ),
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/done"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"iter": 1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
            0
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "consumer")
                + count_events(&events, event_log::EventKind::NodeWaiting, "consumer"),
            0,
            "a failed run schedules nothing"
        );
    }

    #[tokio::test]
    async fn double_node_done_is_noop_without_downstream_respawn() {
        // #198: replaying node_done for an already-completed (node, iter) must
        // not bump the consumer to a fresh iteration.
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "guard-dup-done";
        write_pipeline_with_consumer(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "guard-dup-done-1";
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("consumer".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/done"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"iter": 1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeCompleted, "worker"),
            1,
            "no duplicate node_completed"
        );
        assert_eq!(
            count_events(&events, event_log::EventKind::NodeStarted, "consumer"),
            1,
            "no downstream re-spawn"
        );
    }

    #[tokio::test]
    async fn node_done_returns_409_when_outputs_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "node-done-validate";
        write_pipeline_with_outputs(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "node-done-409";

        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": pipe_name })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: event_log::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            },
        ] {
            append_event(&state, &event).await.unwrap();
        }

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/done"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
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
        assert_eq!(body["error"], "missing_outputs");
        let missing = body["missing"].as_array().unwrap();
        assert!(missing.iter().any(|v| v == "summary"));
        assert!(missing.iter().any(|v| v == "report"));
    }

    // ---- Layer 2 contract tests: command endpoints (issue #10) ----

    #[tokio::test]
    async fn extend_cycle_requires_node_id() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "extend_cycle", "additional_iter": 3 })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn extend_cycle_requires_positive_additional_iter() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "extend_cycle",
                            "node_id": "reviewer",
                            "additional_iter": 0
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn end_region_requires_region_id() {
        // The manager routes a loop region BY ID (ADR-0011 / #152): without a
        // region_id the command is a bad request.
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "end_region" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn end_region_emits_command_issued_with_region_id() {
        // Ending a region by id appends a `command_issued` control-flow event
        // carrying the region id, so the projection can fold it (#152).
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "region-end".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/region-end/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "end_region", "region_id": "review_loop" })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "region-end").await.unwrap();
        let routes = event_log::collect_region_routes(&events);
        let route = routes
            .get("review_loop")
            .expect("end_region routed review_loop");
        assert!(route.ended, "the region is folded as ended");
    }

    #[tokio::test]
    async fn bump_region_requires_positive_additional_iter() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "bump_region",
                            "region_id": "review_loop",
                            "additional_iter": 0
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bump_region_emits_command_issued_with_additional_iter() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "region-bump".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/region-bump/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "bump_region",
                            "region_id": "review_loop",
                            "additional_iter": 2
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "region-bump").await.unwrap();
        let routes = event_log::collect_region_routes(&events);
        let route = routes
            .get("review_loop")
            .expect("bump_region routed review_loop");
        assert_eq!(route.bumped_by, 2, "the region is folded with +2 laps");
        assert!(!route.ended);
    }

    #[tokio::test]
    async fn end_region_resumes_an_exhausted_unrouted_halt() {
        // The acceptance behavior (#152): a run blocked "exhausted — unrouted"
        // (a RunHalted) is continued by routing the region from the manager —
        // `resume_run` lifts the halt so the run is Running again, without a
        // daemon restart.
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "region-unstick".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "loop-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let halt_event = event_log::Event {
            id: None,
            run_id: "region-unstick".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunHalted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "message": "exhausted — unrouted" })),
        };
        append_event(&state, &halt_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/region-unstick/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "end_region", "region_id": "review_loop" })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "region-unstick").await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.status,
            event_log::RunStatus::Running,
            "ending the region resumes the halted run"
        );
    }

    #[tokio::test]
    async fn kill_node_requires_node_id() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "kill_node" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn restart_node_requires_node_id() {
        let state = test_state().await;
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "restart_node" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn inject_artifact_requires_path_and_content() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "inject_artifact" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let app2 = build_router(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/test-run/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "inject_artifact",
                            "path": "some/path.md"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn inject_artifact_rejects_path_traversal() {
        let state = test_state().await;

        for bad_path in [
            "../../etc/passwd",
            "/absolute/path.md",
            "ok/../../../escape",
        ] {
            let app = build_router(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/runs/test-run/commands")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "kind": "inject_artifact",
                                "path": bad_path,
                                "content": "malicious"
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "path {bad_path:?} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn inject_artifact_writes_file_and_appends_event() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "inject-test";
        // Create run dir structure
        let artifacts_dir = tmp
            .path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree")
            .join(".pdo")
            .join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let state = test_state_with_dir(tmp.path()).await;

        // Seed a run_started event
        let run_event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "inject-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "inject_artifact",
                            "path": "manual/iter-1/notes.md",
                            "content": "# Injected\n\nHello from the manager."
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify file was written
        let file_path = artifacts_dir.join("manual/iter-1/notes.md");
        assert!(file_path.exists());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Injected"));

        // Verify event was appended
        let events = load_events(&state.db, run_id).await.unwrap();
        let cmd_events: Vec<_> = events
            .iter()
            .filter(|e| e.kind == event_log::EventKind::CommandIssued)
            .collect();
        assert_eq!(cmd_events.len(), 1);
        let payload = cmd_events[0].payload.as_ref().unwrap();
        assert_eq!(payload["command"], "inject_artifact");
        assert_eq!(payload["path"], "manual/iter-1/notes.md");
    }

    #[tokio::test]
    async fn resume_run_appends_command_issued_event() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "resume-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "resume-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let halt_event = event_log::Event {
            id: None,
            run_id: "resume-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunHalted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "message": "halted" })),
        };
        append_event(&state, &halt_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/resume-test/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "resume_run" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify event was appended and run status changed
        let events = load_events(&state.db, "resume-test").await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Running);
    }

    // --- extract_variable_refs_from_outgoing_edges unit test ---

    #[test]
    fn extract_var_refs_finds_dollar_variables_in_switch_outputs() {
        use crate::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "sw1".into(),
                name: "switch".into(),
                node_type: NodeType::Switch,
                inputs: vec![Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![Port {
                    name: "pass".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: Some(serde_yaml::from_str("iter: { lt: \"$max_iter_review\" }").unwrap()),
                    description: None,
                }],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
            }],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: EdgeEndpoint {
                    node: "sw1".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            prompt_required: true,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "reviewer");
        assert_eq!(refs, vec!["max_iter_review"]);
    }

    #[test]
    fn extract_var_refs_empty_for_no_vars() {
        use crate::pipeline::*;

        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![NodeDef {
                id: "b".into(),
                name: "b".into(),
                node_type: NodeType::DocOnly,
                inputs: vec![Port {
                    name: "in".into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                }],
                outputs: vec![],
                interactive: false,
                view: None,
                max_iter: None,
                over: None,
            }],
            edges: vec![EdgeDef {
                source: EdgeEndpoint {
                    node: "a".into(),
                    port: "out".into(),
                },
                target: EdgeEndpoint {
                    node: "b".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            prompt_required: true,
        };

        let refs = extract_variable_refs_from_outgoing_edges(&pipeline, "a");
        assert!(refs.is_empty());
    }

    // --- Merge resolver tests (issue #8) ---

    #[test]
    fn validate_merge_resolution_clean_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.is_empty(),
            "clean repo should pass validation, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_conflict_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(
            repo.join("conflict.txt"),
            "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nafter\n",
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "conflict.txt"])
            .current_dir(repo)
            .output();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("conflict markers")),
            "should detect conflict markers, got: {problems:?}"
        );
    }

    #[test]
    fn validate_merge_resolution_detects_uncommitted_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        std::fs::write(repo.join("README.md"), "# modified\n").unwrap();

        let problems = validate_merge_resolution(repo).unwrap();
        assert!(
            problems.iter().any(|p| p.contains("not clean")),
            "should detect dirty worktree, got: {problems:?}"
        );
    }

    #[test]
    fn builtin_merge_resolver_prompt_loads_from_file() {
        let prompt = load_merge_resolver_prompt(std::path::Path::new("."));
        assert!(
            prompt.contains("Merge Resolver"),
            "prompt should contain 'Merge Resolver', got first 100 chars: {}",
            &prompt[..prompt.len().min(100)]
        );
    }

    #[test]
    fn builtin_merge_resolver_prompt_falls_back_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let prompt = load_merge_resolver_prompt(tmp.path());
        assert_eq!(prompt, FALLBACK_MERGE_RESOLVER_PROMPT);
    }

    #[test]
    fn merge_resolver_node_id_is_dunder() {
        assert_eq!(MERGE_RESOLVER_NODE_ID, "__merge_resolver__");
    }

    #[test]
    fn conflict_pending_resolution_keeps_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-pending";
        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let sub_wt_1 = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch_1 = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_1, &sub_branch_1, &pipeline_branch).unwrap();

        let sub_wt_2 = sub_worktree_path(repo, run_id, "impl-2", 1);
        let sub_branch_2 = sub_worktree_branch(run_id, "impl-2", 1);
        create_sub_worktree(repo, &sub_wt_2, &sub_branch_2, &pipeline_branch).unwrap();

        std::fs::write(sub_wt_1.join("shared.txt"), "from impl-1\n").unwrap();
        std::fs::write(sub_wt_2.join("shared.txt"), "from impl-2\n").unwrap();

        let r1 =
            commit_and_merge_sub_worktree(&sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1).unwrap();
        assert!(matches!(r1, MergeResult::Success));

        let r2 = commit_and_merge_sub_worktree_inner(
            &sub_wt_2,
            &wt_dir,
            &sub_branch_2,
            "impl-2",
            1,
            true,
        )
        .unwrap();
        assert!(
            matches!(r2, MergeResult::ConflictPendingResolution(_)),
            "expected ConflictPendingResolution"
        );

        // Conflict markers should remain in worktree (merge NOT aborted)
        let content = std::fs::read_to_string(wt_dir.join("shared.txt")).unwrap();
        assert!(
            content.contains("<<<<<<<"),
            "conflict markers should remain in the file"
        );
    }

    // --- Library HTTP integration tests ---

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn library_full_flow() {
        let _guard = crate::library_store::HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp = std::env::temp_dir().join(format!("pdo-lib-http-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);

        // Set up repo with a pipeline that has a named node
        let repo = tmp.join("repo");
        let pipe_dir = repo.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipe_dir).unwrap();
        let yaml = r#"name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: end
    name: End
    type: end
    inputs:
      - name: result
  - id: n1
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
edges: []
"#;
        std::fs::write(pipe_dir.join("test-pipe.yaml"), yaml).unwrap();
        let prompts_dir = pipe_dir.join("test-pipe.prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(prompts_dir.join("n1.md"), "You are a reviewer.").unwrap();

        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (event_tx, _) = broadcast::channel(64);
        let (pipeline_tx, _) = broadcast::channel(16);
        let state = Arc::new(AppState {
            db,
            event_tx,
            pipeline_tx,
            repo_root: repo.clone(),
            port: 0,
            merge_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
            trigger_tick_lock: tokio::sync::Mutex::new(()),
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(Mutex::new(None)),
            // Self-destructing no-op: should any test POST /runs and reach the
            // spawn path, the node session runs `true` and exits immediately —
            // never real claude, never a lingering session (#181).
            tmux_cmd_override: Some("exec true".to_string()),
        });
        let app = build_router(state);

        // GET /library — empty initially
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/library")
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
        assert_eq!(body.as_array().unwrap().len(), 0);

        // POST /library — save node directly from in-memory spec (no disk
        // round-trip through a pipeline file).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/library")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "name": "Reviewer",
                            "type": "doc-only",
                            "inputs": [{"name": "code"}],
                            "outputs": [{"name": "review"}],
                            "interactive": false,
                            "prompt": "You are a reviewer."
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let entry: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(entry["name"], "Reviewer");
        assert_eq!(entry["prompt"], "You are a reviewer.");

        // GET /library — now has one entry
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/library")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body.as_array().unwrap().len(), 1);

        // POST /library/Reviewer/instantiate
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/library/Reviewer/instantiate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let inst: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(inst["spec"]["name"], "Reviewer");
        assert_eq!(inst["prompt"], "You are a reviewer.");

        // DELETE /library/Reviewer
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/library/Reviewer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // DELETE again → 404
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/library/Reviewer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Cleanup
        if let Some(p) = prev_home {
            std::env::set_var("HOME", p);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn library_save_works_without_pipeline_on_disk() {
        let _guard = crate::library_store::HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp = std::env::temp_dir().join(format!("pdo-lib-nopipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);

        // No pipeline file is written. repo_root points to an empty dir.
        let repo = tmp.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_db(&db).await.unwrap();
        let (event_tx, _) = broadcast::channel(64);
        let (pipeline_tx, _) = broadcast::channel(16);
        let state = Arc::new(AppState {
            db,
            event_tx,
            pipeline_tx,
            repo_root: repo.clone(),
            port: 0,
            merge_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
            trigger_tick_lock: tokio::sync::Mutex::new(()),
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
            run_watcher: Arc::new(Mutex::new(None)),
            // Self-destructing no-op: should any test POST /runs and reach the
            // spawn path, the node session runs `true` and exits immediately —
            // never real claude, never a lingering session (#181).
            tmux_cmd_override: Some("exec true".to_string()),
        });
        let app = build_router(state);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/library")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "name": "DraftReviewer",
                            "type": "doc-only",
                            "inputs": [{"name": "in"}],
                            "outputs": [{"name": "out"}],
                            "interactive": false,
                            "prompt": "Inline prompt — pipeline never saved."
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["name"], "DraftReviewer");
        assert_eq!(body["prompt"], "Inline prompt — pipeline never saved.");

        // Cleanup
        let _ = library_store::delete("DraftReviewer");
        if let Some(p) = prev_home {
            std::env::set_var("HOME", p);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // --- Multi-repo run creation tests (issue #114) ---

    #[test]
    fn validate_target_repo_rejects_relative_path() {
        let result = validate_target_repo("relative/path");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("absolute path"));
    }

    #[test]
    fn validate_target_repo_rejects_nonexistent_path() {
        let result = validate_target_repo("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn validate_target_repo_rejects_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = validate_target_repo(tmp.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a git repository"));
    }

    #[test]
    fn validate_target_repo_accepts_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        let result = validate_target_repo(tmp.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_source_branch_rejects_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        let result = validate_source_branch(tmp.path(), "nonexistent-branch");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn validate_source_branch_accepts_existing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        // init_test_repo creates a default branch, find its name
        let output = std::process::Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(!branch.is_empty());
        let result = validate_source_branch(tmp.path(), &branch);
        assert!(result.is_ok());
    }

    #[test]
    fn list_branches_returns_branches() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        let result = list_branches(tmp.path());
        assert!(result.is_ok());
        let branches = result.unwrap();
        assert!(!branches.is_empty());
    }

    #[test]
    fn create_worktree_with_source_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        // Create a feature branch with a file
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap()
        };
        run(&["checkout", "-b", "feature-branch"]);
        std::fs::write(repo.join("feature.txt"), "feature content\n").unwrap();
        run(&["add", "feature.txt"]);
        run(&["commit", "-m", "add feature"]);
        // Go back to default branch
        let default_out = std::process::Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(repo)
            .output()
            .unwrap();
        let branch_list = String::from_utf8_lossy(&default_out.stdout).to_string();
        let default_branch = branch_list
            .trim()
            .lines()
            .find(|b| *b != "feature-branch")
            .unwrap_or("master");
        run(&["checkout", default_branch]);

        // Create worktree from feature-branch
        let wt_dir = repo
            .join(".pdo")
            .join("runs")
            .join("test-run")
            .join("worktree");
        create_worktree(repo, &wt_dir, "pdo/run-test-run", "feature-branch").unwrap();

        // The worktree should contain feature.txt from the feature branch
        assert!(wt_dir.join("feature.txt").exists());
        assert_eq!(
            std::fs::read_to_string(wt_dir.join("feature.txt")).unwrap(),
            "feature content\n"
        );
    }

    #[test]
    fn worktree_dir_for_run_follows_canonical_schema() {
        let path =
            worktree_dir_for_run(std::path::Path::new("/target-repo"), "20260101-120000-abc");
        assert_eq!(
            path,
            PathBuf::from("/target-repo/.pdo/runs/20260101-120000-abc/worktree")
        );
    }

    #[tokio::test]
    async fn effective_repo_root_uses_target_repo_when_set() {
        let state = test_state().await;

        let mut run_state = event_log::RunState::new("test".into(), "pipe".into());

        // Without target_repo — falls back to daemon root
        let default_root = effective_repo_root(&state, &run_state);
        assert_eq!(default_root, state.repo_root);

        // With target_repo — uses it
        run_state.target_repo = Some("/custom/repo".into());
        assert_eq!(
            effective_repo_root(&state, &run_state),
            PathBuf::from("/custom/repo")
        );
    }

    #[test]
    fn run_state_projection_includes_target_repo() {
        let events = vec![event_log::Event {
            id: None,
            run_id: "test-run".into(),
            ts: "2026-01-01T00:00:00Z".into(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "hello",
                "target_repo": "/custom/repo",
                "source_branch": "feature-branch",
                "edges": [],
                "node_defs": [],
            })),
        }];

        let state = event_log::project(&events).unwrap();
        assert_eq!(state.target_repo.as_deref(), Some("/custom/repo"));
        assert_eq!(state.source_branch.as_deref(), Some("feature-branch"));
    }

    #[test]
    fn run_state_projection_no_target_repo_when_absent() {
        let events = vec![event_log::Event {
            id: None,
            run_id: "test-run".into(),
            ts: "2026-01-01T00:00:00Z".into(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "test-pipe",
                "input": "hello",
                "edges": [],
                "node_defs": [],
            })),
        }];

        let state = event_log::project(&events).unwrap();
        assert!(state.target_repo.is_none());
        assert!(state.source_branch.is_none());
    }

    // --- Diff endpoint tests (refs #116) ---

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
    async fn node_diff_returns_404_for_nonexistent_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/runs/no-such-run/nodes/impl-1/diff")
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
        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;

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
        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;

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
    async fn node_diff_returns_per_node_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-node";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        // Create a sub-worktree for impl-1
        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        // Make a change in the sub-worktree
        std::fs::write(sub_wt_dir.join("node_file.rs"), "fn node_work() {}\n").unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&sub_wt_dir)
                .output()
                .unwrap()
        };
        run(&["add", "node_file.rs"]);
        run(&["commit", "-m", "node impl"]);

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/impl-1/diff"))
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
            body.contains("node_file.rs"),
            "diff should mention node_file.rs"
        );
        assert!(
            body.contains("fn node_work()"),
            "diff should contain the node's changes"
        );
    }

    #[tokio::test]
    async fn node_diff_returns_404_for_nonexistent_node() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "diff-no-node";
        let state = test_state_with_dir(repo).await;
        seed_run_with_node(&state, run_id, "impl-1", "code-mutating").await;

        let wt_dir = repo.join(".pdo/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("pdo/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch, "HEAD").unwrap();

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/nodes/nonexistent/diff"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn passthrough_switch_artifact_copies_input_to_matched_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let upstream_artifact = blackboard::artifact_path(&artifacts_dir, "reviewer", 1, "review");
        std::fs::create_dir_all(upstream_artifact.parent().unwrap()).unwrap();
        std::fs::write(&upstream_artifact, "---\nverdict: PASS\n---\nLooks good.\n").unwrap();

        let pipeline = pipeline::PipelineDef {
            name: "passthrough-test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![pipeline::EdgeDef {
                source: pipeline::EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: pipeline::EdgeEndpoint {
                    node: "sw".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            prompt_required: true,
        };

        let ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: "test-run",
            pipeline_path: tmp.path(),
            worktree_dir: tmp.path(),
            artifacts_dir: &artifacts_dir,
            resolved_vars: &HashMap::new(),
            repo_root: tmp.path(),
        };

        passthrough_switch_artifact(&ctx, "sw", "pass", 1);

        let dst = blackboard::artifact_path(&artifacts_dir, "sw", 1, "pass");
        assert!(dst.exists(), "passthrough artifact should exist at {dst:?}");
        let content = std::fs::read_to_string(&dst).unwrap();
        assert!(
            content.contains("verdict: PASS"),
            "passthrough should copy content verbatim"
        );
    }

    #[test]
    fn sanitize_image_filename_accepts_valid_extensions() {
        assert_eq!(
            sanitize_image_filename("photo.png"),
            Some("photo.png".into())
        );
        assert_eq!(sanitize_image_filename("img.JPG"), Some("img.JPG".into()));
        assert_eq!(sanitize_image_filename("pic.jpeg"), Some("pic.jpeg".into()));
        assert_eq!(sanitize_image_filename("anim.gif"), Some("anim.gif".into()));
        assert_eq!(
            sanitize_image_filename("modern.webp"),
            Some("modern.webp".into())
        );
        assert_eq!(sanitize_image_filename("icon.svg"), Some("icon.svg".into()));
        assert_eq!(sanitize_image_filename("old.bmp"), Some("old.bmp".into()));
    }

    #[test]
    fn sanitize_image_filename_rejects_non_image() {
        assert_eq!(sanitize_image_filename("script.js"), None);
        assert_eq!(sanitize_image_filename("data.json"), None);
        assert_eq!(sanitize_image_filename("readme.md"), None);
        assert_eq!(sanitize_image_filename("noext"), None);
    }

    #[test]
    fn sanitize_image_filename_strips_path_components() {
        assert_eq!(
            sanitize_image_filename("../../etc/passwd.png"),
            Some("passwd.png".into())
        );
        assert_eq!(
            sanitize_image_filename("/tmp/photo.jpg"),
            Some("photo.jpg".into())
        );
    }

    #[test]
    fn passthrough_switch_artifact_noop_when_no_source() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let pipeline = pipeline::PipelineDef {
            name: "passthrough-noop".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![pipeline::EdgeDef {
                source: pipeline::EdgeEndpoint {
                    node: "reviewer".into(),
                    port: "review".into(),
                },
                target: pipeline::EdgeEndpoint {
                    node: "sw".into(),
                    port: "in".into(),
                },
                reason: None,
                when: None,
                is_else: false,
                repeated: false,
                ..Default::default()
            }],
            loops: Vec::new(),
            prompt_required: true,
        };

        let ctx = SpawnContext {
            pipeline: &pipeline,
            run_id: "test-run",
            pipeline_path: tmp.path(),
            worktree_dir: tmp.path(),
            artifacts_dir: &artifacts_dir,
            resolved_vars: &HashMap::new(),
            repo_root: tmp.path(),
        };

        passthrough_switch_artifact(&ctx, "sw", "pass", 1);

        let dst = blackboard::artifact_path(&artifacts_dir, "sw", 1, "pass");
        assert!(
            !dst.exists(),
            "no artifact should be created when source is missing"
        );
    }

    #[test]
    fn image_files_stored_in_input_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let input_dir = tmp.path().join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "hello").unwrap();

        let images = vec![
            ImageFile {
                filename: "screenshot.png".into(),
                data: vec![0x89, 0x50, 0x4E, 0x47],
            },
            ImageFile {
                filename: "diagram.jpg".into(),
                data: vec![0xFF, 0xD8, 0xFF],
            },
        ];
        for image in &images {
            std::fs::write(input_dir.join(&image.filename), &image.data).unwrap();
        }

        assert!(input_dir.join("output.md").exists());
        assert!(input_dir.join("screenshot.png").exists());
        assert!(input_dir.join("diagram.jpg").exists());
        assert_eq!(
            std::fs::read(input_dir.join("screenshot.png")).unwrap(),
            vec![0x89, 0x50, 0x4E, 0x47]
        );
    }

    // --- Pause/Resume/Retry-all tests ---

    #[tokio::test]
    async fn pause_run_sets_paused_status() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "pause-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/pause-test/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "pause_run" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "pause-test").await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Paused);
    }

    #[tokio::test]
    async fn pause_run_rejected_on_terminal_state() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "pause-completed".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let completed_event = event_log::Event {
            id: None,
            run_id: "pause-completed".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunCompleted,
            node_id: None,
            iter: None,
            payload: None,
        };
        append_event(&state, &completed_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/pause-completed/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "pause_run" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn resume_run_from_paused_emits_run_resumed() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "resume-paused".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let pause_event = event_log::Event {
            id: None,
            run_id: "resume-paused".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunPaused,
            node_id: None,
            iter: None,
            payload: None,
        };
        append_event(&state, &pause_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/resume-paused/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "resume_run" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "resume-paused").await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(run_state.status, event_log::RunStatus::Running);

        let has_resumed = events
            .iter()
            .any(|e| e.kind == event_log::EventKind::RunResumed);
        assert!(has_resumed, "should have emitted RunResumed event");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn retry_all_archives_and_creates_new_run() {
        // retry_all reconstructs a fresh run by re-resolving the original
        // pipeline by name from disk and creating a new git worktree for it,
        // so the repo root must be a real git repo with the template present.
        // Isolate HOME so user-scoped resolution/cleanup can't leak.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        write_test_pipeline(tmp.path(), "retry-pipe");
        let state = test_state_with_dir(tmp.path()).await;

        let run_event = event_log::Event {
            id: None,
            run_id: "retry-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({
                "pipeline_name": "retry-pipe",
                "input": "do the thing",
                "edges": [],
                "node_defs": [],
            })),
        };
        append_event(&state, &run_event).await.unwrap();

        let fail_event = event_log::Event {
            id: None,
            run_id: "retry-test".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunFailed,
            node_id: None,
            iter: None,
            payload: None,
        };
        append_event(&state, &fail_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/retry-test/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "retry_all" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // retry_all returns create_run_core's response verbatim, which is 201 Created.
        assert_eq!(status, StatusCode::CREATED, "retry_all body: {json}");
        assert!(json.get("run_id").is_some(), "should return new run_id");

        let old_events = load_events(&state.db, "retry-test").await.unwrap();
        let old_state = event_log::project(&old_events).unwrap();
        assert_eq!(old_state.status, event_log::RunStatus::Archived);

        let new_run_id = json["run_id"].as_str().unwrap();
        let new_events = load_events(&state.db, new_run_id).await.unwrap();
        let new_state = event_log::project(&new_events).unwrap();
        assert_eq!(new_state.status, event_log::RunStatus::Running);
        assert_eq!(new_state.pipeline_name, "retry-pipe");
        assert_eq!(new_state.input.as_deref(), Some("do the thing"));
    }

    #[tokio::test]
    async fn retry_all_rejected_on_running_state() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "retry-running".into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::RunStarted,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
        };
        append_event(&state, &run_event).await.unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/retry-running/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "retry_all" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    // --- Per-node control tests ---

    async fn seed_run_for_node_control(state: &Arc<AppState>, run_id: &str, pipeline_name: &str) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
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
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: None,
        };
        append_event(state, &ev).await.unwrap();
    }

    async fn seed_node_completed(state: &Arc<AppState>, run_id: &str, node_id: &str, iter: i64) {
        let ev = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeCompleted,
            node_id: Some(node_id.into()),
            iter: Some(iter),
            payload: None,
        };
        append_event(state, &ev).await.unwrap();
    }

    #[tokio::test]
    async fn node_stop_returns_404_for_unknown_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/no-such-run/nodes/worker/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_stop_returns_conflict_when_not_running() {
        let state = test_state().await;
        seed_run_for_node_control(&state, "stop-notrun", "test-pipe").await;
        seed_node_started(&state, "stop-notrun", "worker", 1).await;
        seed_node_completed(&state, "stop-notrun", "worker", 1).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/stop-notrun/nodes/worker/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn node_stop_returns_not_found_for_unknown_node() {
        let state = test_state().await;
        seed_run_for_node_control(&state, "stop-nonode", "test-pipe").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/stop-nonode/nodes/ghost/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_stop_emits_stopped_event() {
        let state = test_state().await;
        seed_run_for_node_control(&state, "stop-ok", "test-pipe").await;
        seed_node_started(&state, "stop-ok", "worker", 1).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/stop-ok/nodes/worker/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, "stop-ok").await.unwrap();
        let stopped = events
            .iter()
            .find(|e| e.kind == event_log::EventKind::NodeStopped);
        assert!(stopped.is_some(), "should have NodeStopped event");
        let stopped = stopped.unwrap();
        assert_eq!(stopped.node_id.as_deref(), Some("worker"));
        assert_eq!(stopped.iter, Some(1));

        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes.get("worker").unwrap().status,
            event_log::NodeStatus::Stopped
        );
        assert_ne!(
            run_state.status,
            event_log::RunStatus::Failed,
            "NodeStopped must NOT transition the run to failed"
        );
    }

    #[tokio::test]
    async fn node_stop_idempotent_second_call_returns_conflict() {
        let state = test_state().await;
        seed_run_for_node_control(&state, "stop-idem", "test-pipe").await;
        seed_node_started(&state, "stop-idem", "worker", 1).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/stop-idem/nodes/worker/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let app2 = build_router(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/stop-idem/nodes/worker/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn node_start_returns_conflict_when_already_running() {
        let state = test_state().await;
        seed_run_for_node_control(&state, "start-conflict", "test-pipe").await;
        seed_node_started(&state, "start-conflict", "worker", 1).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/start-conflict/nodes/worker/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn node_start_returns_404_for_unknown_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/no-such-run/nodes/worker/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_retry_returns_404_for_unknown_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/no-such-run/nodes/worker/retry")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_retry_invalidates_downstream_and_returns_list() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;

        let run_id = "retry-downstream";
        let pipeline_yaml = "\
name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: code
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
";

        let pipelines_dir = repo_root.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

        let worktree_dir = worktree_dir_for_run(repo_root, run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let reviewer_artifacts = artifacts_dir.join("reviewer").join("iter-1").join("review");
        std::fs::create_dir_all(&reviewer_artifacts).unwrap();
        std::fs::write(reviewer_artifacts.join("output.md"), "old review").unwrap();

        seed_run_for_node_control(&state, run_id, "test-pipe").await;
        seed_node_started(&state, run_id, "worker", 1).await;
        seed_node_completed(&state, run_id, "worker", 1).await;
        seed_node_started(&state, run_id, "reviewer", 1).await;
        seed_node_completed(&state, run_id, "reviewer", 1).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["iter"], 2);

        let invalidated = json["invalidated"]
            .as_array()
            .expect("invalidated should be an array");
        assert!(
            invalidated.iter().any(|v| v == "reviewer"),
            "reviewer should be in invalidated list, got: {invalidated:?}"
        );

        assert!(
            !reviewer_artifacts.exists(),
            "reviewer artifacts should have been deleted"
        );

        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();

        assert!(
            !run_state.nodes.contains_key("reviewer"),
            "reviewer should be removed from state (pending) after invalidation"
        );

        let has_invalidated_worker = events.iter().any(|e| {
            e.kind == event_log::EventKind::NodeInvalidated
                && e.node_id.as_deref() == Some("worker")
        });
        assert!(
            has_invalidated_worker,
            "should have NodeInvalidated event for worker (self-invalidation)"
        );

        let has_invalidated_reviewer = events.iter().any(|e| {
            e.kind == event_log::EventKind::NodeInvalidated
                && e.node_id.as_deref() == Some("reviewer")
        });
        assert!(
            has_invalidated_reviewer,
            "should have NodeInvalidated event for reviewer (downstream)"
        );
    }

    #[tokio::test]
    async fn node_retry_stops_running_node_first() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;

        let run_id = "retry-stop-first";
        let pipeline_yaml = "\
name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";
        let pipelines_dir = repo_root.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

        let worktree_dir = worktree_dir_for_run(repo_root, run_id);
        std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

        seed_run_for_node_control(&state, run_id, "test-pipe").await;
        seed_node_started(&state, run_id, "worker", 1).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        let has_stopped = events
            .iter()
            .any(|e| e.kind == event_log::EventKind::NodeStopped);
        assert!(has_stopped, "should have NodeStopped event before retry");
    }

    #[tokio::test]
    async fn node_retry_idempotent_on_pending_node() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;

        let run_id = "retry-pending";
        let pipeline_yaml = "\
name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";
        let pipelines_dir = repo_root.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

        let worktree_dir = worktree_dir_for_run(repo_root, run_id);
        std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

        seed_run_for_node_control(&state, run_id, "test-pipe").await;
        // worker has never started — it's implicitly pending (not in run state)

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/worker/retry"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["iter"], 2);
    }

    #[tokio::test]
    async fn node_retry_preview_returns_downstream_info() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;

        let run_id = "preview-test";
        let pipeline_yaml = "\
name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: code
  - id: reviewer
    name: Reviewer
    type: doc-only
    inputs:
      - name: code
    outputs:
      - name: review
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: reviewer, port: code }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
";

        let pipelines_dir = repo_root.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

        let worktree_dir = worktree_dir_for_run(repo_root, run_id);
        let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
        let reviewer_artifacts = artifacts_dir.join("reviewer").join("iter-1");
        std::fs::create_dir_all(&reviewer_artifacts).unwrap();
        std::fs::write(reviewer_artifacts.join("review.md"), "old review").unwrap();

        seed_run_for_node_control(&state, run_id, "test-pipe").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/runs/{run_id}/nodes/worker/retry/preview"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let downstream = json["downstream"].as_array().unwrap();
        assert!(
            downstream.iter().any(|v| v == "reviewer"),
            "reviewer should be in downstream"
        );

        assert_eq!(
            json["affected_count"], 1,
            "reviewer has artifacts, so affected_count should be 1"
        );

        let with_artifacts = json["with_artifacts"].as_array().unwrap();
        assert!(
            with_artifacts.iter().any(|v| v == "reviewer"),
            "reviewer should be in with_artifacts"
        );
    }

    #[tokio::test]
    async fn node_retry_preview_returns_404_for_unknown_run() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/runs/no-such-run/nodes/worker/retry/preview")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn node_retry_preview_zero_affected_when_no_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path();
        let state = test_state_with_dir(repo_root).await;

        let run_id = "preview-no-artifacts";
        let pipeline_yaml = "\
name: test-pipe
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: doc-only
    inputs:
      - name: task
    outputs:
      - name: code
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: code }
    target: { node: end, port: result }
";

        let pipelines_dir = repo_root.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        std::fs::write(pipelines_dir.join("test-pipe.yaml"), pipeline_yaml).unwrap();

        let worktree_dir = worktree_dir_for_run(repo_root, run_id);
        std::fs::create_dir_all(worktree_dir.join(".pdo").join("artifacts")).unwrap();

        seed_run_for_node_control(&state, run_id, "test-pipe").await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/runs/{run_id}/nodes/worker/retry/preview"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["affected_count"], 0);
    }

    // --- GET /repos/recent (issue #132) ---

    static SEED_TS_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    async fn seed_run_started(state: &Arc<AppState>, run_id: &str, target_repo: Option<&str>) {
        let seq = SEED_TS_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ts = format!("2026-01-01T00:00:{:02}.000Z", seq);
        let mut payload = serde_json::json!({
            "pipeline_name": "test-pipe",
            "input": "test",
        });
        if let Some(repo) = target_repo {
            payload["target_repo"] = serde_json::json!(repo);
        }
        append_event(
            state,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts,
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(payload),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn repos_recent_empty_when_no_events() {
        let state = test_state().await;
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/repos/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let repos: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert!(repos.is_empty());
    }

    #[tokio::test]
    async fn repos_recent_returns_single_repo() {
        let state = test_state().await;
        seed_run_started(&state, "run-1", Some("/home/user/project-a")).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/repos/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let repos: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(repos, vec!["/home/user/project-a"]);
    }

    #[tokio::test]
    async fn repos_recent_deduplicates_and_orders_by_most_recent() {
        let state = test_state().await;
        seed_run_started(&state, "run-1", Some("/repo/a")).await;
        seed_run_started(&state, "run-2", Some("/repo/b")).await;
        seed_run_started(&state, "run-3", Some("/repo/a")).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/repos/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let repos: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(repos, vec!["/repo/a", "/repo/b"]);
    }

    #[tokio::test]
    async fn repos_recent_limits_to_5() {
        let state = test_state().await;
        for i in 0..8 {
            seed_run_started(&state, &format!("run-{i}"), Some(&format!("/repo/{i}"))).await;
        }

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/repos/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let repos: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(repos.len(), 5);
    }

    #[tokio::test]
    async fn repos_recent_ignores_events_without_target_repo() {
        let state = test_state().await;
        seed_run_started(&state, "run-1", None).await;
        seed_run_started(&state, "run-2", Some("/repo/real")).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/repos/recent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let repos: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert_eq!(repos, vec!["/repo/real"]);
    }

    // --- prompt-optional run creation (#158) ---

    #[test]
    fn prompt_required_pipeline_rejects_empty_input() {
        assert!(validate_run_input(true, "").is_err());
        assert!(validate_run_input(true, "   \n\t ").is_err());
    }

    #[test]
    fn prompt_required_pipeline_accepts_non_empty_input() {
        assert!(validate_run_input(true, "fix the auth bug").is_ok());
    }

    #[test]
    fn prompt_optional_pipeline_accepts_empty_input() {
        assert!(validate_run_input(false, "").is_ok());
        assert!(validate_run_input(false, "   ").is_ok());
    }

    #[test]
    fn prompt_optional_pipeline_still_accepts_a_prompt() {
        assert!(validate_run_input(false, "extra context").is_ok());
    }

    // --- launch validation: dangling port references (#211 / #206) ---

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_refuses_pipeline_with_dangling_port() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();

        // `worker` declares output `result`, but the edge sources `resullt`:
        // a dangling port reference that would stall the run mid-flight.
        let pipelines_dir = tmp.path().join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: dangling-pipe\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    outputs:\n      - name: result\nedges:\n  - source: {{ node: worker, port: resullt }}\n    target: {{ node: end, port: result }}\n"
        );
        std::fs::write(pipelines_dir.join("dangling-pipe.yaml"), yaml).unwrap();

        let state = test_state_with_dir(tmp.path()).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"pipeline": "dangling-pipe", "input": "do the thing"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a dangling port reference must refuse the launch"
        );
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let error = body["error"].as_str().unwrap();
        assert!(
            error.contains("'resullt'") && error.contains("worker"),
            "error must name the missing port and the edge; got: {error}"
        );
        assert!(
            error.contains("end"),
            "error must identify the edge (both endpoints); got: {error}"
        );

        // No run must have been created.
        let app = build_router(state);
        let resp = app
            .oneshot(Request::builder().uri("/runs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let runs: Vec<serde_json::Value> = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(runs.is_empty(), "no run must be created on refusal");
    }
}
