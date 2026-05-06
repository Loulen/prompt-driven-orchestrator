mod event_log;
mod pipeline;
mod prompt_augmenter;

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
use clap::Parser;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time;
use tracing::{error, info, warn};

const DEFAULT_PORT: u16 = 5172;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Embed)]
#[folder = "../../frontend/dist"]
struct FrontendAssets;

#[derive(Parser)]
#[command(
    name = "maestro-daemon",
    about = "Maestro daemon — pipeline orchestrator"
)]
struct Cli {
    #[arg(short, long, env = "MAESTRO_PORT", default_value_t = DEFAULT_PORT)]
    port: u16,
}

struct AppState {
    db: sqlx::SqlitePool,
    event_tx: broadcast::Sender<event_log::Event>,
    repo_root: PathBuf,
    port: u16,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "maestro_daemon=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let addr = SocketAddr::from(([127, 0, 0, 1], cli.port));

    let repo_root = std::env::current_dir().context("failed to determine current directory")?;

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

    let state = Arc::new(AppState {
        db,
        event_tx,
        repo_root,
        port: cli.port,
    });

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind")?;

    info!("Maestro daemon listening on http://{addr}");
    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/runs", post(create_run))
        .route("/runs", get(list_runs))
        .route("/runs/{run_id}", get(get_run))
        .route("/runs/{run_id}/events", get(get_run_events))
        .route("/runs/{run_id}/nodes/{node_id}/done", post(node_done))
        .route("/runs/{run_id}/nodes/{node_id}/fail", post(node_fail))
        .route("/runs/{run_id}/commands", post(run_command))
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

// --- API handlers ---

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

    let run_started = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({
            "pipeline_name": pipeline.name,
            "input": req.input,
        })),
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
    let mut resolved_vars = pipeline.variables.clone();
    for (k, v) in &req.variables {
        resolved_vars.insert(k.clone(), v.clone());
    }

    // Spawn nodes that are ready (for single-node pipeline, that's the first node)
    for node in &pipeline.nodes {
        let prompt_dir = pipeline_path.parent().unwrap_or(std::path::Path::new("."));
        let role_prompt = node
            .prompt_file
            .as_ref()
            .and_then(|pf| pipeline::load_prompt_file(prompt_dir, pf).ok())
            .unwrap_or_default();

        let ctx = prompt_augmenter::AugmentContext {
            pipeline: &pipeline,
            node,
            run_id: &run_id,
            iter: 1,
            artifacts_dir: &artifacts_dir,
            variables: &resolved_vars,
            daemon_url: &format!("http://localhost:{}", state.port),
        };

        let full_prompt = prompt_augmenter::build_full_prompt(&ctx, &role_prompt);

        // Append node_started event
        let node_started = event_log::Event {
            id: None,
            run_id: run_id.clone(),
            ts: event_log::now_iso(),
            kind: event_log::EventKind::NodeStarted,
            node_id: Some(node.id.clone()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
            })),
        };
        if let Err(e) = append_event(&state, &node_started).await {
            error!("failed to append node_started: {e}");
        }

        // Spawn tmux session
        let session_name = format!("maestro-{run_id}-{}-iter-1", node.id);
        if let Err(e) = spawn_tmux_session(
            &session_name,
            &full_prompt,
            &worktree_dir,
            &run_id,
            &node.id,
            1,
            state.port,
        ) {
            error!("failed to spawn tmux session: {e}");
        }
    }

    info!("Run {run_id} started for pipeline {}", pipeline.name);

    (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response()
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

async fn node_done(
    State(state): State<Arc<AppState>>,
    AxumPath((run_id, node_id)): AxumPath<(String, String)>,
    body: Option<Json<NodeDoneRequest>>,
) -> Response {
    let iter = body.and_then(|b| b.iter).unwrap_or(1);

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

    // Kill the tmux session
    let session_name = format!("maestro-{run_id}-{node_id}-iter-{iter}");
    kill_tmux_session(&session_name);

    // Check if all nodes are completed → emit run_completed
    let events = match load_events(&state.db, &run_id).await {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    if let Some(run_state) = event_log::project(&events) {
        let all_done = !run_state.nodes.is_empty()
            && run_state
                .nodes
                .values()
                .all(|n| n.status == event_log::NodeStatus::Completed);

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

#[derive(Deserialize)]
struct RunCommandRequest {
    kind: String,
    #[allow(dead_code)]
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

async fn run_command(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<RunCommandRequest>,
) -> Response {
    match req.kind.as_str() {
        "cleanup_run" => cleanup_run(&state, &run_id).await,
        other => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown command: {other}") })),
        )
            .into_response(),
    }
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

    // Kill tmux sessions for all nodes + manager
    for node in run_state.nodes.values() {
        let session_name = format!("maestro-{run_id}-{}-iter-{}", node.node_id, node.iter);
        kill_tmux_session(&session_name);
    }
    let mgr_session = format!("maestro-mgr-{run_id}");
    kill_tmux_session(&mgr_session);

    // Remove git worktrees and run directory
    let run_dir = state.repo_root.join(".maestro").join("runs").join(run_id);
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

    // Delete the branch
    let branch_name = format!("maestro/run-{run_id}");
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", &branch_name])
        .current_dir(&state.repo_root)
        .output();

    // Remove the entire run directory (artifacts, prompts, etc.)
    if run_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&run_dir) {
            warn!("failed to remove run dir {}: {e}", run_dir.display());
        }
    }

    // Append run_archived event (event log is NOT touched otherwise)
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
        Arc::new(AppState {
            db,
            event_tx,
            repo_root: std::env::current_dir().unwrap(),
            port: 0,
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
}
