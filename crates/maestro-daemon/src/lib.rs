mod blackboard;
mod condition;
mod event_log;
mod frontmatter_parser;
mod pipeline;
mod pipeline_watcher;
mod prompt_augmenter;
mod scheduler;
mod variable_resolver;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Json, Path as AxumPath, State, WebSocketUpgrade};
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

    let watcher = pipeline_watcher::spawn_watcher(repo_root.clone(), pipeline_tx.clone());

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
    });

    let app = build_router(state);

    info!("Maestro daemon listening on http://{bound_addr}");

    let task = tokio::spawn(async move {
        let _watcher = watcher; // keep the file watcher alive for the server's lifetime
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
        .route("/runs/{run_id}/commands", post(run_command))
        .route("/sessions/{session_id}/attach", post(session_attach))
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

    let prompts_dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut prompts: HashMap<String, String> = HashMap::new();
    for node in &parse_result.pipeline.nodes {
        if let Some(ref pf) = node.prompt_file {
            if let Ok(content) = pipeline::load_prompt_file(prompts_dir, pf) {
                prompts.insert(node.id.clone(), content);
            }
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

    if let Err(e) = std::fs::write(&path, &req.yaml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {e}") })),
        )
            .into_response();
    }

    let prompts_dir = path.parent().unwrap_or(std::path::Path::new("."));
    for (node_id, content) in &req.prompts {
        let prompt_path = prompts_dir.join(format!("{pipeline_id}.prompts/{node_id}.md"));
        if let Some(parent) = prompt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
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
        "name: {safe_name}\nversion: \"1.0\"\n\nvariables: {{}}\n\nnodes: []\n\nedges: []\n"
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
    let prompt_dir = spawn_ctx
        .pipeline_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let role_prompt = node
        .prompt_file
        .as_ref()
        .and_then(|pf| pipeline::load_prompt_file(prompt_dir, pf).ok())
        .unwrap_or_default();

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
            },
        })),
    };
    if let Err(e) = append_event(state, &node_started).await {
        error!("failed to append node_started: {e}");
    }

    let session_name = format!("maestro-{run_id}-{}-iter-{iter}", node.id);
    if let Err(e) = spawn_tmux_session(
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
    let pipeline_path = resolve_pipeline_path(&state.repo_root, &run_state.pipeline_name);
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

    let edge_infos: Vec<event_log::EdgeInfo> = pipeline
        .edges
        .iter()
        .map(|e| {
            let (target_node, target_port, halt_message) = match &e.target {
                pipeline::EdgeTarget::Node(ep) => {
                    (ep.node.clone(), ep.port.clone(), None::<String>)
                }
                pipeline::EdgeTarget::Halt(h) => {
                    ("__halt__".into(), String::new(), h.message.clone())
                }
            };
            let when_json = e.when.as_ref().and_then(|w| serde_json::to_value(w).ok());
            event_log::EdgeInfo {
                source_node: e.source.node.clone(),
                source_port: e.source.port.clone(),
                target_node,
                target_port,
                halt_message,
                when_clause: when_json,
            }
        })
        .collect();

    let node_def_infos: Vec<event_log::NodeDefInfo> = pipeline
        .nodes
        .iter()
        .map(|n| event_log::NodeDefInfo {
            id: n.id.clone(),
            node_type: match n.node_type {
                pipeline::NodeType::DocOnly => "doc-only".into(),
                pipeline::NodeType::CodeMutating => "code-mutating".into(),
            },
            view_x: n.view.as_ref().map(|v| v.x),
            view_y: n.view.as_ref().map(|v| v.y),
            inputs: n.inputs.iter().map(|p| p.name.clone()).collect(),
            outputs: n.outputs.iter().map(|p| p.name.clone()).collect(),
        })
        .collect();

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

    // Write _input.md
    let artifacts_dir = worktree_dir.join(".maestro").join("artifacts");
    if let Err(e) = std::fs::create_dir_all(&artifacts_dir) {
        error!("failed to create artifacts dir: {e}");
    }
    let input_path = artifacts_dir.join("_input.md");
    if let Err(e) = std::fs::write(&input_path, &req.input) {
        error!("failed to write _input.md: {e}");
    }

    // Resolve variables (pipeline defaults + overrides)
    let mut resolved_vars = pipeline.variable_defaults();
    for (k, v) in &req.variables {
        resolved_vars.insert(k.clone(), v.clone());
    }

    let run_state = event_log::RunState::new(run_id.clone(), pipeline.name.clone());
    let ready = scheduler::ready_nodes(&pipeline, &run_state);

    let spawn_ctx = SpawnContext {
        pipeline: &pipeline,
        run_id: &run_id,
        pipeline_path: &pipeline_path,
        worktree_dir: &worktree_dir,
        artifacts_dir: &artifacts_dir,
        resolved_vars: &resolved_vars,
    };

    for node_id in &ready {
        let node = match pipeline.nodes.iter().find(|n| &n.id == node_id) {
            Some(n) => n,
            None => continue,
        };

        spawn_node(&state, &spawn_ctx, node, 1).await;
    }

    info!("Run {run_id} started for pipeline {}", pipeline.name);

    (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response()
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
        Some(run_state) => Json(run_state).into_response(),
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

fn find_node_type<'a>(run_state: &'a event_log::RunState, node_id: &str) -> Option<&'a str> {
    run_state
        .node_defs
        .iter()
        .find(|nd| nd.id == node_id)
        .map(|nd| nd.node_type.as_str())
}

async fn node_done(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    body: Option<Json<NodeDoneRequest>>,
) -> Response {
    let iter = body.and_then(|b| b.iter).unwrap_or(1);

    let session_name = format!("maestro-{run_id}-{node_id}-iter-{iter}");
    kill_tmux_session(&session_name);

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

    match find_node_type(&pre_run_state, &node_id) {
        Some("code-mutating") => {
            let sub_wt_dir = sub_worktree_path(&state.repo_root, &run_id, &node_id, iter);
            let sub_branch = sub_worktree_branch(&run_id, &node_id, iter);

            let _lock = state.merge_lock.lock().await;
            match commit_and_merge_sub_worktree(
                &state.repo_root,
                &sub_wt_dir,
                &worktree_dir,
                &sub_branch,
                &node_id,
                iter,
            ) {
                Ok(MergeResult::Success) => {}
                Ok(MergeResult::Conflict(detail)) => {
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
                Err(e) => {
                    error!("failed to commit/merge sub-worktree for {node_id}: {e}");
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

        // Re-load events after handle_node_completion may have appended halt/spawn events
        let events = match load_events(&state.db, &run_id).await {
            Ok(e) => e,
            Err(e) => {
                error!("failed to reload events: {e}");
                return (StatusCode::OK, "ok").into_response();
            }
        };

        if let Some(run_state) = event_log::project(&events) {
            // Don't try to complete a halted run
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

    // Kill the tmux session
    let session_name = format!("maestro-{run_id}-{node_id}-iter-{iter}");
    kill_tmux_session(&session_name);

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
            if let Some(run_state) = event_log::project(&events) {
                if let Some(node) = run_state.nodes.get(&node_id) {
                    if node.status != event_log::NodeStatus::AwaitingUser
                        && node.status != event_log::NodeStatus::Running
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

            let events = match load_events(&state.db, &run_id).await {
                Ok(e) => e,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}"))
                        .into_response();
                }
            };

            if let Some(run_state) = event_log::project(&events) {
                let all_done = !run_state.nodes.is_empty()
                    && run_state
                        .nodes
                        .values()
                        .all(|n| n.status == event_log::NodeStatus::Completed);

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

            info!("mark_node_done: node {node_id} in run {run_id}");
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
        let session_name = format!("maestro-{run_id}-{}-iter-{}", node.node_id, node.iter);
        kill_tmux_session(&session_name);
    }
    let mgr_session = format!("maestro-mgr-{run_id}");
    kill_tmux_session(&mgr_session);

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
}

fn commit_and_merge_sub_worktree(
    repo_root: &std::path::Path,
    sub_worktree_dir: &std::path::Path,
    pipeline_worktree_dir: &std::path::Path,
    sub_branch: &str,
    node_id: &str,
    iter: i64,
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
        let _ = std::process::Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(pipeline_worktree_dir)
            .output();
        return Ok(MergeResult::Conflict(stderr.to_string()));
    }

    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(sub_worktree_dir)
        .current_dir(repo_root)
        .output();

    let _ = std::process::Command::new("git")
        .args(["branch", "-d", sub_branch])
        .current_dir(repo_root)
        .output();

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

// --- tmux ---

fn spawn_tmux_session(
    session_name: &str,
    prompt: &str,
    working_dir: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    daemon_port: u16,
) -> Result<()> {
    // Write prompt to a temp file the agent can read
    let prompt_dir = working_dir.join(".maestro").join("prompts");
    std::fs::create_dir_all(&prompt_dir)?;
    let prompt_path = prompt_dir.join(format!("{node_id}-iter-{iter}.md"));
    std::fs::write(&prompt_path, prompt)?;

    // Build the command that runs inside tmux
    let claude_cmd = format!(
        "export MAESTRO_RUN_ID='{run_id}' && \
         export MAESTRO_NODE_ID='{node_id}' && \
         export MAESTRO_NODE_ITER='{iter}' && \
         export MAESTRO_DAEMON_URL='http://localhost:{daemon_port}' && \
         echo 'Maestro NodeRun: {node_id} iter {iter}' && \
         echo 'Prompt file: {}' && \
         echo '---' && \
         cat '{}' && \
         echo '---' && \
         echo 'Run: maestro complete when done, maestro fail --reason \"...\" on failure'",
        prompt_path.display(),
        prompt_path.display(),
    );

    let output = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", session_name, "-c"])
        .arg(working_dir)
        .arg(&claude_cmd)
        .output()
        .context("failed to run tmux new-session")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux new-session failed: {stderr}");
    }

    info!("Spawned tmux session: {session_name}");
    Ok(())
}

fn kill_tmux_session(session_name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output();
}

pub fn capture_tmux_pane(session_name: &str) -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(["capture-pane", "-pe", "-S", "-100", "-t", session_name])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

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
            commit_and_merge_sub_worktree(repo, &sub_wt_dir, &wt_dir, &sub_branch, "impl-1", 1)
                .unwrap();
        assert!(matches!(result, MergeResult::Success));

        // Verify the file is present in the pipeline worktree
        assert!(wt_dir.join("foo.rs").exists());
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
            commit_and_merge_sub_worktree(repo, &sub_wt_1, &wt_dir, &sub_branch_1, "impl-1", 1)
                .unwrap();
        assert!(matches!(r1, MergeResult::Success));

        // Merge second → conflict
        let r2 =
            commit_and_merge_sub_worktree(repo, &sub_wt_2, &wt_dir, &sub_branch_2, "impl-2", 1)
                .unwrap();
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
        })
    }

    fn write_test_pipeline(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".maestro").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n  - id: worker\n    type: doc-only\n    prompt_file: prompts/worker.md\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
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
        assert_eq!(detail["pipeline"]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(detail["pipeline"]["nodes"][0]["id"], "worker");
        assert!(detail["yaml"].as_str().unwrap().contains("name: my-pipe"));
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

        let new_yaml = "name: editable\nversion: \"2.0\"\nnodes: []\nedges: []\n";
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
        assert_eq!(entry["node_count"], 1);
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
}
