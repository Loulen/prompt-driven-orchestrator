mod blackboard;
#[allow(dead_code)]
mod condition;
mod event_log;
mod frontmatter_parser;
mod node_io_resolver;
mod outputs_validator;
mod pipeline;
pub mod pipeline_migrator;
mod pipeline_watcher;
mod prompt_augmenter;
mod scheduler;
mod scheduler_dispatcher;
pub mod tmux_session_manager;
#[allow(dead_code)]
mod variable_resolver;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Json, Path as AxumPath, Query, State, WebSocketUpgrade};
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
    name = "maestro",
    about = "Maestro — deterministic Claude Code pipeline orchestrator",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the Maestro daemon
    Daemon {
        #[arg(short, long, env = "MAESTRO_PORT", default_value_t = DEFAULT_PORT)]
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
    /// Paths the daemon has just written. The pipeline watcher consults this map
    /// and suppresses `pipeline_changed` broadcasts for paths it sees within the
    /// TTL window — that's how we tell our own writes apart from external ones
    /// (vim, git checkout, future Pipeline Manager) without ignoring the latter.
    recent_writes: Arc<Mutex<HashMap<PathBuf, Instant>>>,
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
    started_at: Option<String>,
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
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct IterQuery {
    #[serde(default = "default_iter")]
    iter: i64,
}

fn default_iter() -> i64 {
    1
}

fn cli_daemon_url() -> String {
    std::env::var("MAESTRO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string())
}

fn cli_run_id() -> Result<String> {
    std::env::var("MAESTRO_RUN_ID").context(
        "MAESTRO_RUN_ID not set — this command must be run inside a Maestro NodeRun session",
    )
}

fn cli_node_id() -> Result<String> {
    std::env::var("MAESTRO_NODE_ID").context(
        "MAESTRO_NODE_ID not set — this command must be run inside a Maestro NodeRun session",
    )
}

fn cli_node_iter() -> i64 {
    std::env::var("MAESTRO_NODE_ITER")
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
}

pub async fn serve(addr: SocketAddr, repo_root: PathBuf) -> Result<DaemonHandle> {
    let db_dir = repo_root.join(".maestro");
    std::fs::create_dir_all(&db_dir).context("failed to create .maestro directory")?;
    let db_path = db_dir.join("maestro.db");

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
        recent_writes,
    });

    if let Err(e) = run_orphan_sweep(&state.db, tmux_session_manager::reaper_ttl()).await {
        warn!("Orphan sweep at boot failed: {e}");
    }

    let app = build_router(state.clone());

    info!("Maestro daemon listening on http://{bound_addr}");

    // Spawn reaper background task
    let reaper_state = state.clone();
    let _reaper_handle = tokio::spawn(async move {
        let interval = tmux_session_manager::reaper_interval();
        let ttl = tmux_session_manager::reaper_ttl();
        let mut tick = time::interval(interval);
        loop {
            tick.tick().await;
            if let Err(e) = run_orphan_sweep(&reaper_state.db, ttl).await {
                warn!("Reaper sweep failed: {e}");
            }
        }
    });

    // Background task: process run-scoped pipeline modifications
    let mod_state = state.clone();
    let _run_modified_handle = tokio::spawn(async move {
        handle_run_pipeline_modifications(mod_state, run_modified_rx).await;
    });

    let task = tokio::spawn(async move {
        let _watcher = watcher; // keep the file watcher alive for the server's lifetime
        let _reaper = _reaper_handle; // keep the reaper alive
        let _run_modified = _run_modified_handle; // keep the pipeline_modified handler alive
        axum::serve(listener, app).await.context("server error")?;
        Ok(())
    });

    Ok(DaemonHandle {
        addr: bound_addr,
        task,
    })
}

pub async fn run_daemon(port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let repo_root = std::env::current_dir().context("failed to determine current directory")?;
    let handle = serve(addr, repo_root).await?;
    handle.task.await.context("daemon task join error")?
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/pipelines", get(list_pipelines))
        .route("/pipelines/{pipeline_id}", get(get_pipeline))
        .route(
            "/pipelines/{pipeline_id}",
            axum::routing::put(save_pipeline),
        )
        .route("/pipelines", post(create_pipeline))
        .route("/runs", post(create_run))
        .route("/runs", get(list_runs))
        .route("/runs/{run_id}", get(get_run))
        .route("/runs/{run_id}/events", get(get_run_events))
        .route("/runs/{run_id}/nodes/{node_id}/done", post(node_done))
        .route("/runs/{run_id}/nodes/{node_id}/fail", post(node_fail))
        .route("/runs/{run_id}/nodes/{node_id}/pane", get(node_pane))
        .route("/runs/{run_id}/nodes/{node_id}/prompt", get(node_prompt))
        .route("/runs/{run_id}/nodes/{node_id}/io", get(node_io))
        .route("/runs/{run_id}/artifact", get(artifact))
        .route("/runs/{run_id}/pipeline", get(get_run_pipeline))
        .route(
            "/runs/{run_id}/pipeline",
            axum::routing::put(save_run_pipeline),
        )
        .route("/runs/{run_id}/commands", post(run_command))
        .route("/sessions/{session_id}/attach", post(session_attach))
        .route("/sessions/{run_id}/manager/attach", post(manager_attach))
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

    Ok(())
}

async fn append_event(state: &AppState, event: &event_log::Event) -> Result<()> {
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

        let (name, node_count, variables) = match std::fs::read_to_string(&path) {
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
                    (r.pipeline.name.clone(), r.pipeline.nodes.len(), vars)
                }
                Err(_) => (file_stem.clone(), 0, HashMap::new()),
            },
            Err(_) => (file_stem.clone(), 0, HashMap::new()),
        };

        entries.push(PipelineListEntry {
            id: file_stem,
            name,
            scope: scope.to_string(),
            path: path.to_string_lossy().to_string(),
            node_count,
            modified,
            variables,
        });
    }
    entries
}

async fn list_pipelines(State(state): State<Arc<AppState>>) -> Response {
    let repo_dir = state.repo_root.join(".maestro").join("pipelines");
    let mut pipelines = scan_pipeline_dir(&repo_dir, "repo");

    if let Some(home) = dirs_next_home() {
        let user_dir = home.join(".maestro").join("pipelines");
        pipelines.extend(scan_pipeline_dir(&user_dir, "user"));
    }

    Json(pipelines).into_response()
}

async fn get_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
) -> Response {
    let path = resolve_pipeline_path(&state.repo_root, &pipeline_id);
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

async fn save_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(pipeline_id): AxumPath<String>,
    Json(req): Json<SavePipelineRequest>,
) -> Response {
    let path = resolve_pipeline_path(&state.repo_root, &pipeline_id);
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "pipeline not found").into_response();
    }

    if let Err(e) = pipeline::parse_pipeline(&req.yaml) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid YAML: {e}") })),
        )
            .into_response();
    }

    mark_self_write(&state.recent_writes, &path);
    if let Err(e) = std::fs::write(&path, &req.yaml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {e}") })),
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
            Some(home) => home.join(".maestro").join("pipelines"),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "cannot determine home directory",
                )
                    .into_response();
            }
        }
    } else {
        state.repo_root.join(".maestro").join("pipelines")
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

// --- API handlers ---

struct SpawnContext<'a> {
    pipeline: &'a pipeline::PipelineDef,
    run_id: &'a str,
    pipeline_path: &'a std::path::Path,
    worktree_dir: &'a std::path::Path,
    artifacts_dir: &'a std::path::Path,
    resolved_vars: &'a HashMap<String, serde_yaml::Value>,
}

async fn spawn_node(
    state: &AppState,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) {
    let run_id = spawn_ctx.run_id;
    let canonical_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let aug_ctx = prompt_augmenter::AugmentContext {
        pipeline: spawn_ctx.pipeline,
        node,
        run_id,
        iter,
        artifacts_dir: spawn_ctx.artifacts_dir,
        variables: spawn_ctx.resolved_vars,
        daemon_url: &format!("http://localhost:{}", state.port),
    };

    let full_prompt = prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

    let working_dir = if node.node_type == pipeline::NodeType::CodeMutating {
        let sub_wt_dir = sub_worktree_path(&state.repo_root, run_id, &node.id, 1);
        let sub_branch = sub_worktree_branch(run_id, &node.id, 1);
        let pipeline_branch = format!("maestro/run-{run_id}");

        if let Err(e) =
            create_sub_worktree(&state.repo_root, &sub_wt_dir, &sub_branch, &pipeline_branch)
        {
            error!("failed to create sub-worktree for {}: {e}", node.id);
            return;
        }
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

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
            },
        })),
    };
    if let Err(e) = append_event(state, &node_started).await {
        error!("failed to append node_started: {e}");
    }

    let session_name = tmux_session_manager::node_session_name(run_id, &node.id, iter);
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &full_prompt,
        &working_dir,
        run_id,
        &node.id,
        iter,
        state.port,
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
    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&state.repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&state.repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };

    let pipeline = parse_result.pipeline;

    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("worktree");
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");

    let resolved_vars = resolve_run_variables(&pipeline, events);

    let source_iter = run_state
        .nodes
        .get(completed_node_id)
        .map(|n| n.iter)
        .unwrap_or(1);
    let frontmatter_fields =
        resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

    let actions = scheduler::evaluate_outgoing_edges_with_context(
        &pipeline,
        run_state,
        completed_node_id,
        &resolved_vars,
        &frontmatter_fields,
    );

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
    };

    for action in &actions {
        match action {
            scheduler::SchedulerAction::Spawn { node_id, iter } => {
                if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                    spawn_node(state, &spawn_ctx, node, *iter).await;
                }
            }
            scheduler::SchedulerAction::Halt { message } => {
                let halt_event = event_log::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: event_log::now_iso(),
                    kind: event_log::EventKind::RunHalted,
                    node_id: None,
                    iter: None,
                    payload: Some(serde_json::json!({ "message": message })),
                };
                if let Err(e) = append_event(state, &halt_event).await {
                    error!("failed to append run_halted: {e}");
                }
                return;
            }
        }
    }
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

    let pipeline_path =
        resolve_run_pipeline_path(&state.repo_root, run_id, &run_state.pipeline_name);
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };
    let pipeline = parse_result.pipeline;

    let ready = scheduler_dispatcher::compute_ready_to_spawn(&pipeline, &run_state);
    if ready.is_empty() {
        // Pipeline was modified but no new nodes need spawning. If all current
        // pipeline nodes are completed, re-complete the run so it doesn't stay
        // dangling in Running state after a trivial YAML edit.
        maybe_complete_run(state, run_id, &pipeline, &run_state).await;
        return;
    }

    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("worktree");
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
    let resolved_vars = resolve_run_variables(&pipeline, &events);

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
    };

    for rs in &ready {
        if let Some(node) = pipeline.nodes.iter().find(|n| n.id == rs.node_id) {
            spawn_node(state, &spawn_ctx, node, rs.iter).await;
        }
    }

    info!(
        "spawn_ready_after_event: spawned {} node(s) for run {run_id}",
        ready.len()
    );
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
        let artifact_path = artifacts_dir
            .join(completed_node_id)
            .join(format!("iter-{iter}"))
            .join(format!("{}.md", port.name));
        if let Ok(port_fields) = frontmatter_parser::parse_frontmatter_from_file(&artifact_path) {
            for (k, v) in port_fields {
                fields.insert(k, v);
            }
        }
    }
    fields
}

async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRunRequest>,
) -> Response {
    let pipeline_path = resolve_pipeline_path(&state.repo_root, &req.pipeline);
    let yaml = match std::fs::read_to_string(&pipeline_path) {
        Ok(y) => y,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("cannot read pipeline: {e}") })),
            )
                .into_response();
        }
    };

    let parse_result = match pipeline::parse_pipeline(&yaml) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("pipeline parse error: {e}") })),
            )
                .into_response();
        }
    };

    for diag in &parse_result.diagnostics {
        warn!("pipeline {}: {}", req.pipeline, diag.message);
    }

    let pipeline = parse_result.pipeline;
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

    let run_started = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(run_payload),
    };

    if let Err(e) = append_event(&state, &run_started).await {
        error!("failed to append run_started: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response();
    }

    // Create worktree
    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("worktree");
    let branch_name = format!("maestro/run-{run_id}");

    if let Err(e) = create_worktree(&state.repo_root, &worktree_dir, &branch_name) {
        error!("failed to create worktree: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("worktree creation failed: {e}") })),
        )
            .into_response();
    }

    // Copy pipeline YAML + prompts to run-scoped location
    if let Err(e) = copy_pipeline_to_run(&state.repo_root, &pipeline_path, &run_id) {
        error!("failed to copy pipeline to run dir: {e}");
    }

    // Write _input.md
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
    if let Err(e) = std::fs::create_dir_all(&artifacts_dir) {
        error!("failed to create artifacts dir: {e}");
    }
    let input_path = artifacts_dir.join("_input.md");
    if let Err(e) = std::fs::write(&input_path, &req.input) {
        error!("failed to write _input.md: {e}");
    }

    spawn_ready_after_event(&state, &run_id).await;

    spawn_manager_session(&state, &run_id, &worktree_dir);

    info!("Run {run_id} started for pipeline {}", pipeline.name);

    (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response()
}

fn spawn_manager_session(state: &AppState, run_id: &str, worktree_dir: &std::path::Path) {
    let daemon_url = format!("http://localhost:{}", state.port);

    let static_prompt = std::fs::read_to_string(
        state
            .repo_root
            .join("prompts")
            .join("builtin")
            .join("manager.md"),
    )
    .unwrap_or_default();

    let full_prompt = prompt_augmenter::build_manager_prompt(run_id, &daemon_url, &static_prompt);

    let session_name = tmux_session_manager::manager_session_name(run_id);
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        &full_prompt,
        worktree_dir,
        run_id,
        "__manager__",
        0,
        state.port,
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
            runs.push(RunListEntry {
                run_id: run_state.run_id,
                pipeline_name: run_state.pipeline_name,
                status: run_state.status,
                started_at: run_state.started_at,
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
            augment_run_state_from_disk(&mut run_state, &state.repo_root);
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
    let yaml_path = run_scoped_pipeline_path(&state.repo_root, &run_id);
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

    let prompts_dir = run_scoped_prompts_dir(&state.repo_root, &run_id);
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
    let yaml_path = run_scoped_pipeline_path(&state.repo_root, &run_id);
    if !yaml_path.exists() {
        return (StatusCode::NOT_FOUND, "run-scoped pipeline not found").into_response();
    }

    if let Err(e) = pipeline::parse_pipeline(&req.yaml) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("invalid YAML: {e}") })),
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

    let prompts_dir = run_scoped_prompts_dir(&state.repo_root, &run_id);
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

    info!("Run-scoped pipeline for {run_id} saved");
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// --- Orphan sweep / reaper ---

async fn run_orphan_sweep(db: &sqlx::SqlitePool, ttl: Duration) -> Result<()> {
    let run_ids = load_all_run_ids(db).await?;
    let mut run_states: HashMap<String, event_log::RunState> = HashMap::new();

    for run_id in &run_ids {
        let events = load_events(db, run_id).await?;
        if let Some(state) = event_log::project(&events) {
            run_states.insert(run_id.clone(), state);
        }
    }

    tmux_session_manager::sweep_orphans(
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

    if let Some(content) = tmux_session_manager::capture(&session_name) {
        return Json(PaneResponse {
            content,
            session_name,
            resumed: false,
            stale: !is_latest_iter,
        })
        .into_response();
    }

    if is_latest_iter && node_state.status != event_log::NodeStatus::Pending {
        let node_type = find_node_type(&run_state, &node_id).unwrap_or("doc-only");
        let working_dir = tmux_session_manager::working_dir_for_node(
            &state.repo_root,
            &run_id,
            &node_id,
            iter,
            node_type,
        );

        if working_dir.exists() {
            if let Err(e) = tmux_session_manager::resume(
                &session_name,
                &working_dir,
                &run_id,
                &node_id,
                iter,
                state.port,
            ) {
                warn!("Failed to resume session {session_name}: {e}");
                return Json(PaneResponse {
                    content: "Session no longer available".to_string(),
                    session_name,
                    resumed: false,
                    stale: false,
                })
                .into_response();
            }

            // Give the resumed session a moment to initialize
            tokio::time::sleep(Duration::from_millis(500)).await;

            let content = tmux_session_manager::capture(&session_name)
                .unwrap_or_else(|| "Connecting...".to_string());

            return Json(PaneResponse {
                content,
                session_name,
                resumed: true,
                stale: false,
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

    let node_type = find_node_type(&run_state, &node_id).unwrap_or("doc-only");
    let working_dir = tmux_session_manager::working_dir_for_node(
        &state.repo_root,
        &run_id,
        &node_id,
        iter,
        node_type,
    );

    let prompt_path = working_dir
        .join(".maestro")
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

    let yaml_path = run_scoped_pipeline_path(&state.repo_root, &run_id);
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

    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("worktree");
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");

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

    if event_log::project(&events).is_none() {
        return (StatusCode::NOT_FOUND, "run not found").into_response();
    }

    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("worktree");
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");

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

    match std::fs::read_to_string(&resolved) {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/markdown")],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
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

    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(&run_id)
        .join("worktree");

    if node_id == MERGE_RESOLVER_NODE_ID {
        return handle_merge_resolver_done(&state, &run_id, &worktree_dir, &pre_run_state).await;
    }

    let auto_merge_resolver = {
        let pipeline_path =
            resolve_run_pipeline_path(&state.repo_root, &run_id, &pre_run_state.pipeline_name);
        std::fs::read_to_string(&pipeline_path)
            .ok()
            .and_then(|yaml| pipeline::parse_pipeline(&yaml).ok())
            .map(|pr| pr.pipeline.auto_merge_resolver)
            .unwrap_or(true)
    };

    match find_node_type(&pre_run_state, &node_id) {
        Some("code-mutating") => {
            let sub_wt_dir = sub_worktree_path(&state.repo_root, &run_id, &node_id, iter);
            let sub_branch = sub_worktree_branch(&run_id, &node_id, iter);

            let _lock = state.merge_lock.lock().await;
            let merge_result = match commit_and_merge_sub_worktree_inner(
                &sub_wt_dir,
                &worktree_dir,
                &sub_branch,
                &node_id,
                iter,
                auto_merge_resolver,
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

                    warn!("Merge conflict for node {node_id} in run {run_id} (auto_merge_resolver disabled)");
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
        resolve_run_pipeline_path(&state.repo_root, &run_id, &pre_run_state.pipeline_name);
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
    if let Some(resp) = check_output_validation(&pipeline_path, &node_id, iter, &artifacts_dir) {
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

    // Per #23: session stays alive for terminal preview post-failure.

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

    info!("Node {node_id} failed in run {run_id}");
    (StatusCode::OK, "ok").into_response()
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
            if let Some(ref rs) = run_state {
                if let Some(node) = rs.nodes.get(&node_id) {
                    if node.status != event_log::NodeStatus::AwaitingUser
                        && node.status != event_log::NodeStatus::Running
                        && node.status != event_log::NodeStatus::Failed
                    {
                        return (
                            StatusCode::CONFLICT,
                            Json(serde_json::json!({
                                "error": format!("node {} is {:?}, cannot mark done", node_id, node.status)
                            })),
                        )
                            .into_response();
                    }
                }
            }

            let pipeline_name = run_state
                .as_ref()
                .map(|rs| rs.pipeline_name.as_str())
                .unwrap_or("");
            let pipeline_path = resolve_run_pipeline_path(&state.repo_root, &run_id, pipeline_name);
            let artifacts_dir = state
                .repo_root
                .join(".maestro")
                .join("runs")
                .join(&run_id)
                .join("worktree/.maestro/artifacts");
            if let Some(resp) =
                check_output_validation(&pipeline_path, &node_id, iter, &artifacts_dir)
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
        "resume_run" => {
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
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
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
            tmux_session_manager::kill(&session_name);

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

            // Kill existing session
            let session_name = tmux_session_manager::node_session_name(&run_id, &node_id, iter);
            tmux_session_manager::kill(&session_name);

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

            let pipeline_path = {
                let run_scoped = run_scoped_pipeline_path(&state.repo_root, &run_id);
                if run_scoped.exists() {
                    run_scoped
                } else {
                    resolve_pipeline_path(&state.repo_root, &run_state.pipeline_name)
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
                let worktree_dir = state
                    .repo_root
                    .join(".maestro")
                    .join("runs")
                    .join(&run_id)
                    .join("worktree");
                let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
                let resolved_vars = resolve_run_variables(&pipeline, &events);

                let spawn_ctx = SpawnContext {
                    pipeline: &pipeline,
                    run_id: &run_id,
                    pipeline_path: &pipeline_path,
                    worktree_dir: &worktree_dir,
                    artifacts_dir: &artifacts_dir,
                    resolved_vars: &resolved_vars,
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

            let worktree_dir = state
                .repo_root
                .join(".maestro")
                .join("runs")
                .join(&run_id)
                .join("worktree");
            let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
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
        "cleanup_run" => cleanup_run(&state, &run_id).await,
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

    let pipeline_path = {
        let run_scoped = run_scoped_pipeline_path(&state.repo_root, run_id);
        if run_scoped.exists() {
            run_scoped
        } else {
            resolve_pipeline_path(&state.repo_root, &run_state.pipeline_name)
        }
    };
    let Ok(yaml) = std::fs::read_to_string(&pipeline_path) else {
        return;
    };
    let Ok(parse_result) = pipeline::parse_pipeline(&yaml) else {
        return;
    };

    let pipeline = parse_result.pipeline;
    let worktree_dir = state
        .repo_root
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("worktree");
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
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
    };

    for completed_node_id in &completed_node_ids {
        let source_iter = run_state
            .nodes
            .get(completed_node_id)
            .map(|n| n.iter)
            .unwrap_or(1);

        let frontmatter_fields =
            resolve_source_frontmatter(&pipeline, completed_node_id, source_iter, &artifacts_dir);

        let actions = scheduler::evaluate_outgoing_edges_with_context(
            &pipeline,
            &run_state,
            completed_node_id,
            &resolved_vars,
            &frontmatter_fields,
        );

        for action in &actions {
            match action {
                scheduler::SchedulerAction::Spawn { node_id, iter } => {
                    // Only spawn if not already running/completed at this iter
                    let already_active = run_state.nodes.get(node_id.as_str()).is_some_and(|n| {
                        n.iter >= *iter
                            && (n.status == event_log::NodeStatus::Running
                                || n.status == event_log::NodeStatus::Completed)
                    });
                    if !already_active {
                        if let Some(node) = pipeline.nodes.iter().find(|n| n.id == *node_id) {
                            spawn_node(state, &spawn_ctx, node, *iter).await;
                        }
                    }
                }
                scheduler::SchedulerAction::Halt { message } => {
                    let halt_event = event_log::Event {
                        id: None,
                        run_id: run_id.to_string(),
                        ts: event_log::now_iso(),
                        kind: event_log::EventKind::RunHalted,
                        node_id: None,
                        iter: None,
                        payload: Some(serde_json::json!({ "message": message })),
                    };
                    if let Err(e) = append_event(state, &halt_event).await {
                        error!("failed to append run_halted: {e}");
                    }
                    return;
                }
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

async fn session_attach(AxumPath(session_id): AxumPath<String>) -> Response {
    let terminal = detect_terminal();

    match spawn_terminal_attach(&terminal, &session_id) {
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

async fn manager_attach(AxumPath(run_id): AxumPath<String>) -> Response {
    let session_name = tmux_session_manager::manager_session_name(&run_id);

    if !tmux_session_manager::session_exists(&session_name) {
        return (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": format!("manager session {session_name} not found") }),
            ),
        )
            .into_response();
    }

    let terminal = detect_terminal();
    match spawn_terminal_attach(&terminal, &session_name) {
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
    // MAESTRO_TERMINAL env var overrides
    if let Ok(t) = std::env::var("MAESTRO_TERMINAL") {
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

fn spawn_terminal_attach(terminal: &str, session_name: &str) -> Result<()> {
    let parts: Vec<&str> = terminal.split_whitespace().collect();
    let (cmd, prefix_args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty terminal command"))?;

    let escaped_name = shell_escape(session_name);
    let tmux_cmd = format!("tmux attach -t {escaped_name}");

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
            let script = format!("#!/bin/bash\ntmux attach -t {escaped_name}\n");
            let script_path =
                std::env::temp_dir().join(format!("maestro-attach-{session_name}.sh"));
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

    for node in run_state.nodes.values() {
        let session_name =
            tmux_session_manager::node_session_name(run_id, &node.node_id, node.iter);
        tmux_session_manager::kill(&session_name);
    }
    let mgr_session = tmux_session_manager::manager_session_name(run_id);
    tmux_session_manager::kill(&mgr_session);

    let run_dir = state.repo_root.join(".maestro").join("runs").join(run_id);

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
                        .current_dir(&state.repo_root)
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
            .current_dir(&state.repo_root)
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
    for pattern in [
        format!("maestro/run-{run_id}"),
        format!("maestro/sub-{run_id}*"),
    ] {
        let branch_output = std::process::Command::new("git")
            .args(["branch", "--list", &pattern])
            .current_dir(&state.repo_root)
            .output();
        if let Ok(o) = branch_output {
            let branches = String::from_utf8_lossy(&o.stdout);
            for branch in branches.lines() {
                let branch = branch.trim().trim_start_matches("* ");
                if !branch.is_empty() {
                    let _ = std::process::Command::new("git")
                        .args(["branch", "-D", branch])
                        .current_dir(&state.repo_root)
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

// --- Pipeline path resolution ---

fn run_scoped_pipeline_path(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("pipeline.yaml")
}

fn run_scoped_prompts_dir(repo_root: &std::path::Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".maestro")
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
    if src_prompts.is_dir() {
        let dest_prompts = run_scoped_prompts_dir(repo_root, run_id);
        std::fs::create_dir_all(&dest_prompts).context("create prompts dir")?;
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
        .join(".maestro")
        .join("pipelines")
        .join(format!("{pipeline_name}.yaml"));
    if repo_path.exists() {
        return repo_path;
    }

    if let Some(home) = dirs_next_home() {
        let user_path = home
            .join(".maestro")
            .join("pipelines")
            .join(format!("{pipeline_name}.yaml"));
        if user_path.exists() {
            return user_path;
        }
    }

    repo_path
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

fn check_output_validation(
    pipeline_path: &std::path::Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &std::path::Path,
) -> Option<Response> {
    let yaml = std::fs::read_to_string(pipeline_path).ok()?;
    let parse_result = pipeline::parse_pipeline(&yaml).ok()?;
    let Err(missing) =
        outputs_validator::validate(&parse_result.pipeline, node_id, iter, artifacts_dir)
    else {
        return None;
    };
    Some(
        (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "missing_outputs",
                "missing": missing,
            })),
        )
            .into_response(),
    )
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
        .join(".maestro")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("iter-{iter}"))
}

fn sub_worktree_branch(run_id: &str, node_id: &str, iter: i64) -> String {
    format!("maestro/sub-{run_id}-{node_id}-iter-{iter}")
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
) -> Result<()> {
    std::fs::create_dir_all(worktree_dir.parent().unwrap_or(std::path::Path::new(".")))?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", branch_name])
        .arg(worktree_dir)
        .arg("HEAD")
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
<head><title>Maestro (dev)</title></head>
<body style="background:#0f1115;color:#e6e8eb;font-family:sans-serif;display:grid;place-items:center;height:100vh;margin:0">
<div style="text-align:center">
<h1>Maestro daemon running</h1>
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
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
        // Save and set MAESTRO_TERMINAL
        let prev = std::env::var("MAESTRO_TERMINAL").ok();
        std::env::set_var("MAESTRO_TERMINAL", "my-custom-terminal");
        let result = detect_terminal();
        // Restore
        match prev {
            Some(v) => std::env::set_var("MAESTRO_TERMINAL", v),
            None => std::env::remove_var("MAESTRO_TERMINAL"),
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
            PathBuf::from("/repo/.maestro/runs/20260101-120000-abc/nodes/impl-1/iter-1")
        );
    }

    #[test]
    fn sub_worktree_branch_name() {
        let branch = sub_worktree_branch("20260101-120000-abc", "impl-1", 1);
        assert_eq!(branch, "maestro/sub-20260101-120000-abc-impl-1-iter-1");
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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

        assert!(!worktree_has_tracked_changes(&wt_dir).unwrap());
    }

    #[test]
    fn doc_only_dirty_worktree_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "test-do-dirty";
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

        // Add an untracked file (like artifacts)
        let artifacts_dir = wt_dir.join(".maestro/artifacts/planner/iter-1");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        std::fs::write(artifacts_dir.join("plan.md"), "# plan\n").unwrap();

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
            recent_writes: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    const START_END_YAML: &str = "  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

    fn write_test_pipeline(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".maestro").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    #[tokio::test]
    async fn list_pipelines_scans_repo_dir() {
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
    async fn list_pipelines_empty_when_no_dir() {
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
        let pipelines_dir = tmp.path().join(".maestro").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let fixture = include_str!("../../../.maestro/pipelines/review-loop.yaml");
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
            .join(".maestro")
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
            .join(".maestro")
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
            .join(".maestro/pipelines/prompt-save.prompts/ab12cd34.md");
        assert!(prompt_path.exists(), "canonical prompt file must exist");
        let content = std::fs::read_to_string(&prompt_path).unwrap();
        assert_eq!(content, "You are a worker agent.");
    }

    #[tokio::test]
    async fn get_pipeline_reads_prompts_from_canonical_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "prompt-read");

        let prompts_dir = tmp.path().join(".maestro/pipelines/prompt-read.prompts");
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
        let dir = tmp.path().join(".maestro").join("pipelines");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.md"), "not a pipeline").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a pipeline").unwrap();
        write_test_pipeline(tmp.path(), "real-pipe");

        let entries = scan_pipeline_dir(&dir, "repo");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "real-pipe");
    }

    #[test]
    fn cli_parses_daemon_subcommand() {
        let cli = Cli::try_parse_from(["maestro", "daemon"]).unwrap();
        match cli.command {
            Commands::Daemon { port } => assert_eq!(port, DEFAULT_PORT),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_port() {
        let cli = Cli::try_parse_from(["maestro", "daemon", "--port", "9999"]).unwrap();
        match cli.command {
            Commands::Daemon { port } => assert_eq!(port, 9999),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_complete_subcommand() {
        let cli = Cli::try_parse_from(["maestro", "complete"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete));
    }

    #[test]
    fn cli_parses_fail_subcommand() {
        let cli = Cli::try_parse_from(["maestro", "fail", "--reason", "timeout"]).unwrap();
        match cli.command {
            Commands::Fail { reason } => assert_eq!(reason, "timeout"),
            _ => panic!("expected Fail subcommand"),
        }
    }

    #[test]
    fn cli_fail_requires_reason() {
        assert!(Cli::try_parse_from(["maestro", "fail"]).is_err());
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["maestro", "--version"]);
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
        let wt_dir = tmp
            .path()
            .join(".maestro/runs")
            .join(run_id)
            .join("worktree");
        let prompt_dir = wt_dir.join(".maestro").join("prompts");
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
        let wt_dir = tmp
            .path()
            .join(".maestro/runs")
            .join(run_id)
            .join("worktree");
        let prompt_dir = wt_dir.join(".maestro").join("prompts");
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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

        let sub_wt_dir = sub_worktree_path(repo, run_id, "impl-1", 1);
        let sub_branch = sub_worktree_branch(run_id, "impl-1", 1);
        create_sub_worktree(repo, &sub_wt_dir, &sub_branch, &pipeline_branch).unwrap();

        // Write prompt file in the sub-worktree (as the daemon does at spawn)
        let prompt_dir = sub_wt_dir.join(".maestro").join("prompts");
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
        let run_dir = dir.join(".maestro/runs").join(run_id);
        let pipeline_path = run_dir.join("pipeline.yaml");
        std::fs::create_dir_all(run_dir.join("worktree/.maestro/artifacts/planner/iter-1"))
            .unwrap();
        std::fs::create_dir_all(run_dir.join("worktree/.maestro/artifacts/implementer/iter-1"))
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
            run_dir.join("worktree/.maestro/artifacts/planner/iter-1/plan.md"),
            "# Plan\nDo stuff.",
        )
        .unwrap();
        std::fs::write(
            run_dir.join("worktree/.maestro/artifacts/implementer/iter-1/summary.md"),
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
                        "/runs/{run_id}/artifact?path=planner/iter-1/plan.md"
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
        let pipelines_dir = dir.join(".maestro").join("pipelines");
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
    async fn mark_node_done_accepts_failed_node_with_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let pipe_name = "failed-rescue";
        write_pipeline_with_outputs(tmp.path(), pipe_name);

        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "failed-rescue-1";
        seed_failed_run(&state, run_id, pipe_name).await;

        // Create the required output files
        let artifacts_dir = tmp
            .path()
            .join(".maestro/runs")
            .join(run_id)
            .join("worktree/.maestro/artifacts/worker/iter-1");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        std::fs::write(artifacts_dir.join("summary.md"), "# Summary\nDone.").unwrap();
        std::fs::write(artifacts_dir.join("report.md"), "# Report\nAll good.").unwrap();

        let app = build_router(state.clone());
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

        assert_eq!(resp.status(), StatusCode::OK);

        let events = load_events(&state.db, run_id).await.unwrap();
        let run_state = event_log::project(&events).unwrap();
        assert_eq!(
            run_state.nodes["worker"].status,
            event_log::NodeStatus::Completed
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
            .join(".maestro")
            .join("runs")
            .join(run_id)
            .join("worktree")
            .join(".maestro")
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
                    frontmatter: None,
                    when: None,
                }],
                outputs: vec![Port {
                    name: "pass".into(),
                    repeated: false,
                    side: None,
                    frontmatter: None,
                    when: Some(serde_yaml::from_str("iter: { lt: \"$max_iter_review\" }").unwrap()),
                }],
                interactive: false,
                view: None,
                max_iter: None,
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
            }],
            auto_merge_resolver: true,
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
                    frontmatter: None,
                    when: None,
                }],
                outputs: vec![],
                interactive: false,
                view: None,
                max_iter: None,
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
            }],
            auto_merge_resolver: true,
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
        let wt_dir = repo.join(".maestro/runs").join(run_id).join("worktree");
        let pipeline_branch = format!("maestro/run-{run_id}");
        create_worktree(repo, &wt_dir, &pipeline_branch).unwrap();

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
}
