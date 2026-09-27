// Don't drop this lint (ADR-0039): the daemon is one crate of sibling modules, so
// a `pub` item with no consumer outside its file is a leak, and only this warning
// (with `clippy -D warnings`) keeps the crate surface from re-widening.
#![warn(unreachable_pub)]

mod agent_profile;
mod contexts;
#[allow(dead_code)]
mod cron_schedule;
mod distribution;
#[allow(dead_code)]
mod fire_decision;
mod guard_runner;
mod instance_config;
mod pipeline_ports;
mod price_table;
mod review_comments;
mod run_cost;
mod run_ports;
mod run_refs;
mod sandbox_ports;
mod session_ports;
mod stats;
mod stats_absorption;
mod stats_performance;
mod structured_diff;
#[allow(dead_code)]
mod trigger_scheduler;
#[allow(dead_code)]
mod trigger_store;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{FromRequest, Json, Multipart, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use clap::{Parser, Subcommand};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time;
use tracing::{error, info, warn};
use ts_rs::TS;

use crate::contexts::ContextResourcesSeed;
pub use crate::contexts::{
    ContextResources, Contexts, PipelineResources, RunResources, SessionRunPorts,
};
pub use pdo_platform::adapters::cli::{PageAction, ServiceAction};
use pdo_platform::adapters::http::OriginHint;
pub use pdo_platform::domain::service_health::ServiceHealthOverride;
use pdo_workspace::adapters::git::fetch::fetch_all_remotes;
use pdo_workspace::adapters::git::repository::{
    count_commits, last_fetch_at, ref_exists, upstream_of, validate_source_branch,
    validate_target_repo,
};

// The workspace context's modules the rest of this crate reads, until each
// context migrates onto the workspace's ports.
pub(crate) use pdo_workspace::adapters::git::worktrees;
#[cfg(test)]
use pdo_workspace::adapters::git::worktrees::create_sub_worktree;
use pdo_workspace::adapters::git::worktrees::{
    create_secondary_snapshot, create_worktree, remove_secondary_snapshot, rev_parse_verified,
    secondary_snapshot_path, worktree_dir_for_run,
};
pub(crate) use pdo_workspace::adapters::sqlite as workspace_db;
use pdo_workspace::domain::branches::FetchError;
pub(crate) use pdo_workspace::domain::provisioning;

// The skill context's rules the rest of this crate reads: the selection per
// tier (instance, Projet, Trigger, pipeline node, Run) and what a delivery
// reports. Every skill gesture goes through `AppState::skills`.
pub(crate) use pdo_skill::domain::delivery as skill_delivery;
pub(crate) use pdo_skill::domain::selection as skill_selection;

// The pipeline context's rules and adapters the rest of this crate reads: the
// document, its frontmatter, the registry's paths and the watcher. Every
// pipeline gesture of an actor goes through the context's own routes.
pub(crate) use pdo_pipeline::adapters::self_writes::SelfWrites;
pub(crate) use pdo_pipeline::adapters::watcher as pipeline_watcher;
pub(crate) use pdo_pipeline::domain::migration as pipeline_migrator;
pub(crate) use pdo_pipeline::domain::pipeline;

// The run context the rest of this crate reads (#923, #924): the graph a Run
// walks, what it allows, its verdicts, and the lifecycle the Run's surfaces still
// here (create, list, archive, review, the manager) call into. Every gesture on a
// node, the scheduler and the Pipeline Manager's commands live in `pdo-run`.
use pdo_run::adapters::cli::{cli_daemon_url, cli_run_id, session_env_claim};
pub(crate) use pdo_run::adapters::http::verdicts::repo_edit as repo_edit_refusal;
use pdo_run::application::boot_recovery;
use pdo_run::application::events::{
    append_event as append_run_event, load_projected as load_run_projected,
};
use pdo_run::application::events::{emit_run_event, reload_run_state_with, run_is_forgotten};
use pdo_run::application::node::project_tiers::project_skills_for_path;
pub(crate) use pdo_run::application::node::prompt as prompt_augmenter;
use pdo_run::application::node::prompt::ALLOWED_IMAGE_EXTENSIONS;
use pdo_run::application::paths::{
    archived_or_live_artifacts_dir, archived_or_live_pipeline_yaml, archived_or_live_prompts_dir,
};
use pdo_run::application::paths::{
    dirs_next_home, durable_run_dir, instance_pipeline_dir, resolve_pipeline_path,
    run_scoped_pipeline_path, run_scoped_prompts_dir,
};
pub(crate) use pdo_run::application::sandbox as run_sandbox;
use pdo_run::application::sandbox_prep::{
    abandon_prep_if_run_is_terminal, fail_run_sandbox_prep, mark_sandbox_prep_ready,
};
use pdo_run::application::scheduler::node_completion::spawn_ready_after_event;
use pdo_run::application::session_claim::verify_session_claim;
use pdo_run::application::session_claim::{session_claim_from_headers, SessionClaim};
pub(crate) use pdo_run::application::stale::detector as stale_detector;
use pdo_run::application::stale::sweep::run_stale_detection_supervised;
use pdo_run::application::supervision::run_isolated;
pub(crate) use pdo_run::domain::{admission, child_awaiting, reap_policy};

/// A Run's events, its state and their projection, from the run context
/// (#923), under the one name the rest of this crate reads them by. Every
/// event enters the log through `AppState::run` ([`append_event`]).
pub(crate) mod event_log {
    pub(crate) use pdo_run::application::run_id::generate_run_id;
    pub(crate) use pdo_run::domain::event::*;
    pub(crate) use pdo_run::domain::projection::*;
    pub(crate) use pdo_run::domain::state::*;
}
pub(crate) use pdo_pipeline::domain::portable as portable_pipeline;

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

async fn reload_run_state(
    state: &AppState,
    run_id: &str,
) -> Option<(Vec<event_log::Event>, event_log::RunState)> {
    pdo_run::application::events::reload_run_state(&state.run, run_id).await
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
// `Create` carries every creation flag, `Wait` two — a clap enum is parsed once
// per process, so the size skew buys nothing to box away (it is already boxed
// behind `Commands::Run`).
#[allow(clippy::large_enum_variant)]
pub enum RunAction {
    /// Create a Run — the thin CLI client of the daemon's `POST /runs`
    /// (issue #721, ADR-0064).
    ///
    /// Identity travels with the session, never in the body: run from a node
    /// session (the `PDO_RUN_ID` / `PDO_NODE_ID` env vars the session is
    /// wrapped with), the created Run is mechanically linked to that Run + node
    /// (`parent_run_id` / `parent_node_id`); run from a plain terminal, it is a
    /// root Run. The child's project defaults to the parent's — `--target-repo`
    /// overrides it explicitly. Daemon refusals (unknown pipeline, a refused
    /// session claim, …) print the daemon's own `error` sentence and exit
    /// non-zero.
    Create {
        /// The pipeline to run, resolved by the daemon against its pipeline stores.
        pipeline: String,
        /// Prompt for the Run's start node. A prompt-optional pipeline accepts
        /// the empty default.
        #[arg(short, long, default_value = "", conflicts_with = "input_file")]
        input: String,
        /// Read the prompt from a file instead of `--input` (#779) — e.g. the
        /// spec an upstream node wrote: `--input-file "$PDO_INPUT_SPEC"`.
        #[arg(long, value_name = "PATH")]
        input_file: Option<std::path::PathBuf>,
        /// Attach an image (#779, repeatable): sent in the multipart `images`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Images` in its entry node's preamble.
        #[arg(long, value_name = "PATH")]
        image: Vec<std::path::PathBuf>,
        /// Attach any file (#779, repeatable): sent in the multipart `files`
        /// field, written to the child's `_input/`, listed under
        /// `## Input Files`. Any extension; one size budget per run
        /// (`max_attachments_mb` in Settings); duplicate names are refused.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
        /// Run variables as a JSON object: `--variables '{"pool":"a"}'`.
        #[arg(long)]
        variables: Option<String>,
        /// Skills selected at the Run tier (#669, ADR-0062): comma-separated
        /// ids (`--skills a,b`) or a JSON list of `{id, name}` objects.
        #[arg(long)]
        skills: Option<String>,
        /// Agentic profile at the Run tier (ADR-0057) as JSON —
        /// `{"mode":"profile","profile_id":"…"}` or
        /// `{"mode":"custom","harness":"…","model":"…","effort":"…"}`.
        #[arg(long)]
        agent_choice: Option<String>,
        /// Agentic harness (ADR-0046): free text, validated at spawn.
        #[arg(long)]
        harness: Option<String>,
        /// Sandbox isolation: `off`, or the name of a staging profile.
        #[arg(long)]
        sandbox: Option<String>,
        /// Primary target repo (path or URL, like the UI's create modal).
        /// Overrides the parent Run's project — ADR-0033 keeps the boundary.
        #[arg(long)]
        target_repo: Option<String>,
        /// Secondary repos as a JSON list of `{repo, base_branch?, read_only?}`
        /// (ADR-0042).
        #[arg(long)]
        target_repos: Option<String>,
        /// Branch the primary repo starts from (default: its local HEAD).
        #[arg(long)]
        source_branch: Option<String>,
        /// Display name for the Run.
        #[arg(long)]
        name: Option<String>,
        /// Whether the manager may auto-name the Run (default: resolved by the
        /// daemon from the presence of `--name`).
        #[arg(long)]
        auto_name: Option<bool>,
        /// The Run's `auto_fail` preference (ADR-0049).
        #[arg(long)]
        auto_fail: Option<bool>,
        /// Run-level provisioning rules as JSON (ADR-0061).
        #[arg(long)]
        provisioning: Option<String>,
    },
    /// Block until a child run of this node becomes terminal (#588, ADR-0069):
    /// a long-poll on the daemon, never a push into your pane. Prints one JSON
    /// line per child that settled (`run_id`, `name`, `status`, `reason`,
    /// `children_active`) and exits 0; exits **2** with nothing on stdout when
    /// `--timeout` elapses (not a failure — call again); exits 1 when the daemon
    /// is unreachable or you are not in a node session. With no active child at
    /// the call, exits 0 with `{"noop":true,…}`. A child `awaiting_user` does
    /// not wake you: its wait shows on your node and run instead.
    Wait {
        /// Return only once EVERY child of this node is terminal.
        #[arg(long)]
        all: bool,
        /// Give up after this many seconds (exit 2). Without it, waits forever.
        #[arg(long)]
        timeout: Option<u64>,
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
    port: u16,
    /// Serializes Trigger scheduler ticks. A tick is read-decide-write
    /// (`due_triggers` → guard subprocess → `set_next_fire`) with an await in the
    /// middle; two concurrent ticks would both see the same Trigger as due and
    /// double-fire it.
    trigger_tick_lock: tokio::sync::Mutex<()>,
    /// Paths the daemon has just written. The pipeline watcher suppresses
    /// `pipeline_changed` broadcasts for paths seen within the TTL window — that
    /// is how our own writes are told apart from external ones (vim, git
    /// checkout) without ignoring the latter.
    recent_writes: SelfWrites,
    /// Live handle to the pipeline file watcher. Run dirs are watched
    /// individually and non-recursively (see `pipeline_watcher::watch_run_dir`),
    /// so run creation must register each new run dir here.
    run_watcher: Arc<Mutex<Option<pipeline_watcher::PipelineDebouncer>>>,
    /// Per-daemon override for the `claude …` tail of spawned tmux scripts.
    /// `None` in production; `Some(cmd)` in tests, seeded via [`DaemonConfig`] —
    /// never process-global env, which parallel test daemons would race on.
    tmux_cmd_override: Option<String>,
    /// Per-daemon override for the `docker` binary the sandbox wiring shells out
    /// to. Seeded via [`DaemonConfig`], never a process-global `std::env` read.
    docker_cmd_override: Option<String>,
    /// Per-daemon sandbox home-root override, so layer-3 staging lands under the
    /// test's tempdir. Seeded via [`DaemonConfig`].
    sandbox_home_override: Option<PathBuf>,
    /// Epoch-millis of the **start** of the most recent Trigger scheduler tick,
    /// or `0` if it has never ticked. Written at tick start so a wedged tick
    /// freezes the value; an isolated panic still advances it (the loop is
    /// alive). Surfaced by `GET /triggers/health`.
    last_trigger_tick_ms: Arc<std::sync::atomic::AtomicI64>,
    /// Debug-only fault injection: when a due Trigger's name matches, its tick
    /// `panic!`s, exercising the scheduler's panic isolation. Per-daemon, never
    /// process-global env.
    panic_on_trigger_name: Option<String>,
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
    /// Serializes price syncs. Unlike `trigger_tick_lock`, which serializes
    /// because both triggers must land, a second concurrent price sync brings
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

impl AppState {
    /// tmux, on this daemon's private socket (`-L pdo-<port>`): the session
    /// context's port. Every tmux call from this daemon goes through it, or
    /// parallel daemons collide.
    fn tmux(&self) -> &dyn pdo_session::application::ports::Tmux {
        self.session.tmux().as_ref()
    }
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
    /// Secondary repos to associate with this Run (ADR-0042). An omitted field
    /// means "mono-repo". When present, `target_repos[0]` is the **primary** and
    /// must agree with `target_repo`; `[1..]` are the secondaries. An absent list
    /// with a `target_repo` set is normalised to `[{target_repo}]` at the create
    /// chokepoint, so downstream always sees `[0]` = primary.
    #[serde(default)]
    target_repos: Vec<TargetRepoInput>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    name: Option<String>,
    /// Provenance: the id of the Trigger that created this Run, if any. Set by
    /// the trigger scheduler; absent for manual runs.
    #[serde(default)]
    triggered_by: Option<String>,
    /// Isolation mode for this Run. Don't collapse to `SandboxMode`: `None` must
    /// stay distinguishable from an explicit `"off"` (omitted defers to the
    /// trigger/instance default; explicit `off` may not be overridden upward) —
    /// see [`pdo_sandbox::domain::mode::effective_sandbox`]. `create_run_inner` resolves it once
    /// against the fresh instance default.
    #[serde(default)]
    sandbox: Option<event_log::SandboxMode>,
    /// The agentic harness this Run chooses (ADR-0046) — the `run` tier of
    /// `nœud → Run → Projet → instance → plancher (claude)`. Free text
    /// (ADR-0045: PDO never validates a harness name; an unknown one fails fast
    /// at spawn, naming it). Omitted/`null`/blank means the Run names no harness.
    /// Frozen into `RunStarted` at the create chokepoint; a pinned node ignores
    /// it. A Trigger fire folds its stored harness in here, so "Run now" and a
    /// cron tick produce the same one.
    #[serde(default)]
    harness: Option<String>,
    /// The Run tier of the agentic-profile union (ADR-0057) — the counterpart of
    /// [`Self::harness`], as the exclusive union (`Inherit`/`Profile`/`Custom`)
    /// atomically supplying harness, model and effort. `None`/`Inherit` ⇒ the Run
    /// states no choice, so `harness` above still decides. `Some(Profile |
    /// Custom)` wins outright at this tier and never merges with `harness`.
    #[serde(default)]
    agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Run tier of the **skills** selection (#669, ADR-0062): ids (+ label)
    /// added to every node of this Run, on top of the instance and Projet tiers.
    /// Frozen into `RunStarted` at the create chokepoint (only when non-empty). A
    /// Trigger fire folds its stored list in here, like `harness`.
    #[serde(default)]
    skills: Vec<crate::skill_selection::SkillRef>,
    /// Whether the manager auto-names this Run. Don't collapse to `bool`: `None`
    /// (omitted) must resolve by the presence of `name`, so a supplied `name` with
    /// no flag keeps the name; `Some(b)` is an explicit choice that wins.
    /// Resolved at the create chokepoint against the FRESH instance default — see
    /// [`prompt_augmenter::default_auto_name_with`].
    #[serde(default)]
    auto_name: Option<bool>,
    /// The Run's `auto_fail` preference (ADR-0049) — the `run` tier of `node →
    /// Run → Projet → instance`. `None` means the Run states no preference, so a
    /// node `pdo fail` resolves through the project / instance tiers. Frozen into
    /// `RunStarted` at the create chokepoint.
    #[serde(default)]
    auto_fail: Option<bool>,
    /// Run-level provisioning rules, composed and frozen with Instance + Project.
    #[serde(default)]
    provisioning: provisioning::ProvisioningRules,
    /// Why the **pre-cut fetch** of a Trigger fire did not happen (#804,
    /// ADR-0070 §1), frozen into `RunStarted` so the Run can say it started on
    /// local state instead of a refreshed one. `None` on every other path, and on
    /// a fire whose fetch worked.
    ///
    /// `#[serde(skip)]` — and therefore absent from [`CREATE_RUN_FIELDS`] — because
    /// the daemon decides this at the fire, never a caller. Gating it on
    /// `triggered_by` instead would be wrong twice over: that key IS accepted in a
    /// body, so any `POST /runs` could claim a fire, and a CLI-created child keeps
    /// its fetch-free cut by design (ADR-0070 §3).
    #[serde(skip)]
    source_fetch_error: Option<FetchError>,
}

/// The top-level keys `POST /runs` accepts, kept in lock-step with
/// [`CreateRunRequest`].
///
/// At this **write boundary** an unknown field is a client mistake we surface
/// (`400` naming it, before any effect), not a key silently dropped — a
/// `target_repos` typo used to launch the run mono-repo in silence. Deliberately
/// **narrower** than the crate-wide "ignore unknown fields" convention
/// (ADR-0015), which governs forward-compatible *config documents* read by an
/// older binary. Hence explicit validation rather than
/// `#[serde(deny_unknown_fields)]`, whose generic message is less actionable.
const CREATE_RUN_FIELDS: &[&str] = &[
    "pipeline",
    "input",
    "variables",
    "pipeline_id",
    "target_repo",
    "target_repos",
    "source_branch",
    "name",
    "triggered_by",
    "sandbox",
    "harness",
    "agent_choice",
    "skills",
    "auto_name",
    "auto_fail",
    "provisioning",
];

/// Reject a `POST /runs` body carrying an unknown top-level key. A non-object
/// body is left for the typed deserialize to reject — its `invalid JSON` message
/// is already actionable.
fn reject_unknown_create_run_fields(
    body: &serde_json::Value,
) -> Result<(), (StatusCode, serde_json::Value)> {
    let Some(obj) = body.as_object() else {
        return Ok(());
    };
    for key in obj.keys() {
        if !CREATE_RUN_FIELDS.contains(&key.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                serde_json::json!({
                    "error": format!(
                        "unknown field `{key}` in POST /runs body; accepted fields: {}",
                        CREATE_RUN_FIELDS.join(", ")
                    )
                }),
            ));
        }
    }
    Ok(())
}

/// One repo line of a multi-repo create request (ADR-0042/0047).
///
/// `base_branch` defaults to `HEAD`, the LOCAL ref — there is no fetch. Index
/// `[0]` is the primary; `[1..]` become secondary snapshots. Shared by create,
/// `PATCH /runs/{id}/repos` (`add`) and the trigger `target_repos` blob.
#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[ts(export, optional_fields = nullable)]
struct TargetRepoInput {
    repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    read_only: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Body of `PATCH /runs/{run_id}/repos` — the mid-run edit of a Run's secondary
/// list (ADR-0042).
///
/// A dedicated endpoint, not a `kind` of `POST /runs/{id}/commands`: a command's
/// payload is a bag of scalars frozen byte-for-byte, whereas a list edit needs
/// arrays. An empty body is a legal no-op `200`. The primary is never reachable
/// from either array — it has no alias, so `remove` cannot name it and an `add`
/// equal to it is refused (`secondary_is_primary`).
#[derive(Deserialize, Debug, Default)]
struct PatchRunReposRequest {
    #[serde(default)]
    add: Vec<TargetRepoInput>,
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct CreateRunResponse {
    run_id: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct RunListEntry {
    run_id: String,
    pipeline_name: String,
    status: event_log::RunStatus,
    /// Display-only "no forward progress" overlay: true when the run has no
    /// running/waiting node and a stale node, so its dot renders amber even
    /// though `status` stays `running`. Derived per read by `event_log::is_stalled`.
    stalled: bool,
    started_at: Option<String>,
    /// Why the Run ended non-green. Carried on the *list* entry so the red dot has
    /// something to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    failure_reason: Option<String>,
    /// Why the Run is parked `AwaitingUser` on an **incident** (ADR-0049), and its
    /// machine slug. On the list entry for the same reason as `failure_reason`.
    /// `None` for an interactive wait, which carries no incident reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    awaiting_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    awaiting_reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    name: Option<String>,
    /// Provenance: the Trigger that created this Run, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    triggered_by: Option<String>,
    /// Provenance: the parent Run + node whose session created this Run
    /// (ADR-0064) — the key the "orchestrated" badge and the "root runs only"
    /// filter read. `None` on a root run. Mechanical: never caller-declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    parent_node_id: Option<String>,
    /// Resolved target repo for "group by project": the run's `target_repo`, or
    /// the daemon's `repo_root` when unset. Only runs predating the hardened
    /// create boundary (ADR-0033) can be unset — this is a READ, so keep the
    /// fallback forever.
    effective_repo: String,
}

/// A Trigger as returned by `GET /triggers`, wrapping the raw stored Trigger with
/// a resolved `effective_repo` for the "group by project" view. Don't fold it into
/// `trigger.target_repo`: the raw value drives the row badge, the detail panel and
/// the Run-now pre-fill, while this one exists purely so the client can group by a
/// concrete path.
#[derive(Serialize, TS)]
#[ts(export)]
struct TriggerListEntry {
    #[serde(flatten)]
    trigger: trigger_store::Trigger,
    effective_repo: String,
}

/// Parse a JSON-typed flag. The error names the flag, so an agent reading the
/// stderr knows which argument was malformed.
fn parse_json_arg(flag: &str, raw: &str) -> Result<serde_json::Value> {
    serde_json::from_str(raw).with_context(|| format!("invalid JSON for --{flag}: {raw}"))
}

/// `--skills`: comma-separated ids (`a,b`) or a JSON list of `{id, name}`
/// objects. Ids become `{id}` objects — the wire shape of the Run tier's
/// `SkillRef` list (`name` defaults; #668: identity is the id).
fn parse_skills_arg(raw: &str) -> Result<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.starts_with('[') {
        return parse_json_arg("skills", trimmed);
    }
    let refs: Vec<serde_json::Value> = trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|id| serde_json::json!({ "id": id }))
        .collect();
    Ok(serde_json::json!(refs))
}

/// One-shot `pdo run create` (issue #721, ADR-0064): the thin CLI client of the
/// daemon's `POST /runs`. Like the other one-shots, blocking `reqwest` and no
/// tokio runtime (see `main.rs`); plain `Result` → `0`/`1` exit mapping — only
/// `pdo complete` owns an exit-code contract (#490).
///
/// The body is built field by field rather than by serialising a struct, so the
/// pass-through stays visibly in lock-step with the daemon's
/// `CREATE_RUN_FIELDS` — and provenance never enters it: identity travels in
/// the session headers, exactly like the UI's fetch calls.
pub fn run_run_create(action: RunAction) -> Result<()> {
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
    } = action
    else {
        anyhow::bail!("`pdo run wait` has its own exit-code contract — see `run_run_wait`");
    };

    let url = cli_daemon_url();

    // #779: `--input-file` reads the prompt from disk (clap already refuses it
    // together with `--input`). Read eagerly so a missing file fails before
    // the daemon is even reached.
    let input = match input_file {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read --input-file {}", path.display()))?,
        None => input,
    };

    let mut body = serde_json::Map::new();
    body.insert("pipeline".into(), serde_json::json!(pipeline));
    body.insert("input".into(), serde_json::json!(input));
    if let Some(v) = variables {
        body.insert("variables".into(), parse_json_arg("variables", &v)?);
    }

    if let Some(v) = skills {
        body.insert("skills".into(), parse_skills_arg(&v)?);
    }
    if let Some(v) = agent_choice {
        body.insert("agent_choice".into(), parse_json_arg("agent-choice", &v)?);
    }
    if let Some(v) = harness {
        // No validation here (ADR-0045) — the daemon refuses an unknown one at
        // spawn, naming it.
        body.insert("harness".into(), serde_json::json!(v));
    }
    if let Some(v) = sandbox {
        body.insert("sandbox".into(), serde_json::json!(v));
    }
    if let Some(v) = target_repo {
        body.insert("target_repo".into(), serde_json::json!(v));
    }
    if let Some(v) = target_repos {
        body.insert("target_repos".into(), parse_json_arg("target-repos", &v)?);
    }
    if let Some(v) = source_branch {
        body.insert("source_branch".into(), serde_json::json!(v));
    }
    if let Some(v) = name {
        body.insert("name".into(), serde_json::json!(v));
    }
    if let Some(v) = auto_name {
        body.insert("auto_name".into(), serde_json::json!(v));
    }
    if let Some(v) = auto_fail {
        body.insert("auto_fail".into(), serde_json::json!(v));
    }
    if let Some(v) = provisioning {
        body.insert("provisioning".into(), parse_json_arg("provisioning", &v)?);
    }

    let session = session_env_claim();
    // #779: any `--image` / `--file` switches the body to multipart — the same
    // text fields as the JSON shape (non-scalars as JSON strings, like the UI),
    // plus one `images` / `files` part per attachment. Files are read here so a
    // missing path is named locally, before any request.
    let (form, attachment_bytes) = if image.is_empty() && file.is_empty() {
        (None, 0)
    } else {
        let (form, bytes) = run_create_multipart(&body, &image, &file)?;
        (Some(form), bytes)
    };
    // #839: a bounded wait. Without it a stalled upload hangs the command
    // forever; with it a slow link still gets a budget that grows with the
    // payload, and the failure names what was being sent and to whom.
    let timeout = upload_timeout(attachment_bytes);
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .context("failed to build the HTTP client")?
        .post(format!("{url}/runs"));
    let mut req = match form {
        Some(form) => client.multipart(form),
        None => client.json(&body),
    };
    if let Some((run_id, node_id)) = &session {
        req = req
            .header(SESSION_RUN_HEADER, run_id)
            .header(SESSION_NODE_HEADER, node_id);
    }
    let resp = req
        .send()
        .map_err(|e| upload_transport_error(&url, attachment_bytes, timeout, &e))?;

    let status = resp.status();
    let raw = resp.text().unwrap_or_default();
    if !status.is_success() {
        // Readable: surface the daemon's own `error` sentence when there is one
        // (unknown pipeline, refused session claim, …). A body that is not the
        // daemon's JSON came from something in front of it (#839): say so, and
        // for a 413 name the proxy body limit rather than PDO's own budget.
        let parsed: Option<serde_json::Value> = serde_json::from_str(&raw).ok();
        let detail = match parsed
            .as_ref()
            .and_then(|p| p.get("error"))
            .and_then(|v| v.as_str())
        {
            Some(err) => err.to_string(),
            None => intermediary_refusal_detail(&url, status.as_u16(), attachment_bytes, &raw),
        };
        anyhow::bail!("daemon refused run creation ({status}): {detail}");
    }
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("unreadable success body from daemon: {raw}"))?;
    let run_id = parsed
        .get("run_id")
        .and_then(|v| v.as_str())
        .with_context(|| format!("`run_id` missing from daemon response: {raw}"))?
        .to_string();

    match session {
        Some((parent_run, parent_node)) => {
            println!("Run {run_id} created — child of run {parent_run} (node {parent_node}).")
        }
        None => println!("Run {run_id} created (root run)."),
    }
    Ok(())
}

/// The multipart body of a `pdo run create` carrying attachments (#779). Text
/// fields mirror the JSON body one-to-one: strings verbatim, everything else
/// (`variables`, `skills`, `auto_name`, …) serialised the way the daemon's
/// multipart parser reads them back — JSON text for objects/lists, `true`/
/// `false` for bools.
/// The bounded wait of a `POST /runs` from the CLI (#839): a minute, plus ten
/// seconds per MB of attachments — generous for a slow link, finite for a
/// stalled one.
fn upload_timeout(attachment_bytes: u64) -> Duration {
    let mb = attachment_bytes.div_ceil(1024 * 1024);
    Duration::from_secs(60 + 10 * mb)
}

/// The sentence for a `POST /runs` that never got an answer (#839): names the
/// host, what was being sent, whether the wait expired, and the two knobs a
/// remote instance usually trips on.
fn upload_transport_error(
    url: &str,
    attachment_bytes: u64,
    timeout: Duration,
    err: &reqwest::Error,
) -> anyhow::Error {
    if attachment_bytes == 0 {
        return anyhow::anyhow!("failed to reach daemon at {url}: {err}");
    }
    let sent = format_attachment_bytes(attachment_bytes);
    let what = if err.is_timeout() {
        format!(
            "upload timed out after {}s: the daemon at {url} did not answer while {sent} of \
             attachments were being sent",
            timeout.as_secs()
        )
    } else {
        format!(
            "upload interrupted: the daemon at {url} did not answer or the connection was cut \
             while sending {sent} of attachments ({err})"
        )
    };
    anyhow::anyhow!(
        "{what}. On a remote instance, check the reverse proxy body limit (nginx \
         `client_max_body_size`) and `max_attachments_mb` in Settings"
    )
}

/// The detail of a non-2xx answer whose body is NOT the daemon's JSON (#839):
/// it came from a reverse proxy or another intermediary, and a 413 there is
/// that layer's body limit, not `max_attachments_mb`.
fn intermediary_refusal_detail(url: &str, status: u16, attachment_bytes: u64, raw: &str) -> String {
    let sent = format_attachment_bytes(attachment_bytes);
    if status == 413 {
        return format!(
            "the server in front of PDO at {url} refused the request body ({sent} of attachments) \
             before it reached the daemon — no PDO error body, so this is the reverse proxy's \
             limit (nginx `client_max_body_size`), not `max_attachments_mb`. Raise it or attach less"
        );
    }
    let excerpt: String = raw.trim().chars().take(200).collect();
    let excerpt = if excerpt.is_empty() {
        "empty body".to_string()
    } else {
        excerpt
    };
    format!(
        "no PDO error body, the reply came from a proxy or another intermediary in front of the \
         daemon at {url}: {excerpt}"
    )
}

fn run_create_multipart(
    body: &serde_json::Map<String, serde_json::Value>,
    images: &[std::path::PathBuf],
    files: &[std::path::PathBuf],
) -> Result<(reqwest::blocking::multipart::Form, u64)> {
    let mut attachment_bytes: u64 = 0;
    let mut form = reqwest::blocking::multipart::Form::new();
    for (key, value) in body {
        let text = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        form = form.text(key.clone(), text);
    }
    for (field, paths) in [("images", images), ("files", files)] {
        for path in paths {
            let data = std::fs::read(path).with_context(|| {
                format!(
                    "failed to read --{} {}",
                    field.trim_end_matches('s'),
                    path.display()
                )
            })?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .with_context(|| {
                    format!(
                        "--{} {} has no usable filename",
                        field.trim_end_matches('s'),
                        path.display()
                    )
                })?
                .to_string();
            attachment_bytes += data.len() as u64;
            let part = reqwest::blocking::multipart::Part::bytes(data).file_name(filename);
            form = form.part(field, part);
        }
    }
    Ok((form, attachment_bytes))
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

/// Human-readable byte size for reap reports (binary units, one decimal).
fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let f = n as f64;
    if f >= GB {
        format!("{:.1} GB", f / GB)
    } else if f >= MB {
        format!("{:.1} MB", f / MB)
    } else if f >= KB {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{n} B")
    }
}

/// snake_case status label for reap reports (mirrors the wire representation).
fn reap_status_label(status: &event_log::RunStatus) -> &'static str {
    match status {
        event_log::RunStatus::Running => "running",
        event_log::RunStatus::AwaitingUser => "awaiting_user",
        event_log::RunStatus::Completed => "completed",
        event_log::RunStatus::Failed => "failed",
        event_log::RunStatus::Skipped => "skipped",
        event_log::RunStatus::Halted => "halted",
        event_log::RunStatus::Paused => "paused",
        event_log::RunStatus::Archived => "archived",
    }
}

/// One-shot `pdo reap`: the deterministic half of the disk janitor. Lists
/// `GET /runs/reapable?size=true`, runs the pure [`reap_policy`] over it, and —
/// unless `--count`/`--dry-run` — archives each selected Run via
/// `POST /runs/{id}/commands {"kind":"cleanup_run"}`.
///
/// **Exit semantics.** Failure to *list* is fatal: a broken URL that silently
/// no-ops forever is worse than a visible node failure. Per-run reclaim errors and
/// a wall-clock cutoff must NOT be fatal — a failed janitor Run leaves its own
/// worktree behind, which a `completed`-only policy never reclaims, so the residue
/// grows monotonically. The rest waits for the next fire.
pub fn run_reap(
    count: bool,
    dry_run: bool,
    ttl_hours: Option<i64>,
    terminal_ttl_hours: Option<i64>,
    budget_secs: u64,
) -> Result<()> {
    let url = cli_daemon_url();
    let client = reqwest::blocking::Client::new();
    let listing = format!("{url}/runs/reapable?size=true");

    let list_runs = |client: &reqwest::blocking::Client| -> Result<Vec<reap_policy::ReapableRun>> {
        client
            .get(&listing)
            .send()
            .context("failed to reach daemon (GET /runs/reapable)")?
            .error_for_status()
            .context("daemon rejected GET /runs/reapable")?
            .json()
            .context("failed to parse /runs/reapable response")
    };

    let runs = list_runs(&client)?;

    let mut policy = reap_policy::ReapPolicy::default();
    if let Some(h) = ttl_hours {
        policy.completed_ttl_secs = h.saturating_mul(3600);
    }
    if let Some(h) = terminal_ttl_hours {
        policy.terminal_ttl_secs = h.saturating_mul(3600);
    }

    let plan = policy.plan(&runs);

    if count {
        println!("{}", plan.reclaim.len());
        return Ok(());
    }

    if plan.reclaim.is_empty() {
        println!(
            "disk-janitor: nothing to reclaim ({} terminal run(s) listed, none past TTL)",
            plan.retained
        );
        return Ok(());
    }

    println!(
        "disk-janitor: {} run(s) to reclaim (~{}), {} retained",
        plan.reclaim.len(),
        human_bytes(plan.reclaim_bytes),
        plan.retained
    );
    for d in &plan.reclaim {
        println!(
            "  {} [{}] ~{} — {}",
            d.run_id,
            reap_status_label(&d.status),
            human_bytes(d.approx_disk_bytes.unwrap_or(0)),
            d.reason
        );
    }

    if dry_run {
        println!("disk-janitor: --dry-run, no reclaim issued");
        return Ok(());
    }

    // Reclaim within the wall-clock budget.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(budget_secs.max(1));
    let mut reclaimed_ids: Vec<String> = Vec::new();
    let mut deferred = 0usize;
    let mut errors = 0usize;

    for d in &plan.reclaim {
        if std::time::Instant::now() >= deadline {
            deferred += 1;
            continue;
        }
        let endpoint = format!("{url}/runs/{}/commands", d.run_id);
        match client
            .post(&endpoint)
            .json(&serde_json::json!({ "kind": "cleanup_run" }))
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                let code = status.as_u16();
                let body = resp.text().unwrap_or_default();
                // Don't trust a bare `200`: a mistyped path 200s the SPA shell and
                // `cleanup_run` swallows fs/git errors, so assert on the body.
                // `409` = archived between listing and now, a benign no-op.
                if code == 409 || (code == 200 && body.contains("\"archived\"")) {
                    reclaimed_ids.push(d.run_id.clone());
                } else {
                    errors += 1;
                    eprintln!("  cleanup_run {} → {} {}", d.run_id, status, body.trim());
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("  cleanup_run {} → transport error: {e}", d.run_id);
            }
        }
    }

    // Honest proof: re-list and confirm the reclaimed ids are gone. A `200` /
    // `409` above only says the command was accepted, not that the bytes left.
    let confirmed = match list_runs(&client) {
        Ok(after) => {
            let still: std::collections::HashSet<String> =
                after.into_iter().map(|r| r.run_id).collect();
            reclaimed_ids
                .iter()
                .filter(|id| !still.contains(*id))
                .count()
        }
        // Can't re-list — report the accepted count without the confirmation.
        Err(_) => reclaimed_ids.len(),
    };

    println!(
        "disk-janitor: reclaimed {} run(s) ({confirmed} confirmed gone), {deferred} deferred (budget {budget_secs}s), {errors} error(s)",
        reclaimed_ids.len()
    );

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
        run_trigger_scheduler_tick_supervised(&self.state).await;
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
        let _ = trigger_store::set_next_fire(
            &self.state.db,
            trigger_id,
            Some("2020-01-01T00:00:00.000Z"),
        )
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
        let trigger = trigger_store::create(
            &self.state.db,
            trigger_store::NewTrigger {
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
            },
        )
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
    /// `run_trigger_scheduler_tick` by re-reading the pause kill-switch late.
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
        self_writes: recent_writes.clone(),
        workspace: workspace.clone(),
        docker_cmd_override: config.docker_cmd_override.clone(),
        sandbox_home_override: config.sandbox_home_override.clone(),
        panic_on_stale_sweep: config.panic_on_stale_sweep,
        panic_on_spawn: config.panic_on_spawn,
    };

    let state = Arc::new_cyclic(|weak: &std::sync::Weak<AppState>| {
        let wired = wire_contexts(resources.into_resources(weak.clone()));
        contexts_router = Some(wired.router);
        AppState {
            workspace,
            skills,
            platform: wired.platform,
            db,
            run: wired.run,
            bus_tx,
            repo_root,
            port: bound_addr.port(),
            trigger_tick_lock: tokio::sync::Mutex::new(()),
            recent_writes,
            run_watcher: run_watcher.clone(),
            tmux_cmd_override: config.tmux_cmd_override,
            docker_cmd_override: config.docker_cmd_override,
            sandbox_home_override: config.sandbox_home_override,
            last_trigger_tick_ms: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            panic_on_trigger_name: config.panic_on_trigger_name,
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
        handle_run_pipeline_modifications(mod_state, run_modified_rx).await;
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

    // Fires due Triggers on a ~30s tick. Best-effort: only runs while the daemon
    // lives, forward-only — no backfill of missed slots.
    let _trigger_handle = if nested_daemon || !run_trigger_scheduler_loop {
        None
    } else {
        let trigger_state = state.clone();
        Some(tokio::spawn(async move {
            let mut tick =
                time::interval(Duration::from_secs(trigger_scheduler::TICK_INTERVAL_SECS));
            loop {
                tick.tick().await;
                run_trigger_scheduler_tick_supervised(&trigger_state).await;
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

#[derive(Deserialize)]
struct CreateTriggerRequest {
    name: String,
    /// Library pipeline id the Trigger fires.
    pipeline_id: String,
    #[serde(default)]
    target_repo: Option<String>,
    /// Secondary repos to associate with fired Runs (ADR-0042). Stored as raw JSON
    /// TEXT and forwarded at fire time. Absent → mono-repo fire.
    #[serde(default)]
    target_repos: Vec<TargetRepoInput>,
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
    /// Bounded-`allow` ceiling: max simultaneous live Runs. `None` = unbounded.
    /// Inert unless `overlap_policy == "allow"`.
    #[serde(default)]
    max_concurrent: Option<i64>,
    /// Per-Trigger sandbox mode: `"off"` | `"full"` | `"minimal"`, or absent/`null` to
    /// inherit the instance default. Read at fire time and folded into the create
    /// request's explicit tier.
    #[serde(default)]
    sandbox: Option<String>,
    /// Per-Trigger harness (ADR-0046): a harness name, or absent/`null`/blank to
    /// inherit the instance default. Folded at fire time into the fired Run's harness —
    /// there is no separate Trigger tier. Free text, no validation (ADR-0045).
    #[serde(default)]
    harness: Option<String>,
    /// Per-Trigger `AgentChoice` (ADR-0057): `Inherit`/absent falls through to the
    /// legacy `harness` field above; `Profile`/`Custom` is folded into the fired Run's
    /// explicit tier at fire time.
    #[serde(default)]
    agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Run-tier skills every fired Run carries (#669, ADR-0062). Folded into
    /// the fired Run's `skills` at fire time, like `harness`.
    #[serde(default)]
    skills: Vec<crate::skill_selection::SkillRef>,
    /// Whether Runs this Trigger fires are auto-named. Seeded from the instance
    /// default in the create modal and frozen on the row; defaults to `true` so an
    /// older client that omits the field keeps auto-naming.
    #[serde(default = "default_true")]
    auto_name: bool,
}

fn default_overlap_policy() -> String {
    "skip".to_string()
}

/// Matches the `NOT NULL DEFAULT 1` column and the built-in resolver default.
fn default_true() -> bool {
    true
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

/// Validate a `max_concurrent` cap: when present it must be >= 1. Shared by create
/// and patch so the rule can't drift between them. A cap set while
/// `overlap_policy == "skip"` is accepted and stays inert.
fn validate_max_concurrent(v: Option<i64>) -> Result<(), &'static str> {
    match v {
        Some(n) if n < 1 => Err("max_concurrent must be >= 1"),
        _ => Ok(()),
    }
}

/// Deserialize a double-`Option` PATCH field so a present `null` maps to `Some(None)`
/// ("clear to NULL"), distinct from absent → `None` ("leave untouched"). serde's
/// default for `Option<Option<T>>` collapses both to `None`; this helper is only
/// invoked when the key is present, which is what recovers the three-way distinction.
fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

/// Body of `POST /triggers/guard/test`: the guard command *as currently typed in the
/// New-trigger tab* plus an optional target repo. No trigger id — this is pre-save.
#[derive(serde::Deserialize)]
struct GuardTestRequest {
    guard_command: String,
    #[serde(default)]
    target_repo: Option<String>,
}

/// Response of `POST /triggers/guard/test`: a 1:1 projection of
/// [`fire_decision::GuardResult`]. The would-fire / would-skip verdict is composed
/// client-side from `outcome`. `exit_code` is `None` on error (spawn failure/timeout).
#[derive(serde::Serialize, TS)]
#[ts(export, rename = "TestGuardResponse")]
struct GuardTestResponse {
    outcome: &'static str,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    detail: Option<String>,
}

impl From<fire_decision::GuardResult> for GuardTestResponse {
    fn from(r: fire_decision::GuardResult) -> Self {
        use fire_decision::GuardResult::*;
        match r {
            Pass { stdout } => Self {
                outcome: "pass",
                stdout,
                stderr: String::new(),
                exit_code: Some(0),
                detail: None,
            },
            Skip {
                stdout,
                stderr,
                exit_code,
            } => Self {
                outcome: "skip",
                stdout,
                stderr,
                exit_code,
                detail: None,
            },
            Error { detail } => Self {
                outcome: "error",
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
                detail: Some(detail),
            },
        }
    }
}

/// `POST /triggers/guard/test` — dry-run a Trigger guard command.
///
/// Zero-side-effect invariant: this handler must call ONLY `required_target_repo`,
/// `instance_config::get`, `guard_runner::guard_timeout_with` and
/// `guard_runner::run_guard`. It must never route through `fire_one_trigger`,
/// `plan_tick`, `create_run_inner`, `trigger_store::*` or `record_and_broadcast_fire`
/// — no Run spawned, no `trigger_fires` row, no `next_fire_at` recompute.
async fn guard_test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GuardTestRequest>,
) -> Response {
    if req.guard_command.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "guard_command must not be empty" })),
        )
            .into_response();
    }

    // Same rule as a real fire: a guard must never execute in a repository nobody
    // named. The dry-run is pre-save, but it runs the same arbitrary `sh -c`.
    let cwd = match required_target_repo(req.target_repo.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    // Same `stored → env → default` precedence a real fire uses (ADR-0015), so the
    // dry-run reflects the bound the scheduler would apply. Stored is seconds; the env
    // seam is ms.
    let stored_secs = instance_config::get(&state.db)
        .await
        .ok()
        .and_then(|c| c.guard_timeout_secs)
        .map(|n| n as u64);
    let timeout = guard_runner::guard_timeout_with(stored_secs);

    let result = guard_runner::run_guard(req.guard_command.trim(), &cwd, timeout).await;
    Json(GuardTestResponse::from(result)).into_response()
}

async fn create_trigger(
    State(state): State<Arc<AppState>>,
    actor: OriginHint,
    Json(req): Json<CreateTriggerRequest>,
) -> Response {
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "trigger name must not be empty" })),
        )
            .into_response();
    }

    // Fail loud at config time, not at 3am (ADR-0001).
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

    if let Err(msg) = validate_max_concurrent(req.max_concurrent) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }

    if let Some(raw) = req.sandbox.as_deref() {
        if let Err(msg) = validate_sandbox_ref(&state.db, raw).await {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }

    let yaml =
        std::fs::read_to_string(resolve_pipeline_path(&state.repo_root, &req.pipeline_id)).ok();
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

    // A cron-only Trigger has no guard, so the input template is the only input
    // source: empty + prompt-required is a misconfiguration. Refuse at creation.
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

    // A Trigger is a Run template (ADR-0012b), so it cannot be valid without a
    // target repo.
    let target_repo = match required_target_repo(req.target_repo.as_deref()) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    // The first fire, via the single recompute seam. `NewTrigger.next_fire_at` is a
    // flat `Option<String>`, so flatten what `Create` returns; `Some(None)` = an
    // impossible cron, which stays dormant.
    let now = chrono::Utc::now();
    let next_fire_at = trigger_scheduler::recompute_next_fire(
        now,
        trigger_scheduler::Transition::Create(&schedule),
    )
    .flatten();

    let variables = serde_json::to_value(&req.variables).unwrap_or(serde_json::json!({}));

    let new = trigger_store::NewTrigger {
        name: req.name,
        pipeline_id: req.pipeline_id,
        pipeline_name,
        target_repo: Some(target_repo),
        target_repos: if req.target_repos.is_empty() {
            None
        } else {
            serde_json::to_string(&req.target_repos).ok()
        },
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
        max_concurrent: req.max_concurrent,
        // Normalise `Some("")` to `None` (inherit the instance default), so an empty
        // selector value never persists as a bogus stored mode.
        sandbox: req.sandbox.filter(|s| !s.trim().is_empty()),
        // Same normalisation for the harness: a blank value must inherit at fire time
        // rather than persist as a harness named "".
        harness: req.harness.filter(|s| !s.trim().is_empty()),
        // No normalisation needed: `None`/`Inherit`/blank payloads are already
        // transparent inside `pdo_harness::agent_choice::resolve`.
        agent_choice: req.agent_choice,
        // #669: the store normalises (trim, dedupe) and stores NULL for empty.
        skills: req.skills,
        // Frozen on the row, never re-resolved at fire time — the runtime never
        // decides autonomy (ADR-0012 frontier).
        auto_name: req.auto_name,
        next_fire_at,
    };

    match trigger_store::create(&state.db, new).await {
        Ok(trigger) => {
            let _ = state.bus_tx.send(serde_json::json!({
                "type": "trigger_created",
                "trigger_id": trigger.id,
            }));
            // Audit AFTER the row committed (ADR-0044 §2·B — never over-report).
            state
                .platform
                .record_audit()
                .execute(pdo_platform::domain::audit::NewAuditEntry {
                    actor_hint: actor.as_hint().to_string(),
                    action: "trigger.created".to_string(),
                    target_kind: Some("trigger".to_string()),
                    target_id: Some(trigger.id.clone()),
                    before: None,
                    after: serde_json::to_value(&trigger).ok(),
                })
                .await;
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
        Ok(triggers) => {
            // Read-side fallback only, and it stays: historical rows legitimately
            // carry a null repo (the write boundary now refuses one), and the "no
            // Unassigned bucket" invariant depends on resolving them here. Do NOT
            // symmetrise it with `create_trigger`/`patch_trigger`. The raw
            // `target_repo` is left untouched so the badge / detail / Run-now
            // pre-fill keep showing only what the user actually set.
            let repo_root = state.repo_root.to_string_lossy().into_owned();
            let entries: Vec<TriggerListEntry> = triggers
                .into_iter()
                .map(|t| {
                    let effective_repo = t.target_repo.clone().unwrap_or_else(|| repo_root.clone());
                    TriggerListEntry {
                        trigger: t,
                        effective_repo,
                    }
                })
                .collect();
            Json(entries).into_response()
        }
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
    actor: OriginHint,
) -> Response {
    // Read the "before" snapshot HERE, before the row is gone: `delete` returns
    // `bool`, never the row, so a later read would leave the audit a dead UUID.
    let before = trigger_store::get(&state.db, &trigger_id)
        .await
        .ok()
        .flatten();
    match trigger_store::delete(&state.db, &trigger_id).await {
        Ok(true) => {
            let _ = state.bus_tx.send(serde_json::json!({
                "type": "trigger_deleted",
                "trigger_id": trigger_id,
            }));
            state
                .platform
                .record_audit()
                .execute(pdo_platform::domain::audit::NewAuditEntry {
                    actor_hint: actor.as_hint().to_string(),
                    action: "trigger.deleted".to_string(),
                    target_kind: Some("trigger".to_string()),
                    target_id: Some(trigger_id.clone()),
                    before: before.as_ref().and_then(|t| serde_json::to_value(t).ok()),
                    after: None,
                })
                .await;
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

/// A partial Trigger edit. Every field is optional; absent fields are left
/// untouched.
#[derive(Deserialize)]
struct PatchTriggerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Repoint the Trigger to a different library pipeline. Validated against the
    /// library/repo before applying; the denormalised display name and a revived
    /// next-fire follow from it.
    #[serde(default)]
    pipeline_id: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    input_template: Option<String>,
    #[serde(default)]
    overlap_policy: Option<String>,
    /// Double-wrapped AND custom-deserialized, unlike its plain siblings below: the
    /// point is to make the explicit CLEAR (`"target_repo": null`) *reachable* so it
    /// can be REFUSED. With plain `#[serde(default)]` a present-`null` collapses to
    /// `None` and the clear is a silent no-op, which is what the UI was sending on
    /// every Trigger save.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    target_repo: Option<Option<String>>,
    /// The secondary list, double-wrapped like `target_repo`: absent = leave, present
    /// `null`/`[]` = clear to mono-repo, an array = set. Stored as raw JSON TEXT.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    target_repos: Option<Option<Vec<TargetRepoInput>>>,
    #[serde(default)]
    source_branch: Option<Option<String>>,
    #[serde(default)]
    guard_command: Option<Option<String>>,
    #[serde(default)]
    variables: Option<HashMap<String, serde_yaml::Value>>,
    /// Bounded-`allow` ceiling, double-wrapped: absent = leave, present `null` =
    /// clear to unbounded, `n` = set. `source_branch` / `guard_command` above keep
    /// plain `#[serde(default)]`, where present-`null` collapses and cannot clear.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    max_concurrent: Option<Option<i64>>,
    /// Per-Trigger sandbox mode, double-wrapped like `max_concurrent`: present `null`
    /// clears back to inheriting the instance default, which is how the UI offers
    /// "use the instance default".
    #[serde(default, deserialize_with = "deserialize_double_option")]
    sandbox: Option<Option<String>>,
    /// Per-Trigger harness, double-wrapped like `sandbox`; present `null` clears back
    /// to inheriting the instance default.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    harness: Option<Option<String>>,
    /// Per-Trigger `AgentChoice`, double-wrapped like `harness`; present `null` clears
    /// back to inheriting the legacy `harness` field.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    agent_choice: Option<Option<pdo_harness::agent_choice::AgentChoice>>,
    /// The Run-tier skills (#669), replaced wholesale: absent leaves them, a list
    /// sets them, an empty list clears them. Flat: the empty list IS the clear.
    #[serde(default)]
    skills: Option<Vec<crate::skill_selection::SkillRef>>,
    /// Auto-naming toggle — a FLAT `Option<bool>`, deliberately not double-wrapped:
    /// there is no "inherit" state to clear back to, a Trigger's autonomy is a plain
    /// on/off.
    #[serde(default)]
    auto_name: Option<bool>,
}

async fn patch_trigger(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
    actor: OriginHint,
    Json(req): Json<PatchTriggerRequest>,
) -> Response {
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

    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "trigger name must not be empty" })),
            )
                .into_response();
        }
    }

    // One `now` for the whole request: a combined enable + cron-edit + repoint PATCH
    // must recompute every slot against the same instant.
    let now = chrono::Utc::now();

    let mut next_fire_at: Option<Option<String>> = None;
    if let Some(ref cron) = req.cron {
        match cron_schedule::CronSchedule::parse(cron) {
            Ok(schedule) => {
                next_fire_at = trigger_scheduler::recompute_next_fire(
                    now,
                    trigger_scheduler::Transition::CronEdit(&schedule),
                );
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

    // Parse the target's YAML to capture the display name (denormalised on the row)
    // and the prompt_required flag the reject rule below needs.
    let mut repoint: Option<(String, String)> = None;
    let mut repoint_prompt_required: Option<bool> = None;
    if let Some(ref new_pid) = req.pipeline_id {
        if *new_pid != existing.pipeline_id {
            let yaml =
                std::fs::read_to_string(resolve_pipeline_path(&state.repo_root, new_pid)).ok();
            let yaml = match yaml {
                Some(y) => y,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("pipeline not found: {new_pid}")
                        })),
                    )
                        .into_response();
                }
            };
            match pipeline::parse_pipeline(&yaml) {
                Ok(r) => {
                    repoint_prompt_required = Some(r.pipeline.prompt_required);
                    repoint = Some((new_pid.clone(), r.pipeline.name.clone()));
                }
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("pipeline parse error: {e}") })),
                    )
                        .into_response();
                }
            }
            // Revive the next fire from the existing cron unless a schedule edit
            // already recomputed it, so a Trigger that went dormant on a pipeline-id
            // rename comes back to life on repoint instead of staying dead.
            if next_fire_at.is_none() {
                if let Ok(schedule) = cron_schedule::CronSchedule::parse(&existing.cron) {
                    next_fire_at = trigger_scheduler::recompute_next_fire(
                        now,
                        trigger_scheduler::Transition::Repoint(&schedule),
                    );
                }
            }
        }
    }

    // Re-activating a disabled Trigger recomputes next_fire_at FORWARD from now:
    // the missed slot is SKIPPED, no catch-up (ADR-0012 amended). Merely flipping
    // the bit leaves next_fire_at frozen in the past and the next tick fires an
    // implicit catch-up. `enabling` is the disabled→enabled front ONLY: an
    // `{enabled:true}` on an already-live Trigger must not shift its schedule. The
    // `next_fire_at.is_none()` guard is load-bearing — a combined enable + cron-edit
    // already recomputed off the *new* schedule, and Enable would overwrite it with
    // the *existing* cron.
    let enabling = !existing.enabled && req.enabled == Some(true);
    if enabling && next_fire_at.is_none() {
        if let Ok(schedule) = cron_schedule::CronSchedule::parse(&existing.cron) {
            next_fire_at = trigger_scheduler::recompute_next_fire(
                now,
                trigger_scheduler::Transition::Enable(&schedule),
            );
        }
    }

    // PATCH stays a partial merge: an ABSENT `target_repo` leaves the stored value
    // untouched, which is what makes `{"enabled": true}` a legal body. What is
    // refused is an explicit CLEAR (`null` or a blank string).
    let mut validated_target_repo: Option<Option<String>> = None;
    if let Some(ref edit) = req.target_repo {
        match required_target_repo(edit.as_deref()) {
            Ok(p) => validated_target_repo = Some(Some(p.to_string_lossy().into_owned())),
            Err(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
        }
    }

    if let Some(Some(n)) = req.max_concurrent {
        if let Err(msg) = validate_max_concurrent(Some(n)) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }

    // Same shared existence gate as create and `PUT /settings`.
    if let Some(Some(ref mode)) = req.sandbox {
        if let Err(msg) = validate_sandbox_ref(&state.db, mode).await {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }

    // Re-apply the create-time reject rule against the *resulting* config.
    let resulting_guard = match &req.guard_command {
        Some(g) => g.clone(),
        None => existing.guard_command.clone(),
    };
    let resulting_input = req
        .input_template
        .clone()
        .unwrap_or_else(|| existing.input_template.clone());
    // When repointing, judge the *new* pipeline's prompt_required, not the old one's.
    let prompt_required =
        repoint_prompt_required.unwrap_or_else(|| trigger_prompt_required(&state, &existing));
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
        pipeline_id: repoint.as_ref().map(|(id, _)| id.clone()),
        pipeline_name: repoint.as_ref().map(|(_, name)| name.clone()),
        // The trimmed, validated path — never the raw field.
        target_repo: validated_target_repo,
        // Absent → leave; present `null`/`[]` → clear to mono-repo; a non-empty array
        // → the serialized JSON.
        target_repos: req.target_repos.map(|inner| {
            inner
                .filter(|list| !list.is_empty())
                .and_then(|list| serde_json::to_string(&list).ok())
        }),
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
        max_concurrent: req.max_concurrent,
        sandbox: req.sandbox,
        // Normalise a stray `Some(Some(""))` to a clean clear, so a blank never
        // persists as a harness named "".
        harness: req
            .harness
            .map(|inner| inner.filter(|s| !s.trim().is_empty())),
        agent_choice: req.agent_choice,
        skills: req.skills,
        auto_name: req.auto_name,
        next_fire_at,
        // The enable bit and the forward next_fire_at must land in ONE atomic UPDATE,
        // or enabled=1 can persist with a stale (past) next_fire_at.
        enabled: req.enabled,
    };

    if let Err(e) = trigger_store::update(&state.db, &trigger_id, edit).await {
        error!("failed to update trigger: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "failed to update trigger" })),
        )
            .into_response();
    }

    match trigger_store::get(&state.db, &trigger_id).await {
        Ok(Some(updated)) => {
            let _ = state.bus_tx.send(serde_json::json!({
                "type": "trigger_updated",
                "trigger_id": trigger_id,
            }));
            // Persist BOTH snapshots (ADR-0044 §2·D): "a patch happened" cannot tell
            // a manual `enabled:false` from a cron edit unless before→after is on the
            // row.
            state
                .platform
                .record_audit()
                .execute(pdo_platform::domain::audit::NewAuditEntry {
                    actor_hint: actor.as_hint().to_string(),
                    action: "trigger.updated".to_string(),
                    target_kind: Some("trigger".to_string()),
                    target_id: Some(trigger_id.clone()),
                    before: serde_json::to_value(&existing).ok(),
                    after: serde_json::to_value(&updated).ok(),
                })
                .await;
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

/// `POST /triggers/{id}/fire` — a manual "Run now" fire (ADR-0027). A first-class
/// fire: same guard, same overlap gate, same `trigger_fires` audit row
/// (`source = "manual"`), same `triggered_by` provenance — but `next_fire_at` is
/// left untouched, because the cron heartbeat owns the schedule.
///
/// Contract (ADR-0025): 404 unknown; 409 disabled or broken reference, before any
/// effect and with no audit row; otherwise `200 {ok, fired, …}` — a guard/overlap
/// skip is an honest 200, not an error.
async fn fire_trigger_now(
    State(state): State<Arc<AppState>>,
    AxumPath(trigger_id): AxumPath<String>,
) -> Response {
    let trigger = match trigger_store::get(&state.db, &trigger_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("trigger not found: {trigger_id}") })),
            )
                .into_response();
        }
        Err(e) => {
            error!("failed to load trigger {trigger_id}: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to load trigger" })),
            )
                .into_response();
        }
    };

    // Disabled ⇒ explicit 409 before any effect. A human click on a paused trigger
    // deserves a truthful refusal, not a silent skip (ADR-0025); `decide()`'s silent
    // no-op stays cron-only.
    if !trigger.enabled {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("trigger '{trigger_id}' is disabled — enable it first"),
            })),
        )
            .into_response();
    }

    // Refuse explicitly rather than record an error outcome — the cron path keeps
    // its own audited treatment.
    if let Some(reason) = trigger_dangling_reason(&state, &trigger) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": format!("broken reference: {reason}") })),
        )
            .into_response();
    }

    // Serialize with the cron tick, or a manual fire and a tick race the same
    // overlap window.
    let _tick = state.trigger_tick_lock.lock().await;
    let now = chrono::Utc::now();
    let record =
        fire_one_trigger(&state, &trigger, now, trigger_scheduler::FireSource::Manual).await;

    match record {
        Some(r) if r.outcome == "fired" => Json(serde_json::json!({
            "ok": true,
            "fired": true,
            "run_id": r.run_id,
        }))
        .into_response(),
        Some(r) => Json(serde_json::json!({
            "ok": true,
            "fired": false,
            "outcome": r.outcome,
            "reason": r.reason,
        }))
        .into_response(),
        // Unreachable today (due is forced, enabled was checked); stay truthful
        // rather than panic if the decision core evolves.
        None => Json(serde_json::json!({
            "ok": true,
            "fired": false,
            "outcome": "skipped",
            "reason": "no action taken",
        }))
        .into_response(),
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
        // #779: the multipart body of `POST /runs` is bounded by the
        // `max_attachments_mb` setting, enforced IN the handler on the attachment
        // parts (read fresh per request) — not by a static limit frozen at
        // router build. The JSON branch keeps its own 10 MB `to_bytes` cap.
        .route(
            "/runs",
            post(create_run).route_layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .route("/runs", get(list_runs))
        // Static path — must stay declared before `/runs/{run_id}`.
        .route("/runs/reapable", get(list_reapable_runs))
        .route("/runs/{run_id}", axum::routing::delete(forget_run))
        .route("/runs/{run_id}/children", get(list_run_children))
        .route("/runs/{run_id}/diff", get(run_diff))
        .route("/runs/{run_id}/diff/structured", get(run_diff_structured))
        .route("/runs/{run_id}/file", get(run_file_at_ref))
        .route("/runs/{run_id}/refs", get(run_refs))
        .route("/runs/{run_id}/source-drift", get(run_source_drift))
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
        .route("/runs/{run_id}/artifact", get(artifact))
        .route("/runs/{run_id}/pipeline", get(get_run_pipeline))
        .route(
            "/runs/{run_id}/pipeline/document",
            get(get_run_pipeline_document),
        )
        .route(
            "/runs/{run_id}/pipeline/document/skills",
            get(get_run_pipeline_document_skills),
        )
        .route(
            "/runs/{run_id}/repos",
            axum::routing::patch(patch_run_repos),
        )
        .route(
            "/runs/{run_id}/pipeline",
            axum::routing::put(save_run_pipeline),
        )
        // Manager on demand: start/stop the Run's Pipeline Manager from the
        // Manager tab. Start is idempotent (a double-click is a benign
        // re-answer); stop on a dead session is a calm no-op.
        .route("/runs/{run_id}/manager/start", post(start_run_manager))
        .route("/runs/{run_id}/manager/stop", post(stop_run_manager))
        .route("/triggers", get(list_triggers).post(create_trigger))
        .route("/triggers/health", get(triggers_health))
        // Dry-run a guard command with no side effects — the opposite pole of "Run
        // now" (ADR-0027 addendum).
        .route("/triggers/guard/test", post(guard_test))
        // Global Trigger kill-switch.
        .route("/triggers/pause", post(pause_triggers))
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
        .route(
            "/triggers/{trigger_id}",
            get(get_trigger).patch(patch_trigger).delete(delete_trigger),
        )
        .route("/triggers/{trigger_id}/fires", get(list_trigger_fires))
        // Manual "Run now" — a first-class fire.
        .route("/triggers/{trigger_id}/fire", post(fire_trigger_now))
        .fallback(static_handler)
        .with_state(state.clone())
        .merge(
            pdo_workspace::adapters::http::router::<LegacyProjectSettings>(state.workspace.clone()),
        )
        .merge(pdo_skill::adapters::http::router(state.skills.clone()))
        // The run context (#923, #924): `/ws`, a Run's detail and events, the node
        // routes, the Pipeline Manager's commands, the stale sweep's heartbeat.
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

    trigger_store::init(db)
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

/// How many of the Trigger's *own* Runs are still live. A finished Run drops out of
/// the count and frees a slot, which is what makes "max simultaneous live" hold with
/// no extra state to track.
async fn trigger_live_run_count(db: &sqlx::SqlitePool, trigger_id: &str) -> usize {
    let run_ids = match load_all_run_ids(db).await {
        Ok(ids) => ids,
        Err(e) => {
            warn!("trigger scheduler: failed to load run ids: {e}");
            return 0;
        }
    };
    let mut count = 0usize;
    for run_id in run_ids {
        let events = match load_events(db, &run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(state) = event_log::project(&events) {
            if state.triggered_by.as_deref() == Some(trigger_id) && state.status.is_live() {
                count += 1;
            }
        }
    }
    count
}

/// Load a Trigger's target pipeline yaml from the library (falling back to a
/// pipeline file under the repo). `None` means a dangling pipeline reference.
fn trigger_pipeline_yaml(state: &AppState, trigger: &trigger_store::Trigger) -> Option<String> {
    std::fs::read_to_string(resolve_pipeline_path(
        &state.repo_root,
        &trigger.pipeline_id,
    ))
    .ok()
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

/// Check a Trigger's external references before firing, so the scheduler surfaces
/// an error outcome and stops firing rather than rotting silently.
fn trigger_dangling_reason(state: &AppState, trigger: &trigger_store::Trigger) -> Option<String> {
    if trigger_pipeline_yaml(state, trigger).is_none() {
        return Some(format!("pipeline not found: {}", trigger.pipeline_id));
    }
    // A null/blank target repo is a dangling reference, not a fallback. Refuse HERE
    // rather than at `create_run_inner`, for three things a chokepoint 400 cannot
    // buy: the guard command never runs (guards do `git pull` / `gh issue list`,
    // which would execute in the unnamed daemon repo), the `Dangling` transition
    // NULLs `next_fire_at` so the Trigger goes dormant instead of emitting one red
    // row per tick forever, and `POST /triggers/:id/fire` answers 409 with a banner
    // instead of a `200 {fired:false}` buried in the fire history.
    if let Err(msg) = required_target_repo(trigger.target_repo.as_deref()) {
        return Some(msg);
    }
    None
}

/// Run one Trigger scheduler tick under panic isolation. The ~30 s background loop
/// *and* the [`DaemonHandle::run_trigger_tick`] test seam both go through here, so
/// production and the synchronous test path behave identically.
async fn run_trigger_scheduler_tick_supervised(state: &Arc<AppState>) {
    let tick_state = state.clone();
    run_isolated("trigger scheduler", async move {
        run_trigger_scheduler_tick(&tick_state).await
    })
    .await;
}

/// Run one scheduler tick: fire every due Trigger that the decision admits,
/// recording every significant outcome and recomputing each next fire.
async fn run_trigger_scheduler_tick(state: &Arc<AppState>) {
    let _tick = state.trigger_tick_lock.lock().await;
    let now = chrono::Utc::now();
    // Record liveness at tick START: a wedged tick then freezes the value, while an
    // isolated panic has already advanced it, so `/triggers/health` answers "is the
    // loop alive?".
    state
        .last_trigger_tick_ms
        .store(now.timestamp_millis(), std::sync::atomic::Ordering::Relaxed);

    // Global kill-switch: skip the whole fire loop while paused. Read FRESH each
    // tick so `POST /triggers/pause` takes effect without a restart. Liveness above
    // still advances — paused ≠ dead. Because `due_triggers` is never called while
    // paused, `next_fire_at` stays frozen in the past, so on unpause the next
    // ordinary tick fires each due Trigger once and recomputes forward: a one-shot
    // catch-up with NO dedicated code and NO `Transition` variant. Fail-OPEN on a
    // read error, matching the scheduler's best-effort posture.
    if instance_config::triggers_paused(&state.db)
        .await
        .unwrap_or(false)
    {
        return;
    }

    let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let due = match trigger_store::due_triggers(&state.db, &now_str).await {
        Ok(t) => t,
        Err(e) => {
            warn!("trigger scheduler: due_triggers failed: {e}");
            return;
        }
    };

    for trigger in due {
        // TOCTOU close: re-read the kill-switch at the last moment. The tick-start
        // read is stale by now — `due_triggers` suspends on a DB round-trip, and
        // `POST /triggers/pause` deliberately does NOT take `trigger_tick_lock` (a
        // lock held across the guard subprocess would make the brake hang for
        // ~guard-timeout, and a brake that hangs is no brake). Without this re-read a
        // tick inside its critical section still fires a Run AFTER the operator
        // paused. Bail the whole tick: `next_fire_at` stays frozen, preserving the
        // one-shot catch-up. Fail-OPEN, same posture as above. "Run now" never
        // reaches this Cron path, so it stays exempt.
        if instance_config::triggers_paused(&state.db)
            .await
            .unwrap_or(false)
        {
            return;
        }

        if state.panic_on_trigger_name.as_deref() == Some(trigger.name.as_str()) {
            panic!(
                "PDO_DEBUG_PANIC_TRIGGER fault injection (#222): trigger {}",
                trigger.name
            );
        }

        // Surface an error outcome and stop firing (clear next_fire) rather than rot
        // silently or auto-delete. The clear goes through the single recompute seam.
        if let Some(reason) = trigger_dangling_reason(state, &trigger) {
            let cleared = trigger_scheduler::recompute_next_fire(
                now,
                trigger_scheduler::Transition::Dangling,
            )
            .flatten();
            let _ = trigger_store::set_next_fire(&state.db, &trigger.id, cleared.as_deref()).await;
            let record = trigger_store::FireRecord {
                outcome: "error".to_string(),
                reason: Some(reason),
                run_id: None,
                guard_stdout: None,
                guard_stderr: None,
                guard_exit_code: None,
                source: Some(trigger_scheduler::FireSource::Cron.as_str().to_string()),
            };
            record_and_broadcast_fire(state, &trigger.id, &record).await;
            continue;
        }

        fire_one_trigger(state, &trigger, now, trigger_scheduler::FireSource::Cron).await;
    }
}

/// Evaluate and (maybe) fire one Trigger — the shared spine of the cron tick and
/// the manual "Run now" endpoint (ADR-0027). On `Cron` the next fire is recomputed
/// forward; on `Manual` `next_fire_at` is left untouched, because the cron heartbeat
/// owns the schedule and a 14:32 click must not shift the 15:00 slot.
///
/// `None` means the evaluation was a silent no-op.
async fn fire_one_trigger(
    state: &Arc<AppState>,
    trigger: &trigger_store::Trigger,
    now: chrono::DateTime<chrono::Utc>,
    source: trigger_scheduler::FireSource,
) -> Option<trigger_store::FireRecord> {
    let live_count = trigger_live_run_count(&state.db, &trigger.id).await;
    let prompt_required = trigger_prompt_required(state, trigger);

    // Only run the guard when overlap wouldn't already skip — never spend a guard
    // run on a tick we'd skip anyway. Route it through the same `overlap_ceiling`
    // the decision core uses, or the guard-gate drifts from the fire decision.
    let overlap = if trigger.overlap_policy == "allow" {
        fire_decision::OverlapPolicy::Allow
    } else {
        fire_decision::OverlapPolicy::Skip
    };
    let overlap_skips =
        fire_decision::overlap_ceiling(overlap, trigger.max_concurrent.map(|m| m.max(0) as usize))
            .is_some_and(|ceiling| live_count >= ceiling);
    let guard = match (&trigger.guard_command, overlap_skips) {
        (Some(cmd), false) if !cmd.trim().is_empty() => {
            // Guaranteed present: `trigger_dangling_reason` refused the fire upstream
            // otherwise. No fallback to the daemon root.
            let cwd = PathBuf::from(trigger.target_repo.as_deref().unwrap_or_default());
            // Resolve the timeout fresh each tick so a settings change takes effect
            // without a restart. Stored is seconds; the env seam is ms.
            let stored_secs = instance_config::get(&state.db)
                .await
                .ok()
                .and_then(|c| c.guard_timeout_secs)
                .map(|n| n as u64);
            let timeout = guard_runner::guard_timeout_with(stored_secs);
            Some(guard_runner::run_guard(cmd, &cwd, timeout).await)
        }
        _ => None,
    };

    let plan =
        trigger_scheduler::plan_tick(trigger, now, live_count, guard, prompt_required, source);

    // Route the manual case through the recompute seam too, so the leave-vs-advance
    // rule stays in the one exhaustive match rather than an ad-hoc branch.
    let next_write: Option<Option<String>> = match source {
        trigger_scheduler::FireSource::Cron => Some(plan.next_fire_at.clone()),
        trigger_scheduler::FireSource::Manual => {
            trigger_scheduler::recompute_next_fire(now, trigger_scheduler::Transition::ManualFire)
        }
    };
    if let Some(next) = next_write {
        if let Err(e) = trigger_store::set_next_fire(&state.db, &trigger.id, next.as_deref()).await
        {
            warn!(
                "trigger scheduler: set_next_fire failed for {}: {e}",
                trigger.id
            );
        }
    }

    match plan.decision {
        fire_decision::FireDecision::Fire { input } => {
            // #804 / ADR-0070 §1: refresh the remotes BEFORE the cut, so a Trigger
            // whose source is a tracking ref fires on the server's state rather than
            // on whatever the last manual fetch happened to leave behind. Here, in
            // the `Fire` arm only: a tick we are about to skip must not touch the
            // network. Never a fast-forward — nobody is around to arbitrate one.
            //
            // It holds the (serialized) tick for up to `FETCH_TIMEOUT`, exactly as
            // the guard subprocess above already does and for the same reason: a
            // fire is read-decide-act, and the act needs the decision's inputs to
            // still be true. `next_fire_at` was written before this match, so a slow
            // fetch delays THIS fire without ever shifting the schedule.
            let source_fetch_error = fetch_before_trigger_cut(&trigger_fetch_targets(
                trigger.target_repo.as_deref(),
                trigger.target_repos.as_deref(),
            ))
            .await;
            let req = CreateRunRequest {
                pipeline: trigger.pipeline_id.clone(),
                input,
                variables: trigger_variables(trigger),
                pipeline_id: Some(trigger.pipeline_id.clone()),
                target_repo: trigger.target_repo.clone(),
                // Stored as raw JSON TEXT so `trigger_store` stays decoupled from lib
                // types; a blank/absent/malformed value defers to a mono-repo fire.
                target_repos: trigger
                    .target_repos
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .and_then(|s| serde_json::from_str::<Vec<TargetRepoInput>>(s).ok())
                    .unwrap_or_default(),
                source_branch: trigger.source_branch.clone(),
                name: None,
                triggered_by: Some(trigger.id.clone()),
                // A Run has a single origin, so the Trigger's stored mode IS the
                // request's explicit tier. `parse` is syntactic, so an unknown token
                // reads as a profile NAME and the chokepoint 400s it by name; that
                // message becomes this fire's `FireRecord` reason, visibly red in the
                // Trigger history (ADR-0031 §7).
                sandbox: trigger
                    .sandbox
                    .as_deref()
                    .and_then(event_log::SandboxMode::parse),
                // The Trigger carries the harness IN its Run template — there is no
                // separate Trigger tier. Both a cron tick and a "Run now" route through
                // here, so they freeze the SAME harness.
                harness: trigger.harness.clone(),
                agent_choice: trigger.agent_choice.clone(),
                // #669: the Trigger's skills ARE the fired Run's Run tier, same as
                // `harness` — a cron tick and a "Run now" freeze the SAME list.
                skills: trigger.skills.clone(),
                // `Some(b)` wins the chokepoint resolution unconditionally, so a fire
                // with `auto_name=false` keeps a stable per-id name.
                auto_name: Some(trigger.auto_name),
                // No Trigger tier for `auto_fail` (ADR-0049): a fire states no
                // run-level preference, so `pdo fail` resolves through project/instance.
                auto_fail: None,
                provisioning: provisioning::ProvisioningRules::default(),
                source_fetch_error,
            };
            let record = match create_run_inner(state, req, Vec::new(), None).await {
                Ok(run_id) => trigger_store::FireRecord {
                    outcome: "fired".to_string(),
                    reason: None,
                    run_id: Some(run_id),
                    guard_stdout: None,
                    guard_stderr: None,
                    guard_exit_code: None,
                    source: Some(source.as_str().to_string()),
                },
                Err((_, body)) => {
                    let reason = body
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("run creation failed")
                        .to_string();
                    trigger_store::FireRecord {
                        outcome: "error".to_string(),
                        reason: Some(reason),
                        run_id: None,
                        guard_stdout: None,
                        guard_stderr: None,
                        guard_exit_code: None,
                        source: Some(source.as_str().to_string()),
                    }
                }
            };
            record_and_broadcast_fire(state, &trigger.id, &record).await;
            Some(record)
        }
        fire_decision::FireDecision::Skip { .. } | fire_decision::FireDecision::Reject { .. } => {
            if let Some(record) = plan.record {
                record_and_broadcast_fire(state, &trigger.id, &record).await;
                Some(record)
            } else {
                None
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
    let _ = state.bus_tx.send(serde_json::json!({
        "type": "trigger_fired",
        "trigger_id": trigger_id,
        "outcome": record.outcome,
        "run_id": record.run_id,
    }));
}

// --- API handlers ---

/// One attachment of a `POST /runs` multipart body (#779): an image (the
/// `images` field, image extension required) or any other file (the `files`
/// field). Both land in the child's `_input/` beside `output.md`; the entry
/// node sees them under `## Input Images` / `## Input Files`.
struct InputFile {
    filename: String,
    data: Vec<u8>,
    /// Classified by EXTENSION, whatever field carried the file — so a `.png`
    /// sent under `files` is still discovered as an image at spawn.
    is_image: bool,
}

/// Built-in per-run attachment budget (images + files, bytes counted on the
/// multipart parts) when neither the stored setting nor
/// [`MAX_ATTACHMENTS_MB_ENV`] speaks: 50 MB. The modal reads the SAME value
/// from `GET /settings` so its client-side gate and the daemon's agree (#779).
pub(crate) const MAX_ATTACHMENTS_MB_DEFAULT: i64 = 50;
/// Env seam of the attachment budget, read only inside
/// [`resolve_max_attachments_mb`].
pub(crate) const MAX_ATTACHMENTS_MB_ENV: &str = "PDO_MAX_ATTACHMENTS_MB";

/// The attachment budget in MB, `stored → env → default(50)`. A stored `0` or
/// negative is impossible (`PUT /settings` refuses `< 1`), an unparsable env
/// falls through to the default.
pub(crate) fn resolve_max_attachments_mb(stored: Option<i64>) -> i64 {
    if let Some(n) = stored.filter(|&n| n >= 1) {
        return n;
    }
    if let Some(n) = env_max_attachments_mb() {
        return n;
    }
    MAX_ATTACHMENTS_MB_DEFAULT
}

pub(crate) fn env_max_attachments_mb() -> Option<i64> {
    std::env::var(MAX_ATTACHMENTS_MB_ENV)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n >= 1)
}

/// The prompt file every Run writes into `_input/` — an attachment may not
/// take its name, it would clobber the user prompt.
const INPUT_PROMPT_FILENAME: &str = "output.md";

/// Sanitize the filename of a `files` attachment: keep the last path component
/// only (no traversal), refuse empty / dot names and a collision with the
/// prompt file. Every extension is accepted — the per-run size budget is the
/// guard, not a type list (#779).
fn sanitize_attachment_filename(raw: &str) -> Result<String, String> {
    let name = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("invalid attachment filename: {raw:?}"));
    }
    if name == INPUT_PROMPT_FILENAME {
        return Err(format!(
            "attachment `{name}` is reserved for the run prompt; rename the file"
        ));
    }
    Ok(name.to_string())
}

fn has_image_extension(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// A refused multipart body: the status the client gets and the sentence that
/// names why. `PAYLOAD_TOO_LARGE` for the budget, `BAD_REQUEST` otherwise.
struct MultipartRefusal {
    status: StatusCode,
    message: String,
}

impl From<String> for MultipartRefusal {
    fn from(message: String) -> Self {
        MultipartRefusal {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
}

impl From<&str> for MultipartRefusal {
    fn from(message: &str) -> Self {
        MultipartRefusal::from(message.to_string())
    }
}

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

/// The 400 sentence for a multipart body that ended before its closing
/// boundary (#839). Names the layer (transport, not the form), the attachment
/// being read when it happened, and how much had arrived, so a remote user
/// can tell a proxy body limit or a dropped connection from a client bug.
fn multipart_cut_message(
    last_field: Option<&str>,
    filename: Option<&str>,
    received_bytes: u64,
    cause: &str,
) -> String {
    let received = format_attachment_bytes(received_bytes);
    let where_ = match (filename, last_field) {
        (Some(name), _) if !name.is_empty() => format!("while reading attachment `{name}`"),
        (_, Some(field)) if !field.is_empty() => format!("after field `{field}`"),
        _ => "before any field".to_string(),
    };
    format!(
        "upload interrupted {where_} ({received} of attachments received): the connection was \
         cut or a proxy in front of PDO truncated the multipart body ({cause}). Check the \
         network, the reverse proxy body limit (nginx `client_max_body_size`) and \
         `max_attachments_mb` in Settings"
    )
}

/// The 400 sentence for a field that could not be read (#839). axum's
/// `MultipartError` displays a flat "Error parsing `multipart/form-data`
/// request"; the multer source underneath says whether the stream simply
/// ended (a cut upload — named as such, with the field or attachment in
/// progress) or the field itself was malformed (a plain `bad field`).
fn multipart_field_error(
    field: &str,
    filename: Option<&str>,
    received_bytes: u64,
    err: &axum::extract::multipart::MultipartError,
) -> String {
    let cause = std::error::Error::source(err)
        .map(|s| s.to_string())
        .unwrap_or_else(|| err.to_string());
    let lower = cause.to_lowercase();
    let cut = lower.contains("incomplete")
        || lower.contains("failed to read stream")
        || lower.contains("failed to read field complete headers");
    if cut {
        multipart_cut_message(Some(field), filename, received_bytes, &cause)
    } else {
        match filename {
            Some(name) if !name.is_empty() => {
                format!("failed to read attachment `{name}`: {cause}")
            }
            _ => format!("bad field {field}: {cause}"),
        }
    }
}

/// `18 KB`, `1.4 MB` — the size wording of upload errors (#839).
fn format_attachment_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

async fn parse_multipart_create_run(
    mut multipart: Multipart,
    max_attachment_bytes: u64,
) -> std::result::Result<(CreateRunRequest, Vec<InputFile>), MultipartRefusal> {
    let mut pipeline = None;
    let mut input = None;
    let mut variables: HashMap<String, serde_yaml::Value> = HashMap::new();
    let mut pipeline_id = None;
    let mut target_repo = None;
    let mut target_repos: Vec<TargetRepoInput> = Vec::new();
    let mut source_branch = None;
    let mut name = None;
    let mut sandbox: Option<event_log::SandboxMode> = None;
    let mut harness: Option<String> = None;
    let mut agent_choice: Option<pdo_harness::agent_choice::AgentChoice> = None;
    let mut skills: Vec<crate::skill_selection::SkillRef> = Vec::new();
    let mut auto_name: Option<bool> = None;
    let mut auto_fail: Option<bool> = None;
    let mut provisioning = provisioning::ProvisioningRules::default();
    let mut attachments: Vec<InputFile> = Vec::new();
    let mut attachment_bytes: u64 = 0;

    // The budget is enforced on the ATTACHMENT parts (what the user chose),
    // never on the text fields — a long prompt does not eat the file budget.
    // Refused at the first part that crosses the line, so a runaway upload is
    // cut without buffering the whole body.
    let mut push_attachment = |filename: String,
                               data: Vec<u8>,
                               attachments: &mut Vec<InputFile>|
     -> std::result::Result<(), MultipartRefusal> {
        attachment_bytes += data.len() as u64;
        if attachment_bytes > max_attachment_bytes {
            return Err(MultipartRefusal {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                message: format!(
                    "attachments exceed the per-run limit of {} MB (at `{filename}`); \
                     raise `max_attachments_mb` in Settings or attach less",
                    max_attachment_bytes / (1024 * 1024)
                ),
            });
        }
        // #779: a duplicate name never overwrites — today the second silently
        // replaced the first. Named 400 so the caller can rename or drop one.
        if attachments.iter().any(|a| a.filename == filename) {
            return Err(MultipartRefusal::from(format!(
                "duplicate attachment filename `{filename}`: each attachment needs a distinct name"
            )));
        }
        let is_image = has_image_extension(&filename);
        attachments.push(InputFile {
            filename,
            data,
            is_image,
        });
        Ok(())
    };

    // #839: bytes of attachment parts fully received so far and the last field
    // seen — what a truncation error names, so a cut upload reads as a cut
    // upload and never as "missing field: input".
    let mut received_bytes: u64 = 0;
    let mut last_field: Option<String> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            // A transport error mid-stream (connection reset, proxy dropped the
            // body) used to end the loop silently and fall through to a
            // misleading 400 "missing field". Named for what it is.
            Err(e) => {
                let cause = std::error::Error::source(&e)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.to_string());
                return Err(MultipartRefusal::from(multipart_cut_message(
                    last_field.as_deref(),
                    None,
                    received_bytes,
                    &cause,
                )));
            }
        };
        let field_name = field.name().unwrap_or("").to_string();
        last_field = Some(field_name.clone());
        match field_name.as_str() {
            "pipeline" => {
                pipeline =
                    Some(field.text().await.map_err(|e| {
                        multipart_field_error("pipeline", None, received_bytes, &e)
                    })?);
            }
            "input" => {
                input = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| multipart_field_error("input", None, received_bytes, &e))?,
                );
            }
            "variables" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("variables", None, received_bytes, &e))?;
                if !text.is_empty() {
                    variables = serde_json::from_str(&text)
                        .map_err(|e| format!("invalid variables JSON: {e}"))?;
                }
            }
            "pipeline_id" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("pipeline_id", None, received_bytes, &e))?;
                if !v.is_empty() {
                    pipeline_id = Some(v);
                }
            }
            "target_repo" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("target_repo", None, received_bytes, &e))?;
                if !v.is_empty() {
                    target_repo = Some(v);
                }
            }
            "target_repos" => {
                // A JSON array of {repo, base_branch?}. Empty → mono-repo; malformed
                // JSON is a 400, like `variables`.
                let text = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("target_repos", None, received_bytes, &e))?;
                if !text.is_empty() {
                    target_repos = serde_json::from_str(&text)
                        .map_err(|e| format!("invalid target_repos JSON: {e}"))?;
                }
            }
            "source_branch" => {
                let v = field.text().await.map_err(|e| {
                    multipart_field_error("source_branch", None, received_bytes, &e)
                })?;
                if !v.is_empty() {
                    source_branch = Some(v);
                }
            }
            "name" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("name", None, received_bytes, &e))?;
                if !v.is_empty() {
                    name = Some(v);
                }
            }
            "sandbox" => {
                // `parse` is syntactic, so a non-`off` token becomes `Profile(name)`
                // and an unknown NAME is a hard 400 at the chokepoint rather than a
                // silent demotion to the instance default (ADR-0031 §7).
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("sandbox", None, received_bytes, &e))?;
                if !v.is_empty() {
                    sandbox = event_log::SandboxMode::parse(&v);
                }
            }
            "harness" => {
                // No validation (ADR-0045).
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("harness", None, received_bytes, &e))?;
                if !v.trim().is_empty() {
                    harness = Some(v);
                }
            }
            "agent_choice" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("agent_choice", None, received_bytes, &e))?;
                if !value.trim().is_empty() {
                    agent_choice = Some(
                        serde_json::from_str(&value)
                            .map_err(|e| format!("invalid agent_choice JSON: {e}"))?,
                    );
                }
            }
            "skills" => {
                // #669: a JSON list of `{id, name}`, mirrored from the JSON path so a
                // Run created WITH images keeps its skills.
                let value = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("skills", None, received_bytes, &e))?;
                if !value.trim().is_empty() {
                    skills = serde_json::from_str(&value)
                        .map_err(|e| format!("invalid skills JSON: {e}"))?;
                }
            }
            "auto_name" => {
                // An unrecognised token leaves `None`, deferring to the chokepoint's
                // name-presence resolution.
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("auto_name", None, received_bytes, &e))?;
                if !v.is_empty() {
                    auto_name = stale_detector::parse_bool_setting(&v);
                }
            }
            "auto_fail" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("auto_fail", None, received_bytes, &e))?;
                if !v.is_empty() {
                    auto_fail = stale_detector::parse_bool_setting(&v);
                }
            }
            "provisioning" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("provisioning", None, received_bytes, &e))?;
                provisioning =
                    serde_json::from_str(&v).map_err(|e| format!("bad field provisioning: {e}"))?;
            }
            "images" => {
                let raw_filename = field.file_name().unwrap_or("image.png").to_string();
                let data = field.bytes().await.map_err(|e| {
                    multipart_field_error("images", Some(&raw_filename), received_bytes, &e)
                })?;
                received_bytes += data.len() as u64;
                if data.is_empty() {
                    continue;
                }
                let filename = sanitize_image_filename(&raw_filename)
                    .ok_or_else(|| format!("unsupported image type: {raw_filename}"))?;
                push_attachment(filename, data.to_vec(), &mut attachments)?;
            }
            "files" => {
                // #779: any file. Extension-free, budget-guarded, name-checked.
                let raw_filename = field.file_name().unwrap_or("").to_string();
                let data = field.bytes().await.map_err(|e| {
                    multipart_field_error("files", Some(&raw_filename), received_bytes, &e)
                })?;
                received_bytes += data.len() as u64;
                if data.is_empty() {
                    continue;
                }
                let filename = sanitize_attachment_filename(&raw_filename)?;
                push_attachment(filename, data.to_vec(), &mut attachments)?;
            }
            // An unknown form field is a client mistake, named — never silently
            // dropped. `images` and `files` are the multipart-only fields; the
            // rest mirror `CREATE_RUN_FIELDS`.
            other => {
                return Err(MultipartRefusal::from(format!(
                    "unknown field `{other}` in POST /runs multipart body; accepted fields: {}, images, files",
                    CREATE_RUN_FIELDS.join(", ")
                )));
            }
        }
    }

    let req = CreateRunRequest {
        pipeline: pipeline.ok_or("missing field: pipeline")?,
        input: input.ok_or("missing field: input")?,
        variables,
        pipeline_id,
        target_repo,
        target_repos,
        source_branch,
        name,
        triggered_by: None,
        sandbox,
        harness,
        agent_choice,
        skills,
        auto_name,
        auto_fail,
        provisioning,
        // A multipart `POST /runs` is a manual launch: the form already fetched
        // before it opened, so nothing is refreshed here (#804, ADR-0070 §3).
        source_fetch_error: None,
    };
    Ok((req, attachments))
}

use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

async fn create_run(State(state): State<Arc<AppState>>, req: axum::extract::Request) -> Response {
    // ADR-0064: read the session claim BEFORE the body is consumed — identity
    // travels in headers, never in the body (whose parent fields are refused).
    let session = session_claim_from_headers(req.headers());
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let (parsed_req, attachments) = if content_type.starts_with("multipart/form-data") {
        // #779: the body limit of this route is the attachment budget, read
        // FRESH from the instance setting (the route has no static
        // `DefaultBodyLimit`, see the router). Text fields ride on top of it.
        let max_mb = resolve_max_attachments_mb(
            instance_config::get(&state.db)
                .await
                .ok()
                .and_then(|cfg| cfg.max_attachments_mb),
        );
        let max_attachment_bytes = (max_mb as u64) * 1024 * 1024;
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
        match parse_multipart_create_run(multipart, max_attachment_bytes).await {
            Ok(r) => r,
            Err(refusal) => {
                return (
                    refusal.status,
                    Json(serde_json::json!({ "error": refusal.message })),
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
        // Parse to a `Value` FIRST so an unknown top-level field is a named `400`
        // before any effect, not a silently-dropped key.
        let value: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid JSON: {e}") })),
                )
                    .into_response();
            }
        };
        if let Err((status, err)) = reject_unknown_create_run_fields(&value) {
            return (status, Json(err)).into_response();
        }
        let parsed: CreateRunRequest = match serde_json::from_value(value) {
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

    create_run_core(&state, parsed_req, attachments, session).await
}

/// Validate the user input against the pipeline's `prompt_required` flag. A
/// prompt-optional pipeline accepts empty input — its entry node sources its own
/// work — and treats a provided prompt as additional info.
fn validate_run_input(prompt_required: bool, input: &str) -> Result<(), String> {
    if prompt_required && input.trim().is_empty() {
        return Err("this pipeline requires a prompt: input must not be empty".to_string());
    }
    Ok(())
}

/// Deterministic placeholder display name for a run with neither a user-supplied
/// name nor any input to summarise. Derived from the run-id's own timestamp prefix
/// rather than a fresh clock read, so it matches the run-id. run-ids are always
/// `YYYYMMDD-HHMMSS-xxxxxxx` (≥ 23 ASCII chars), so `[..15]` cannot panic on a char
/// boundary.
fn placeholder_run_name(run_id: &str) -> String {
    format!("Untitled run {}", &run_id[..15])
}

/// Decide how the daemon should treat naming for a run at spawn.
///
/// `auto_name == false` ⇒ the manager is never instructed to rename. The `true`
/// branch is gated on `input`, NOT on the pipeline's `prompt_required` flag: an
/// empty input means there is nothing to summarise yet, so the daemon sets a
/// deterministic placeholder and the manager renames best-effort later.
///
/// This hint only governs whether the manager is *told to rename* — the display name
/// itself is chosen by `create_run_inner`. Hence the `false` branch ignoring `name`:
/// a blank name still yields `UserProvided`, a stable placeholder with no rename
/// instruction, which is what the Trigger case needs.
fn run_name_hint(
    auto_name: bool,
    _name: Option<&str>,
    input: &str,
) -> prompt_augmenter::RunNameHint {
    if !auto_name {
        prompt_augmenter::RunNameHint::UserProvided
    } else if input.trim().is_empty() {
        prompt_augmenter::RunNameHint::Placeholder
    } else {
        prompt_augmenter::RunNameHint::DeriveFromInput
    }
}

async fn create_run_core(
    state: &Arc<AppState>,
    req: CreateRunRequest,
    attachments: Vec<InputFile>,
    session: Option<SessionClaim>,
) -> Response {
    match create_run_inner(state, req, attachments, session).await {
        Ok(run_id) => (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response(),
        Err((status, body)) => (status, Json(body)).into_response(),
    }
}

/// The Run-creation logic, returning the new `run_id` on success or a
/// `(status, body)` pair on failure. `create_run_core` wraps this into an HTTP
/// `Response`; the trigger scheduler calls it directly to learn the run id for
/// `triggered_by` provenance — with `session: None`, a fired Run is always a
/// root (a Trigger is not a node session).
async fn create_run_inner(
    state: &Arc<AppState>,
    req: CreateRunRequest,
    attachments: Vec<InputFile>,
    session: Option<SessionClaim>,
) -> Result<String, (StatusCode, serde_json::Value)> {
    // ADR-0064: provenance is mechanical. Verify the session claim FIRST — before
    // any effect (worktree, snapshots, event) — and keep the parent's projected
    // state: the child's project defaults to it when the request names none.
    let parent = match &session {
        Some(claim) => Some(verify_session_claim(&state.run, claim).await?),
        None => None,
    };

    // The target repo is REQUIRED at the creation boundary (ADR-0033): the daemon's
    // own `repo_root` stays its storage root but is never an implicit Run target, or
    // a Run mutates code in a repository nobody named. The read side keeps its
    // fallback for historical runs (`effective_repo_root`).
    //
    // Normalise the multi-repo shape down to the scalar here, so the ENTIRE
    // downstream primary path (worktree, merge-back, cost) stays unchanged and the
    // array only ever ADDS secondaries.
    //
    // One exception, itself mechanical (ADR-0064): a Run created from a node
    // session defaults to the PARENT's project when the request names none — an
    // orchestrated child is an ordinary run whose common case inherits the
    // orchestrator's repo. An explicit `target_repo`/`target_repos` always wins.
    let effective_target_repo: Option<String> = req
        .target_repo
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            req.target_repos
                .first()
                .map(|r| r.repo.clone())
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            parent
                .as_ref()
                .map(|p| effective_repo_root(state, p).to_string_lossy().into_owned())
        });
    let run_repo_root = match required_target_repo(effective_target_repo.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
        }
    };

    // The primary's base branch is the scalar `source_branch` when present, else
    // `target_repos[0].base_branch`: row 0 of the multi-repo modal carries the
    // primary's branch in the same field the secondaries use.
    let effective_source_branch: Option<String> = req
        .source_branch
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            req.target_repos
                .first()
                .and_then(|r| r.base_branch.clone())
                .filter(|s| !s.trim().is_empty())
        });
    let source_ref = if let Some(ref branch) = effective_source_branch {
        if let Err(msg) = validate_source_branch(&run_repo_root, branch) {
            return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
        }
        branch.as_str()
    } else {
        "HEAD"
    };

    // Resolve the secondary repos into frozen pins (ADR-0042). Each is validated
    // (absolute + git) and its base ref resolved to a single commit NOW — no fetch,
    // so the base is the operator's LOCAL ref. Must resolve BEFORE
    // `append_event`/`create_worktree`, or "a bad repo → no Run" stops holding.
    let secondary_pins = match resolve_secondary_pins(&run_repo_root, &req.target_repos) {
        Ok(pins) => pins,
        Err(msg) => {
            return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
        }
    };

    let (yaml, pipeline_path) = if let Some(ref pipeline_id) = req.pipeline_id {
        let path = resolve_pipeline_path(&state.repo_root, pipeline_id);
        match std::fs::read_to_string(&path) {
            Ok(yaml) => (yaml, path),
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({ "error": format!("pipeline not found: {pipeline_id}") }),
                ));
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

    // Dangling edge references are info-only warnings at EDIT time (ADR-0001) but
    // hard refusals at launch: a run started over a dangling port is guaranteed to
    // stall silently mid-run.
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

    // A `script` node runs `bash <body>`, and an empty body exits 0 — a silent
    // no-op masquerading as success. Fail loud at launch, before any run exists.
    let empty_scripts: Vec<String> = pipeline
        .nodes
        .iter()
        .filter(|n| n.node_type == pipeline::NodeType::Script)
        .filter(|n| {
            let path = pipeline::canonical_prompt_path(&pipeline_path, &n.id);
            std::fs::read_to_string(&path)
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|n| n.id.clone())
        .collect();
    if !empty_scripts.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            serde_json::json!({
                "error": format!(
                    "cannot launch run: script node(s) have an empty body: {}",
                    empty_scripts.join(", ")
                ),
            }),
        ));
    }

    if let Err(msg) = validate_run_input(pipeline.prompt_required, &req.input) {
        return Err((StatusCode::BAD_REQUEST, serde_json::json!({ "error": msg })));
    }

    let frozen_provisioning = match state
        .workspace
        .scoped_provisioning_rules(
            &run_repo_root.to_string_lossy(),
            req.provisioning.clone(),
            provisioning::ProvisioningRules::default(),
        )
        .await
    {
        Ok(rules) => rules,
        Err(e) => {
            error!("failed to load provisioning recipe: {e}");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "provisioning store unavailable" }),
            ));
        }
    };
    let provisioning_plan = match pdo_workspace::adapters::fs::provisioner::resolve_at_git_ref(
        &run_repo_root,
        &frozen_provisioning,
        source_ref,
    ) {
        Ok(plan) if plan.conflicts.is_empty() => plan,
        Ok(plan) => {
            let conflict = &plan.conflicts[0];
            return Err((
                StatusCode::BAD_REQUEST,
                serde_json::json!({
                    "error": format!(
                        "mode conflict in {:?}: `{}` is declared in multiple modes",
                        conflict.scope, conflict.relative_path
                    )
                }),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": format!("invalid provisioning recipe: {e:#}") }),
            ))
        }
    };

    let run_id = event_log::generate_run_id();

    // Freeze the fork point NOW, from the same local `source_ref` `create_worktree`
    // will cut the run branch from, or a later HEAD move in the shared checkout
    // displaces the LOC/diff base and sweeps in phantom line counts. A resolve
    // failure is non-fatal — it degrades to the `source_branch`/`HEAD` fallback
    // ladder at read time and must never block run creation.
    let fork_sha = match pdo_workspace::adapters::git::worktrees::rev_parse_verified(
        &run_repo_root,
        source_ref,
    ) {
        Ok(sha) => Some(sha),
        Err(e) => {
            warn!("could not resolve fork_sha for run {run_id} from {source_ref}: {e}");
            None
        }
    };

    // CHOKEPOINT: resolve the effective sandbox mode ONCE, here, where the JSON,
    // multipart and trigger-fire paths converge, just before the mode is frozen into
    // `RunStarted`. `req.sandbox` already carries either the explicit run-level
    // choice or the trigger's mode (a Run has a single origin), so the trigger arm
    // stays `None` in production. `default_sandbox` is read FRESH so a
    // `PUT /settings` bites on the next create with no restart.
    let stored_default = instance_config::get(&state.db)
        .await
        .ok()
        .and_then(|c| c.default_sandbox);
    let explicit_tier = req.sandbox.is_some();
    let sandbox = pdo_sandbox::domain::mode::effective_sandbox(
        req.sandbox.clone(),
        None,
        pdo_sandbox::adapters::env::default_sandbox_with(stored_default),
    );

    // #432 (ADR-0031 §6 + §7): resolve the winning tier's staging profile to its entry
    // list HERE, at the same chokepoint, so the name AND the list are frozen together
    // into `RunStarted` — editing (or deleting) the profile afterwards can never rewrite
    // what this Run stages. Only the WINNER is resolved: an unknown name sitting in the
    // instance default is not an error for a Run that explicitly picks another profile,
    // because nothing ever consults it.
    //
    // An unknown name **fails hard**, never falls back to the instance default or to
    // `off`. This is before `append_event(run_started)` and before `create_worktree`, so
    // "no Run is created" holds in the strict sense. A Trigger fire needs ZERO new code:
    // `fire_one_trigger`'s `Err((_, body))` arm already turns this 400's message into a
    // `FireRecord { outcome: "error", reason }` that the history renders red — which is
    // why the wording below is the deliverable, not an afterthought.
    // the profile's env is frozen at the SAME resolve, for the same reason and with
    // the same consequence — editing it afterwards cannot rewrite what this Run's container
    // was created with. One `resolve` feeds both keys; two reads could straddle a
    // concurrent PUT and freeze a list from one revision with an env from another.
    // The image SOURCE is resolved at the same moment, for the same reason: a Run must
    // not have its image swapped under it by a profile edit, or two of its nodes land
    // in two different images (ADR-0031 §6/§9).
    let mut sandbox_env: std::collections::BTreeMap<String, String> = Default::default();
    let mut sandbox_image_src: Option<pdo_sandbox::domain::image::ProfileImage> = None;
    let sandbox_entries: Option<Vec<String>> = match sandbox.profile() {
        None => None,
        Some(name) => match pdo_sandbox::application::profiles::resolve(
            &sandbox_ports::profile_store(&state.db),
            name,
        )
        .await
        {
            Ok(Some(resolved)) => {
                sandbox_env = resolved.env;
                sandbox_image_src = resolved.image;
                Some(resolved.resolved.entries)
            }
            Ok(None) => {
                let error = if explicit_tier {
                    if req.triggered_by.is_some() {
                        format!(
                            "unknown sandbox profile `{name}`: the Trigger's sandbox setting \
                             names a staging profile that does not exist. Recreate it under \
                             Settings → Staging profiles, or point the Trigger at `off`."
                        )
                    } else {
                        format!(
                            "unknown sandbox profile `{name}`: no such staging profile. \
                             Create it under Settings → Staging profiles, or use `off`."
                        )
                    }
                } else {
                    format!(
                        "unknown sandbox profile `{name}`: the instance default sandbox names \
                         a staging profile that does not exist. Fix it under \
                         Settings → Default sandbox."
                    )
                };
                return Err((
                    StatusCode::BAD_REQUEST,
                    serde_json::json!({ "error": error }),
                ));
            }
            Err(e) => {
                error!("failed to resolve sandbox profile `{name}`: {e}");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    serde_json::json!({ "error": "sandbox profile store error" }),
                ));
            }
        },
    };

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
    // Persist the canonical (trimmed, validated) path, never the raw field. Historical
    // events keep their legitimate `null`; the read side resolves those (ADR-0033).
    run_payload["target_repo"] = serde_json::json!(run_repo_root.to_string_lossy());
    // Carry the frozen secondary pins ONLY when there is at least one, so a mono-repo
    // Run's payload stays byte-identical to the historical shape.
    if !secondary_pins.is_empty() {
        run_payload["target_repos"] = serde_json::json!(secondary_pins);
    }
    // Carry the RESOLVED isolation mode only when it is NOT `off`, so `off` payloads
    // stay byte-identical (absence → Off). `sandbox_entries` is a SIBLING key written
    // in the same breath: written together or not at all is what makes the replay
    // table decidable — a `sandbox` with no entries can only mean an older daemon.
    // Don't nest them as `sandbox: {name, entries}`: every historical payload carries
    // `"sandbox": "full"` as a bare string, so both readers would have to accept
    // `String | Object` for ever.
    if !sandbox.is_off() {
        run_payload["sandbox"] = serde_json::json!(sandbox);
        run_payload["sandbox_entries"] = serde_json::json!(sandbox_entries
            .as_ref()
            .expect("a non-off sandbox always resolves an entry list at this chokepoint"));
        // Written ONLY when non-empty, and the asymmetry with `sandbox_entries` is
        // deliberate: an empty entry LIST is a legitimate resolution (`minimal`), so
        // absence there had to mean "older daemon". For the env there is nothing to
        // distinguish — an absent key and an empty map describe the same container.
        if !sandbox_env.is_empty() {
            run_payload["sandbox_env"] = serde_json::json!(sandbox_env);
        }
        // Same asymmetry as `sandbox_env`: an absent key means "the profile posed
        // none", indistinguishable from an older daemon precisely because both resolve
        // the same way, to the sandbox context's `DEFAULT_PROFILE_IMAGE`.
        if let Some(image) = &sandbox_image_src {
            run_payload["sandbox_image"] = serde_json::json!(image);
        }
    }
    if let Some(ref branch) = effective_source_branch {
        run_payload["source_branch"] = serde_json::json!(branch);
    }
    // #804 / ADR-0070 §1: the Trigger fire could not refresh the remotes before
    // cutting. The Run is NOT failed by it — it was cut from the last state known
    // locally, and the Run detail says exactly that instead of leaving someone to
    // wonder why the fork point looks old. Written ONLY on failure, so every other
    // Run's payload keeps its historical shape.
    if let Some(ref err) = req.source_fetch_error {
        run_payload["source_fetch_error"] = serde_json::json!(err);
    }
    // FREEZE the resolved fork point into `RunStarted`. A resolve failure leaves it
    // absent, and the read-side `run_diff_base` ladder recovers via `source_branch`,
    // then `HEAD`.
    if let Some(ref fs) = fork_sha {
        run_payload["fork_sha"] = serde_json::json!(fs);
    }
    // FREEZE the Run's harness choice into `RunStarted`, same immutability posture as
    // `sandbox` above: editing the pipeline or the instance default afterwards must
    // never re-decide a Run in flight. Blank/whitespace is normalised to absent, so
    // `Some("")` can never persist as a harness name. No validation (ADR-0045): an
    // unknown name fails fast at the first node spawn, naming it.
    if let Some(h) = req
        .harness
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        run_payload["harness"] = serde_json::json!(h);
    }
    // FREEZE the Run's `AgentChoice` ATOMICALLY at spawn (ADR-0057 ¶4) — never
    // re-decided by a later profile edit or resume. Written ONLY for an explicit
    // choice with a non-blank payload, so a Run that states none lets `harness` above
    // decide the Run tier.
    let explicit_agent_choice = match &req.agent_choice {
        Some(pdo_harness::agent_choice::AgentChoice::Custom { harness, .. })
            if !harness.trim().is_empty() =>
        {
            req.agent_choice.clone()
        }
        Some(pdo_harness::agent_choice::AgentChoice::Profile { profile_id })
            if !profile_id.trim().is_empty() =>
        {
            req.agent_choice.clone()
        }
        _ => None,
    };
    if let Some(choice) = explicit_agent_choice {
        run_payload["agent_choice"] = serde_json::to_value(choice).unwrap_or_default();
    }
    // FREEZE the Run tier of the skills selection (#669, ADR-0062) — same posture as
    // `harness`/`agent_choice`: set once here, never re-decided by a later edit of
    // the bank or the Trigger. Written ONLY when non-empty, so a Run that selects
    // none keeps the historical payload shape. Not validated against the bank: an
    // unknown id is a spawn-time WARNING, never a refusal to launch (#669 AC).
    let run_skills = crate::skill_selection::normalise(req.skills.clone());
    if !run_skills.is_empty() {
        run_payload["skills"] = serde_json::to_value(&run_skills).unwrap_or_default();
    }
    // #672 / ADR-0062: FREEZE the resolved instance + Projet + Run selection (ids,
    // bank names, origin tiers) — the base every node spawn of this Run adds its own
    // tier to. The bank's names are read once here; a later rename or edit of the
    // bank never reaches this Run (its content is snapshotted below). Written only
    // when non-empty so a Run selecting nothing keeps the historical payload shape.
    // An id the bank does not know is a warning on the Run, never a refusal.
    let frozen_skills = {
        let instance_skills = instance_config::get(&state.db)
            .await
            .map(|c| c.skills)
            .unwrap_or_default();
        let project_skills = project_skills_for_path(&state.run, &run_repo_root.to_string_lossy())
            .await
            .unwrap_or_default();
        let bank = state.skills.bank_names().await.unwrap_or_default();
        let tiers = crate::skill_selection::SkillTiers {
            instance: Some(&instance_skills),
            project: Some(&project_skills),
            run: Some(&run_skills),
            node: None,
        };
        let resolved = crate::skill_selection::resolve(&tiers, &bank);
        for missing in &resolved.missing {
            warn!(
                "run {run_id}: skill '{}' ({}) selected at {:?} is no longer in the bank; \
                 the Run proceeds without it",
                missing.name, missing.id, missing.tiers
            );
        }
        if !resolved.missing.is_empty() {
            run_payload["missing_skills"] =
                serde_json::to_value(&resolved.missing).unwrap_or_default();
        }
        if !resolved.skills.is_empty() {
            run_payload[crate::skill_delivery::RUN_STARTED_FROZEN_KEY] =
                serde_json::to_value(&resolved.skills).unwrap_or_default();
        }
        resolved.skills
    };
    // FREEZE the Run's `auto_fail` choice (ADR-0049), same immutability posture as
    // `harness`. Absent ⇒ resolve through the project / instance tiers.
    if let Some(af) = req.auto_fail {
        run_payload["auto_fail"] = serde_json::json!(af);
    }
    // Resolved ONCE here, at the same chokepoint as the sandbox default, and read
    // FRESH so a `PUT /settings` bites on the next create. Never cache it at boot
    // (the reaper-TTL trap of ADR-0015).
    //
    // The back-compat ordering is load-bearing: an omitted flag WITH a supplied `name`
    // must keep that name, so a caller passing `name` without the flag is unaffected.
    // Only an omitted flag AND no name consults the instance default.
    let auto_name = match req.auto_name {
        Some(b) => b,
        None => {
            if req.name.as_deref().is_some_and(|n| !n.trim().is_empty()) {
                false
            } else {
                let stored = instance_config::get(&state.db)
                    .await
                    .ok()
                    .and_then(|c| c.default_auto_name);
                prompt_augmenter::default_auto_name_with(stored)
            }
        }
    };

    // Shared between the payload write here and the manager spawn below, so the two
    // can never disagree.
    let name_hint = run_name_hint(auto_name, req.name.as_deref(), &req.input);
    let display_name: Option<String> = match name_hint {
        // With no supplied name, fall back to the stable placeholder so a Trigger with
        // autonomy off does not leave every fire sharing one blank name — WITHOUT
        // re-arming the rename instruction, which lives in the hint.
        prompt_augmenter::RunNameHint::UserProvided => Some(match req.name.as_deref() {
            Some(n) if !n.trim().is_empty() => n.to_string(),
            _ => placeholder_run_name(&run_id),
        }),
        prompt_augmenter::RunNameHint::Placeholder => Some(placeholder_run_name(&run_id)),
        prompt_augmenter::RunNameHint::DeriveFromInput => None,
    };
    if let Some(name) = &display_name {
        run_payload["name"] = serde_json::json!(name);
    }
    if let Some(ref trigger_id) = req.triggered_by {
        if !trigger_id.is_empty() {
            run_payload["triggered_by"] = serde_json::json!(trigger_id);
        }
    }
    // ADR-0064: FREEZE the mechanical parent provenance — decided by the daemon
    // from the verified session claim, never read from the body (which refuses
    // any parent key outright). Written ONLY when the create came from a live
    // node session, so an out-of-session create keeps the historical root shape.
    if let (Some(parent), Some(claim)) = (&parent, &session) {
        run_payload["parent_run_id"] = serde_json::json!(parent.run_id);
        run_payload["parent_node_id"] = serde_json::json!(claim.node_id);
    }
    // Carry the library pipeline id so aggregated "by pipeline" stats survive a
    // pipeline rename. The consumer falls back to `pipeline_name` when absent.
    if let Some(ref pid) = req.pipeline_id {
        if !pid.is_empty() {
            run_payload["pipeline_id"] = serde_json::json!(pid);
        }
    }
    // Upload order is kept: what the user sees on the Start node is what they
    // attached, in that order.
    let image_names: Vec<&str> = attachments
        .iter()
        .filter(|a| a.is_image)
        .map(|a| a.filename.as_str())
        .collect();
    if !image_names.is_empty() {
        run_payload["image_filenames"] = serde_json::json!(image_names);
    }
    let file_names: Vec<&str> = attachments
        .iter()
        .filter(|a| !a.is_image)
        .map(|a| a.filename.as_str())
        .collect();
    if !file_names.is_empty() {
        run_payload["file_filenames"] = serde_json::json!(file_names);
    }
    if frozen_provisioning
        .iter()
        .any(|scoped| !scoped.rules.is_empty())
    {
        run_payload["provisioning_rules"] = serde_json::json!(frozen_provisioning);
    }

    // `mut`: the skill delivery below (#672) appends `skipped_skills` once the
    // worktree exists, before the event is appended.
    let mut run_started = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(run_payload),
    };

    // Artifacts live under <target_repo>/.pdo/runs/<run-id>/.
    let worktree_dir = worktree_dir_for_run(&run_repo_root, &run_id);
    let branch_name = format!("pdo/run-{run_id}");

    if let Err(e) = create_worktree(&run_repo_root, &worktree_dir, &branch_name, source_ref) {
        error!("failed to create worktree: {e}");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": format!("worktree creation failed: {e}") }),
        ));
    }

    if let Err(e) = pdo_workspace::adapters::fs::provisioner::provision_missing(
        &run_repo_root,
        &worktree_dir,
        &provisioning_plan,
    ) {
        reap_orphan_run_worktree(&run_repo_root, &worktree_dir, &branch_name);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "error": format!(
                    "Run not created. Provisioning failed: {e:#}. No node was spawned."
                )
            }),
        ));
    }

    // #672 / ADR-0062: snapshot the frozen selection's content under the Run dir
    // (outside the worktree), then deliver it into the Run worktree — the tree the
    // non-isolated nodes, the Pipeline Manager and the run shell work in. Paths are
    // excluded per skill in the target repo's `info/exclude` and recorded in the
    // worktree's provisioning manifest, so no skill ever reaches the Run branch. A
    // versioned homonym is skipped and reported on the Run; an I/O failure refuses
    // the Run like a provisioning failure (never a partial environment).
    let skill_delivery =
        state
            .skills
            .deliver(&worktree_dir, &run_repo_root, &run_id, &frozen_skills);
    match skill_delivery {
        Ok(skipped) => {
            for skipped in &skipped {
                warn!(
                    "run {run_id}: skill '{}' not delivered: {}",
                    skipped.name, skipped.reason
                );
            }
            if !skipped.is_empty() {
                if let Some(payload) = run_started.payload.as_mut() {
                    payload["skipped_skills"] = serde_json::to_value(&skipped).unwrap_or_default();
                }
            }
        }
        Err(e) => {
            let _ = state.skills.withdraw(&run_repo_root, &run_id);
            reap_orphan_run_worktree(&run_repo_root, &worktree_dir, &branch_name);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!(
                        "Run not created. Skill delivery failed: {e:#}. No node was spawned."
                    )
                }),
            ));
        }
    }

    // Materialise each secondary as a detached, pinned snapshot beside
    // `worktree/`/`nodes/`. At Run start, NOT per-node spawn, so the reap surface
    // stays minimal; under `run_repo_root`, so the sandbox sees them at an identical
    // path with no new mount. A failure fails the Run loud before `RunStarted`;
    // otherwise a missing snapshot would silently starve every node reading it.
    let mut created_secondary_snapshots: Vec<(String, PathBuf)> = Vec::new();
    for pin in &secondary_pins {
        let dest = secondary_snapshot_path(&run_repo_root, &run_id, &pin.alias);
        if let Err(e) = create_secondary_snapshot(Path::new(&pin.repo), &dest, &pin.sha) {
            error!("failed to create secondary snapshot for {}: {e}", pin.repo);
            for (repo, snapshot) in &created_secondary_snapshots {
                remove_secondary_snapshot(Path::new(repo), snapshot);
            }
            reap_orphan_run_worktree(&run_repo_root, &worktree_dir, &branch_name);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!("secondary snapshot creation failed for {}: {e}", pin.repo)
                }),
            ));
        }
        created_secondary_snapshots.push((pin.repo.clone(), dest));
    }

    if let Err(e) = append_event(state, &run_started).await {
        error!("failed to append run_started: {e}");
        for (repo, snapshot) in &created_secondary_snapshots {
            remove_secondary_snapshot(Path::new(repo), snapshot);
        }
        reap_orphan_run_worktree(&run_repo_root, &worktree_dir, &branch_name);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "event log error" }),
        ));
    }

    if let Err(e) = copy_pipeline_to_run(&run_repo_root, &pipeline_path, &run_id) {
        error!("failed to copy pipeline to run dir: {e}");
    } else if run_repo_root == state.repo_root {
        // Run dirs are watched individually and non-recursively, so each new one must
        // be registered. Runs in another target repo stay unwatched — the watcher's
        // run-id detection is rooted at `state.repo_root`.
        let mut guard = state.run_watcher.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(debouncer) = guard.as_mut() {
            if let Some(run_dir) = run_scoped_pipeline_path(&run_repo_root, &run_id).parent() {
                pipeline_watcher::watch_run_dir(debouncer, run_dir);
            }
        }
    }

    // Directory-based artifact (ADR-0010).
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let input_dir = artifacts_dir.join("_input");
    if let Err(e) = std::fs::create_dir_all(&input_dir) {
        error!("failed to create _input artifact dir: {e}");
    }
    let input_path = input_dir.join("output.md");
    if let Err(e) = std::fs::write(&input_path, &req.input) {
        error!("failed to write _input/output.md: {e}");
    }

    for attachment in &attachments {
        let path = input_dir.join(&attachment.filename);
        if let Err(e) = std::fs::write(&path, &attachment.data) {
            error!("failed to write attachment {}: {e}", attachment.filename);
        }
    }

    // Manager on demand: the automatic spawn is GATED on the instance setting
    // (`stored → env PDO_MANAGER_ENABLED → false`), read FRESH at the edge like
    // every instance knob. It is NOT a permission — the Manager tab's manual
    // start is always available, which is what keeps the setting honest — and it
    // applies to future Runs only: a live Run is unaffected by a mid-flight
    // toggle.
    let manager_enabled = instance_config::resolve_manager_enabled(
        instance_config::get(&state.db)
            .await
            .ok()
            .and_then(|cfg| cfg.manager_enabled),
    );

    if sandbox.is_off() {
        // Host path — inline, no docker.
        spawn_ready_after_event(&state.run, &run_id).await;
        if manager_enabled {
            spawn_manager_session(state, &run_id, &worktree_dir, name_hint, false).await;
        }
    } else {
        // Eager fail-fast prep on a detached task: the 201 must not block on a
        // first-run `docker build` (ADR-0023). Image + container + staging are
        // guaranteed BEFORE the first node/manager spawn, and any Docker
        // unavailability is a `RunFailed` with ZERO host spawn — never a silent host
        // fallback for a sandboxed Run's work.
        let task_state = state.clone();
        let task_run_id = run_id.clone();
        let task_worktree = worktree_dir.clone();
        tokio::spawn(async move {
            // Emitted at the HEAD of the detached task, before the long
            // `ensure_ready`, so the UI shows "préparation du sandbox…" instead of a
            // seemingly-stuck Run.
            emit_run_event(
                &task_state.run,
                &task_run_id,
                event_log::EventKind::SandboxPrepStarted,
                None,
            )
            .await;
            // A vanished/half-projected state → fail loud.
            let ctx = match reload_run_state(&task_state, &task_run_id).await {
                Some((_, rs)) => {
                    match run_sandbox::context_from_state(&task_state.run, &rs).await {
                        Ok(ctx) => ctx,
                        Err(e) => {
                            fail_run_sandbox_prep(
                                &task_state.run,
                                &task_run_id,
                                &format!("sandbox prep failed: {e:#}"),
                            )
                            .await;
                            return;
                        }
                    }
                }
                None => {
                    fail_run_sandbox_prep(
                        &task_state.run,
                        &task_run_id,
                        "sandbox prep failed: run state disappeared before prep",
                    )
                    .await;
                    return;
                }
            };
            // `ensure_ready` blocks on docker build/probe, so it must run under
            // `spawn_blocking` — which also isolates its panic into a `JoinError`.
            match tokio::task::spawn_blocking(move || ctx.ensure_ready()).await {
                Ok(Ok(())) => {
                    // The Run may have gone terminal WHILE the prep ran. Emitting
                    // `SandboxPrepReady` and spawning into a dead Run would leave a
                    // container nobody can exec into, so abandon instead. Idempotent —
                    // a later `resume_run` re-runs `ensure_ready`.
                    if abandon_prep_if_run_is_terminal(&task_state.run, &task_run_id).await {
                        return;
                    }
                    // Emitted just before the spawn so the prep banner clears exactly
                    // when work can start. This pair — event, then advance — is the
                    // single REPLAY point for every spawn deferred while the prep ran.
                    mark_sandbox_prep_ready(&task_state.run, &task_run_id).await;
                    spawn_ready_after_event(&task_state.run, &task_run_id).await;
                    if manager_enabled {
                        spawn_manager_session(
                            &task_state,
                            &task_run_id,
                            &task_worktree,
                            name_hint,
                            true,
                        )
                        .await;
                    }
                }
                Ok(Err(e)) => {
                    fail_run_sandbox_prep(
                        &task_state.run,
                        &task_run_id,
                        &format!("sandbox prep failed: {e:#}"),
                    )
                    .await;
                }
                Err(join_err) => {
                    fail_run_sandbox_prep(
                        &task_state.run,
                        &task_run_id,
                        &format!("sandbox prep panicked: {join_err}"),
                    )
                    .await;
                }
            }
        });
    }

    info!("Run {run_id} started for pipeline {}", pipeline.name);

    Ok(run_id)
}

/// Spawn the Run's Pipeline Manager session.
///
/// **Its only reservation is `RunStarted`** (ADR-0038): the orphan sweep resolves a
/// `pdo-mgr-<run>` session against the *run*, not a node, so a manager spawned for a
/// run whose `RunStarted` never landed is an orphan the sweep will legitimately kill.
/// Keep every spawn site downstream of a successful `RunStarted` append.
async fn spawn_manager_session(
    state: &AppState,
    run_id: &str,
    worktree_dir: &std::path::Path,
    name_hint: prompt_augmenter::RunNameHint,
    sandboxed: bool,
) {
    // The URL must be reachable from the side this manager runs on. A sandboxed
    // manager execs into the Run's container, where `localhost` is the container, so a
    // host URL makes every `curl` of its own preamble hit nothing and the manager
    // report the daemon dead.
    let daemon_url = pdo_sandbox::domain::container::daemon_url(state.port, sandboxed);

    let static_prompt = std::fs::read_to_string(
        state
            .repo_root
            .join("prompts")
            .join("builtin")
            .join("manager.md"),
    )
    .unwrap_or_default();

    let full_prompt =
        prompt_augmenter::build_manager_prompt(run_id, &daemon_url, &static_prompt, name_hint);

    let session_name = pdo_session::domain::names::manager_session_name(run_id);
    // The manager follows the harness OF THE RUN — `Run → instance → floor`, no node
    // tier and deliberately no model/effort — so "this Run runs on X" holds with no
    // exception … unless a stored `manager_profile` pin says otherwise (applied
    // just below). An unknown name falls back to `claude` with a warning rather than
    // failing the Run: the first NODE spawn is where an unknown harness fails fast
    // (ADR-0037), and the manager is a best-effort assist that must not wedge the Run.
    let run_state = reload_run_state(state, run_id)
        .await
        .map(|(_, state)| state);
    let config = instance_config::get(&state.db).await.ok();
    let profiles = agent_profile::snapshot(&state.db).await.unwrap_or_default();
    let manager_agent = pdo_harness::agent_choice::resolve_infra(
        run_state.as_ref().and_then(|run| run.agent_choice.as_ref()),
        run_state.as_ref().and_then(|run| run.harness.as_deref()),
        config.as_ref().and_then(|cfg| cfg.agent_choice.as_ref()),
        config
            .as_ref()
            .and_then(|cfg| cfg.default_harness.as_deref()),
        &profiles,
        agent_profile::DEFAULT_PROFILE_ID,
    );
    // The stored `manager_profile` pin, applied AFTER « Follow the Run » so a pin
    // fully wins (design §2.2): the profile's harness · model · effort replace the
    // Run-tier combo entirely — a pin that only replaced the floor would almost
    // never fire, since the Run tier always resolves. A pin naming a profile that
    // was deleted (or renamed) AFTER registration warns and falls back to « Follow
    // the Run » — loud, and the stored value is never rewritten (#432 tombstone).
    let stored_pin = config
        .as_ref()
        .and_then(|cfg| cfg.manager_profile.as_deref())
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let found = match stored_pin {
        Some(pin) => agent_profile::find_by_name_ci(&state.db, pin)
            .await
            .map_err(|e| e.to_string()),
        None => Ok(None),
    };
    let (manager_combo, pin_warning) =
        apply_manager_profile_pin(stored_pin, found, manager_agent.combo.clone());
    if let Some(warning) = &pin_warning {
        warn!("manager for run {run_id}: {warning}");
    }
    let manager_harness_name = manager_combo.harness.clone();
    // Resolve against the DISK TIER, through the same registry the node spawns use,
    // so "this Run runs on X" holds even when X is a user-declared harness.
    let harness_home_root = run_sandbox::sandbox_home_roots(&state.run)
        .map(|(home, _)| home)
        .unwrap_or_default();
    let manager_harness = pdo_harness::adapters::descriptor_file::load(&harness_home_root)
        .resolve(&manager_harness_name)
        .unwrap_or_else(|| {
            warn!(
                "manager for run {run_id}: unknown harness '{manager_harness_name}' — \
                 launching the manager on the claude floor (the first node spawn fails fast)"
            );
            pdo_harness::registry::claude()
        });
    // #708: the manager runs the Run's harness inside the container, so it fills that
    // harness's staging set (once per Run — the first node may not have spawned yet)
    // and carries its env on the exec. Best-effort: a fill error must not wedge the
    // best-effort manager assist, so it degrades to no env (the first NODE spawn is
    // the authoritative fail-fast on an unfillable set).
    let manager_set_env: Vec<(String, String)> = match (sandboxed, run_state.as_ref()) {
        (true, Some(rs)) => {
            let trusted = effective_repo_root(state, rs);
            run_sandbox::stage_harness_set(&state.run, rs, &manager_harness_name, &trusted)
                .unwrap_or_else(|e| {
                    warn!("manager for run {run_id}: staging `{manager_harness_name}` set failed (best-effort): {e:#}");
                    Vec::new()
                })
        }
        _ => Vec::new(),
    };
    // The manager runs inside the Run's container too when sandboxed.
    let sandbox = sandboxed.then(|| {
        pdo_run::application::sandbox::SandboxExec::new(
            state.docker_cmd_override.as_deref(),
            run_id,
            &session_name,
            worktree_dir,
            &manager_set_env,
        )
    });
    // Reservation before spawn (ADR-0038, extended to every path): the durable
    // event lands FIRST, the tmux spawn after. The projection ignores this kind —
    // the wire's `has_manager` stays an observed tmux fact, so a spawn that fails
    // after the reservation reads `false`, not `true`.
    emit_run_event(
        &state.run,
        run_id,
        event_log::EventKind::ManagerStarted,
        None,
    )
    .await;
    if let Err(e) = pdo_session::application::launch::spawn(
        state.tmux(),
        &session_name,
        &full_prompt,
        worktree_dir,
        run_id,
        "__manager__",
        0,
        state.port,
        state.tmux_cmd_override.as_deref(),
        // No pinned session id: the manager owns no `NodeStarted`, the sweep never
        // probes it and it is never resumed. Resolving nodes by their own id is
        // precisely what stops the manager's transcript, which shares this cwd, from
        // being read as a node's.
        pdo_session::domain::launch::SessionTail::Agent {
            harness: &manager_harness,
            model: manager_combo.model.as_deref(),
            effort: manager_combo.effort.as_deref(),
            session_id: None,
        },
        sandbox
            .as_ref()
            .map(pdo_run::application::sandbox::SandboxExec::wrap)
            .as_ref(),
        // The manager is an infra session, not a pipeline node — it never calls
        // `pdo complete`, so it gets no turn-end `Stop` hook.
        false,
    ) {
        error!("failed to spawn manager tmux session: {e}");
    } else {
        info!("Spawned manager session: {session_name}");
    }
}

/// Apply the stored `manager_profile` pin to the manager's « Follow the Run »
/// combo (design §2.2 « Manager profile »). `stored` is the trimmable
/// instance-config value; `found` is the `agent_profile::find_by_name_ci` answer
/// for it (errors stringified so the helper stays pure and unit-testable).
///
/// Returns the combo to launch with — the named profile's harness · model ·
/// effort when the pin resolves, otherwise the « Follow the Run » combo
/// untouched — plus a warning for a pin that could not fire (profile missing or
/// unreadable): the fallback is loud, and the stored value is never rewritten.
fn apply_manager_profile_pin(
    stored: Option<&str>,
    found: Result<Option<agent_profile::AgentProfile>, String>,
    follow_run: pdo_harness::agent_choice::ResolvedCombo,
) -> (pdo_harness::agent_choice::ResolvedCombo, Option<String>) {
    let Some(pin) = stored else {
        return (follow_run, None);
    };
    match found {
        Ok(Some(profile)) => (profile.combo(), None),
        Ok(None) => (
            follow_run,
            Some(format!(
                "manager profile `{pin}` names no agent profile — falling back to \
                 « Follow the Run »"
            )),
        ),
        Err(e) => (
            follow_run,
            Some(format!(
                "cannot read agent profile `{pin}` ({e}) — falling back to \
                 « Follow the Run »"
            )),
        ),
    }
}

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
    let guard_default = guard_runner::GUARD_TIMEOUT_SECS as i64;
    let guard_stored_wins = cfg.guard_timeout_secs.filter(|&n| (1..=600).contains(&n));
    let guard_env_ms = guard_runner::env_guard_timeout_ms().map(|n| n as i64);
    let guard_effective =
        guard_runner::guard_timeout_with(cfg.guard_timeout_secs.map(|n| n as u64)).as_secs() as i64;
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

    let triggers = trigger_store::list(&state.db)
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

#[derive(Deserialize)]
struct PauseTriggersRequest {
    paused: bool,
}

/// `POST /triggers/pause` — flip the global Trigger kill-switch.
///
/// Persists `triggers_paused` on `instance_config` (a daemon-wide scheduler gate,
/// outside the `/settings` machinery) and broadcasts a WS event, because there is no
/// trigger polling and a second tab would otherwise never learn. Idempotent.
///
/// The per-Trigger `enabled` flag is never touched: pause and per-Trigger disable are
/// orthogonal channels, which is what makes "restore the prior state on unpause" free.
async fn pause_triggers(
    State(state): State<Arc<AppState>>,
    actor: OriginHint,
    Json(req): Json<PauseTriggersRequest>,
) -> Response {
    // This endpoint writes blind, so capture the "before" flag first or the audit
    // entry cannot show the direction of the flip.
    let before = instance_config::triggers_paused(&state.db).await.ok();
    match instance_config::set_triggers_paused(&state.db, req.paused).await {
        Ok(()) => {
            let _ = state.bus_tx.send(serde_json::json!({
                "type": "triggers_paused",
                "paused": req.paused,
            }));
            state
                .platform
                .record_audit()
                .execute(pdo_platform::domain::audit::NewAuditEntry {
                    actor_hint: actor.as_hint().to_string(),
                    action: "triggers.pause_changed".to_string(),
                    target_kind: Some("instance".to_string()),
                    target_id: None,
                    before: Some(serde_json::json!({ "paused": before })),
                    after: Some(serde_json::json!({ "paused": req.paused })),
                })
                .await;
            Json(serde_json::json!({ "ok": true, "paused": req.paused })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// `GET /triggers/health` — scheduler liveness: the most recent tick start, the
/// configured interval, and the global pause flag, so a dead/stalled scheduler — or
/// a deliberately-paused one — is observable instead of silently dormant.
async fn triggers_health(State(state): State<Arc<AppState>>) -> Response {
    let ms = state
        .last_trigger_tick_ms
        .load(std::sync::atomic::Ordering::Relaxed);
    let last_tick_at = (ms != 0)
        .then(|| chrono::DateTime::from_timestamp_millis(ms))
        .flatten()
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    // Fail-open on a DB read error, matching the tick gate: a paused daemon must
    // never look "dead", and a genuinely-broken DB surfaces on the next tick anyway.
    let paused = instance_config::triggers_paused(&state.db)
        .await
        .unwrap_or(false);
    Json(serde_json::json!({
        "last_tick_at": last_tick_at,
        "tick_interval_secs": trigger_scheduler::TICK_INTERVAL_SECS,
        "paused": paused,
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

    let mut states = Vec::new();
    for run_id in run_ids {
        let events = match load_events(&state.db, &run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(run_state) = event_log::project(&events) {
            states.push(run_state);
        }
    }
    // #588 / ADR-0069 §4: a child awaiting its user lifts its parent on read.
    child_awaiting::overlay(&mut states);

    let mut runs = Vec::new();
    for run_state in states {
        {
            let stalled = event_log::is_stalled(&run_state);
            // Must be computed before `run_state`'s fields move into the literal:
            // `effective_repo_root` borrows `&run_state`.
            let effective_repo = effective_repo_root(&state, &run_state)
                .to_string_lossy()
                .into_owned();
            runs.push(RunListEntry {
                run_id: run_state.run_id,
                pipeline_name: run_state.pipeline_name,
                status: run_state.status,
                stalled,
                started_at: run_state.started_at,
                failure_reason: run_state.failure_reason,
                awaiting_reason: run_state.awaiting_reason,
                awaiting_reason_code: run_state.awaiting_reason_code,
                name: run_state.name,
                triggered_by: run_state.triggered_by,
                parent_run_id: run_state.parent_run_id,
                parent_node_id: run_state.parent_node_id,
                effective_repo,
            });
        }
    }

    Json(runs).into_response()
}

/// One entry of `GET /runs/reapable`: a terminal, non-archived run whose on-disk
/// worktree(s) still exist and can be reclaimed via the explicit `cleanup_run`
/// command.
#[derive(Serialize, TS)]
#[ts(export)]
struct ReapableEntry {
    run_id: String,
    pipeline_name: String,
    /// The run's terminal status. Deliberately NOT pre-filtered — the consumer
    /// applies its own status policy.
    status: event_log::RunStatus,
    /// ISO timestamp of the terminal transition — the age a janitor TTL policy
    /// compares against.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    completed_at: Option<String>,
    /// Seconds since `completed_at` (`None` when it is absent/unparseable), so a
    /// consumer can apply an age threshold without re-parsing timestamps.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    age_secs: Option<i64>,
    /// Always `true` on an emitted entry (the endpoint only lists worktree-present
    /// runs); carried explicitly so the payload is self-describing.
    worktree_present: bool,
    /// Resolved target repo the worktree lives under (`effective_repo_root`).
    effective_repo: String,
    /// Best-effort recursive byte size of the run dir, present only when the
    /// caller passes `?size=true` (the default omits it to stay fast). `None`
    /// when not requested or when sizing failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    approx_disk_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct ReapableQuery {
    /// `?size=true` computes `approx_disk_bytes` per run (walks the tree, so it
    /// is opt-in). Defaults to `false` to keep the listing cheap.
    #[serde(default)]
    size: bool,
}

/// Best-effort recursive byte size of a directory tree (apparent size). Symlinks are
/// not followed, so it cannot cycle. `None` only when the root does not exist;
/// per-entry I/O errors are skipped. Opt-in behind `?size=true` because a run
/// worktree can hold a full `node_modules`.
fn dir_size_bytes(root: &Path) -> Option<u64> {
    if !root.exists() {
        return None;
    }
    let mut total: u64 = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue; // don't follow — avoids cycles and double counting
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    Some(total)
}

/// GET /runs/reapable — read-only surfacing of terminal runs whose on-disk
/// worktree(s) still exist, so a human or a janitor pipeline can reclaim the disk via
/// the explicit `cleanup_run` command.
///
/// DOCTRINE (ADR-0012a): this endpoint NEVER deletes anything. It only *surfaces*
/// candidates; the irreversible teardown stays a pipeline/human action.
///
/// A run is emitted iff it is terminal AND not already `Archived` AND a `worktree/`
/// or `nodes/` dir still exists under its run dir.
async fn list_reapable_runs(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ReapableQuery>,
) -> Response {
    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let now = chrono::Utc::now();
    let mut entries = Vec::new();
    for run_id in run_ids {
        let events = match load_events(&state.db, &run_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Some(run_state) = event_log::project(&events) else {
            continue;
        };

        // `Archived` is terminal too, so `is_terminal()` alone is not enough.
        let eligible =
            run_state.status.is_terminal() && run_state.status != event_log::RunStatus::Archived;
        if !eligible {
            continue;
        }

        let repo_root = effective_repo_root(&state, &run_state);
        let run_dir = repo_root.join(".pdo").join("runs").join(&run_id);
        let worktree_present = run_dir.join("worktree").exists() || run_dir.join("nodes").exists();
        if !worktree_present {
            continue;
        }

        // `completed_at` is borrowed here and moved into the entry below.
        let age_secs = run_state
            .completed_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| {
                now.signed_duration_since(dt.with_timezone(&chrono::Utc))
                    .num_seconds()
            });

        let approx_disk_bytes = if q.size {
            dir_size_bytes(&run_dir)
        } else {
            None
        };

        entries.push(ReapableEntry {
            run_id: run_state.run_id,
            pipeline_name: run_state.pipeline_name,
            status: run_state.status,
            completed_at: run_state.completed_at,
            age_secs,
            worktree_present,
            effective_repo: repo_root.to_string_lossy().into_owned(),
            approx_disk_bytes,
        });
    }

    Json(entries).into_response()
}

/// One child Run of `GET /runs/{run_id}/children` — the read model the
/// Orchestration view consumes. Ordinary projection fields, plus a cost
/// **derived on read** through the cost cache, never persisted (ADR-0052):
/// `None` means unavailable and the surface renders "—", never `$0`.
#[derive(Serialize, TS)]
#[ts(export)]
struct RunChildEntry {
    run_id: String,
    pipeline_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    name: Option<String>,
    status: event_log::RunStatus,
    /// Display-only "no forward progress" overlay (#180, #783): true when the
    /// child has no running/waiting node and a stale node, even though `status`
    /// stays `running`. Derived per read by `event_log::is_stalled`, exactly as
    /// on `GET /runs`, so the Orchestration tab's stale pill and the run list's
    /// amber dot can never disagree about the same child.
    stalled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    completed_at: Option<String>,
    /// Derived on read via the cost cache (ADR-0052) — never persisted. Absent
    /// (JSON `null`/omitted) when the cost is unavailable: the surface renders
    /// "—", never `$0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    cost: Option<event_log::CostStat>,
}

/// One group of `GET /runs/{run_id}/children`: all children spawned by ONE
/// node's sessions of the parent Run. `node_id: None` groups the (theoretically
/// impossible but tolerated) children whose `RunStarted` carried no node id —
/// grouped last, never dropped.
#[derive(Serialize, TS)]
#[ts(export)]
struct RunChildrenGroup {
    node_id: Option<String>,
    children: Vec<RunChildEntry>,
}

#[derive(Serialize, TS)]
#[ts(export)]
struct RunChildrenResponse {
    run_id: String,
    nodes: Vec<RunChildrenGroup>,
}

/// `GET /runs/{run_id}/children` — the dedicated children read (ADR-0064):
/// every Run whose mechanical `parent_run_id` is this Run, grouped by parent
/// node, in the parent's node-definition order (unknown/absent node last),
/// children chronological within a group. Cost and duration live HERE, not on
/// `GET /runs` — the global list stays cheap (ADR-0052: derived on read, never
/// persisted).
async fn list_run_children(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    // The parent must exist, so a typo'd id is a 404 — not a silent empty list
    // (which would be indistinguishable from a childless Run).
    let Some((_, parent_state)) = reload_run_state_with(&state.run, &run_id).await else {
        return (StatusCode::NOT_FOUND, "run not found").into_response();
    };

    let run_ids = match load_all_run_ids(&state.db).await {
        Ok(ids) => ids,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response();
        }
    };

    let mut by_node: HashMap<Option<String>, Vec<RunChildEntry>> = HashMap::new();
    let mut projected: Vec<(Vec<event_log::Event>, event_log::RunState)> = Vec::new();
    for child_id in run_ids {
        if child_id == run_id {
            continue;
        }
        let events = match load_events(&state.db, &child_id).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Some(child) = event_log::project(&events) else {
            continue;
        };
        projected.push((events, child));
    }
    // #588 / ADR-0069 §4: a grandchild's wait shows on the child row too, so
    // the Orchestration tab and the run list agree about the same child.
    let mut states: Vec<event_log::RunState> = projected.iter().map(|(_, s)| s.clone()).collect();
    child_awaiting::overlay(&mut states);
    for ((events, _), child) in projected.into_iter().zip(states) {
        if child.parent_run_id.as_deref() != Some(run_id.as_str()) {
            continue;
        }
        let cost = derive_run_cost(&state, &child, &events);
        let stalled = event_log::is_stalled(&child);
        let entry = RunChildEntry {
            run_id: child.run_id,
            pipeline_name: child.pipeline_name,
            name: child.name,
            status: child.status,
            stalled,
            started_at: child.started_at,
            completed_at: child.completed_at,
            cost,
        };
        by_node.entry(child.parent_node_id).or_default().push(entry);
    }

    // Group order: the parent's own node-definition order, so the response reads
    // like the pipeline; a group keyed by an unknown or absent node id lands last.
    let mut nodes: Vec<RunChildrenGroup> = Vec::new();
    let mut seen: std::collections::HashSet<Option<String>> = Default::default();
    let mut push_group = |node_id: Option<String>, by_node: &mut HashMap<_, _>| {
        if !seen.insert(node_id.clone()) {
            return;
        }
        let mut children: Vec<RunChildEntry> = by_node.remove(&node_id).unwrap_or_default();
        if children.is_empty() {
            return;
        }
        children.sort_by(|a, b| match (&a.started_at, &b.started_at) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        nodes.push(RunChildrenGroup { node_id, children });
    };
    for def in &parent_state.node_defs {
        push_group(Some(def.id.clone()), &mut by_node);
    }
    let mut leftover_keys: Vec<Option<String>> = by_node.keys().cloned().collect();
    leftover_keys.sort();
    for key in leftover_keys {
        push_group(key, &mut by_node);
    }

    Json(RunChildrenResponse { run_id, nodes }).into_response()
}

/// Derive a Run's USD cost on read via the cost cache — the same fold
/// `GET /runs/{id}` augments its state with, WITHOUT the LOC walk. `None` ⇒ the
/// surface renders "—", never `$0` (ADR-0052). Not a wrapper that hides the
/// injected roots: every input stays an explicit argument at the call edge.
fn derive_run_cost(
    state: &AppState,
    run_state: &event_log::RunState,
    events: &[event_log::Event],
) -> Option<event_log::CostStat> {
    let (home_root, sandbox_root) =
        run_sandbox::sandbox_home_roots(&state.run).unwrap_or_else(|_| {
            let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let sandbox = home.join(".pdo").join("sandbox");
            (home, sandbox)
        });
    let projects_root = pdo_sandbox::adapters::staging::stores::transcripts_root(
        !run_state.sandbox.is_off(),
        &run_state.run_id,
        &home_root,
        &sandbox_root,
    );
    let prices = price_table::PriceTable::load(&home_root);
    let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::for_run(
        !run_state.sandbox.is_off(),
        &run_state.run_id,
        &home_root,
        &sandbox_root,
    );
    run_cost::compute_run_cost_breakdown_cached(
        events,
        &projects_root,
        &stores,
        &effective_repo_root(state, run_state),
        &run_state.run_id,
        &prices,
    )
    .cost
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

/// The Run's shared worktree, when it still exists on disk (#835). `None` once
/// the Run is finished and cleaned up, or archived: the `worktree` ref is gone.
fn run_worktree_dir(
    state: &AppState,
    run_state: &event_log::RunState,
    run_id: &str,
) -> Option<PathBuf> {
    let dir = worktrees::worktree_dir_for_run(&effective_repo_root(state, run_state), run_id);
    worktrees::is_git_worktree(&dir).then_some(dir)
}

/// The id a `from`/`to`/`ref` query defaults to when absent: the working tree
/// while it exists (#835), the tip otherwise — the same rule as `run_refs::collect`.
fn default_to_id(worktree: Option<&PathBuf>) -> &'static str {
    if worktree.is_some() {
        run_refs::WORKTREE_ID
    } else {
        run_refs::TIP_ID
    }
}

/// What a `from`/`to`/`ref` query value resolved to (#835).
enum RefTarget {
    /// A git ref (branch, SHA) to hand to `git`.
    Git(String),
    /// The Run's working tree — read from the worktree dir / its snapshot tree.
    Worktree,
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
    let worktree = run_worktree_dir(&state, &run_state, &run_id);
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
struct FileAtRefQuery {
    path: String,
    /// Defaults to the Run's working tree while it exists (#835), else to the
    /// Run's tip (`pdo/run-<id>`).
    #[serde(rename = "ref")]
    git_ref: Option<String>,
}

/// `GET /runs/<id>/file?path=<repo-relative>&ref=<ref>` (#748): the full content
/// of one file at one Run ref, for the Review page's context expansion. Text is
/// served as `text/plain`, anything else as `application/octet-stream`; a path
/// missing at that ref is a 404.
async fn run_file_at_ref(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(q): Query<FileAtRefQuery>,
) -> Response {
    let (events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    if !structured_diff::is_safe_path(&q.path) {
        return (
            StatusCode::BAD_REQUEST,
            format!("invalid path: {:?}", q.path),
        )
            .into_response();
    }
    // #749: stable Run ref ids resolve like on the structured diff endpoint.
    let worktree = run_worktree_dir(&state, &run_state, &run_id);
    let target = match resolve_ref_param(
        &run_id,
        &run_state,
        &events,
        q.git_ref.as_deref(),
        default_to_id(worktree.as_ref()),
    ) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let repo = effective_repo_root(&state, &run_state);
    let content = match target {
        RefTarget::Git(git_ref) => structured_diff::file_at_ref(&repo, &git_ref, &q.path),
        // #835: the working tree's file is the file on disk (untracked included).
        RefTarget::Worktree => {
            let Some(dir) = worktree else {
                return (StatusCode::NOT_FOUND, "run worktree not found").into_response();
            };
            match std::fs::read(dir.join(&q.path)) {
                Ok(bytes) => Ok(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return (StatusCode::NOT_FOUND, "file not found at ref").into_response();
                }
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
                }
            }
        }
    };
    match content {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                text,
            )
                .into_response(),
            Err(e) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/octet-stream")],
                e.into_bytes(),
            )
                .into_response(),
        },
        Err(structured_diff::GitError::Failed { stderr })
            if stderr.contains("does not exist")
                || stderr.contains("exists on disk, but not in")
                || stderr.contains("unknown revision")
                || stderr.contains("not a git repository")
                || stderr.contains("bad revision")
                || stderr.contains("nvalid object name") =>
        {
            (StatusCode::NOT_FOUND, "file not found at ref").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Resolve one `from`/`to`/`ref` query value (#749): absent ⇒ `default_id`; a
/// stable Run ref id ⇒ its git ref (404 when the Run knows no such ref) or the
/// working tree (#835); anything else ⇒ a raw git ref, kept for compatibility
/// once it passes the injection guard.
fn resolve_ref_param(
    run_id: &str,
    run_state: &event_log::RunState,
    events: &[event_log::Event],
    value: Option<&str>,
    default_id: &str,
) -> Result<RefTarget, Box<Response>> {
    let value = value.unwrap_or(default_id);
    match run_refs::resolve_id(run_id, run_state, events, value) {
        run_refs::Resolved::Git(r) => Ok(RefTarget::Git(r)),
        run_refs::Resolved::Worktree => Ok(RefTarget::Worktree),
        run_refs::Resolved::Unknown => Err(Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("unknown run ref: {value}") })),
            )
                .into_response(),
        )),
        run_refs::Resolved::NotAnId => {
            if !structured_diff::is_safe_ref(value) {
                return Err(Box::new(
                    (StatusCode::BAD_REQUEST, format!("invalid ref: {value:?}")).into_response(),
                ));
            }
            Ok(RefTarget::Git(value.to_string()))
        }
    }
}

/// `GET /runs/<id>/refs` (#749, ADR-0067 §1): the Run's refs — fork point, Run
/// tip, every node delivery's `before`/`after` from the event log (labelled by
/// node name and `iter`), the live sub-worktree branch of a running isolated
/// node, and the Run's working tree while it exists (#835, the default
/// destination) — plus the ready-made delivery pairs. Labels are built here so
/// the UI and the CLI agree. SHAs are resolved in the Run's effective repository.
async fn run_refs(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let (events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let repo = effective_repo_root(&state, &run_state);
    let sha_of = |r: &str| structured_diff::rev_parse(&repo, r);
    let snapshot = run_worktree_dir(&state, &run_state, &run_id)
        .map(|dir| structured_diff::snapshot_worktree(&dir).ok());
    Json(run_refs::collect(
        &run_id,
        &run_state,
        &events,
        &sha_of,
        snapshot.as_ref().map(Option::as_deref),
    ))
    .into_response()
}

/// `GET /runs/<id>/source-drift` (#803, ADR-0070 §4): how far the Run and its source
/// have drifted apart since the fork point.
///
/// A READ over local refs, recomputed per call — no fetch, ever (opening a Run must
/// not put traffic on someone's repository). A Run whose branch or source is gone
/// answers `unavailable` with a reason, **200**: an archived Run is the expected end
/// of a Run's life, not an error to report as one.
async fn run_source_drift(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let (_events, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let repo = effective_repo_root(&state, &run_state);
    Json(source_drift(&repo, &run_id, &run_state)).into_response()
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
    let worktree = run_worktree_dir(&state, &run_state, &run_id);
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
    let worktree = run_worktree_dir(&state, &run_state, &run_id);
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
    let created = match ensure_run_manager(&state, &run_id, &run_state).await {
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

async fn handle_run_pipeline_modifications(
    state: Arc<AppState>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<pipeline_watcher::RunPipelineModified>,
) {
    while let Some(modified) = rx.recv().await {
        let event = event_log::Event {
            id: None,
            run_id: modified.run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
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

        spawn_ready_after_event(&state.run, &modified.run_id).await;
    }
}

async fn get_run_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    // Resolve against the durable store when the run is archived — its repo-local
    // worktree and pipeline.yaml are gone — against the live run dir otherwise.
    let run_state = match load_events(&state.db, &run_id).await {
        Ok(events) => event_log::project(&events),
        Err(_) => None,
    };
    let yaml_path = match &run_state {
        Some(rs) => archived_or_live_pipeline_yaml(&state.run, rs, &run_id),
        None => run_scoped_pipeline_path(&state.repo_root, &run_id),
    };
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

    let prompts_dir = match &run_state {
        Some(rs) => archived_or_live_prompts_dir(&state.run, rs, &run_id),
        None => run_scoped_prompts_dir(&state.repo_root, &run_id),
    };
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

/// The pipeline a Run ran (archived or live) with its prompts — what the Run's
/// portable document and its skills sidecar are both built from.
async fn run_pipeline_for_export(
    state: &AppState,
    run_id: &str,
) -> Result<(pipeline::PipelineDef, HashMap<String, String>), Box<Response>> {
    let run_state = load_events(&state.db, run_id)
        .await
        .ok()
        .and_then(|events| event_log::project(&events));
    let yaml_path = match &run_state {
        Some(run) => archived_or_live_pipeline_yaml(&state.run, run, run_id),
        None => run_scoped_pipeline_path(&state.repo_root, run_id),
    };
    let yaml = std::fs::read_to_string(&yaml_path)
        .map_err(|_| Box::new((StatusCode::NOT_FOUND, "run pipeline not found").into_response()))?;
    let parsed = pipeline::parse_pipeline(&yaml).map_err(|error| {
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("invalid run pipeline: {error}") })),
            )
                .into_response(),
        )
    })?;
    let prompts_dir = match &run_state {
        Some(run) => archived_or_live_prompts_dir(&state.run, run, run_id),
        None => run_scoped_prompts_dir(&state.repo_root, run_id),
    };
    Ok((
        parsed.pipeline,
        pdo_pipeline::adapters::registry::read_prompts_dir(&prompts_dir),
    ))
}

async fn get_run_pipeline_document_skills(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    match run_pipeline_for_export(&state, &run_id).await {
        Ok((pipeline, _)) => pdo_pipeline::adapters::http::skills_sidecar_response(
            pdo_pipeline::application::sidecar_export(
                &pdo_pipeline::adapters::skill_bank::BankOfSkills(state.skills.clone()),
                &pipeline,
            ),
        ),
        Err(response) => *response,
    }
}

async fn get_run_pipeline_document(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let (pipeline, prompts) = match run_pipeline_for_export(&state, &run_id).await {
        Ok(found) => found,
        Err(response) => return *response,
    };
    match portable_pipeline::export(&pipeline, &prompts) {
        Ok(document) => (
            [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
            document,
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

/// What the pipeline context's run-edit rules read of a Run: each node's
/// status, each loop's lap.
fn run_progress(run_state: &event_log::RunState) -> pdo_pipeline::domain::mutation::RunProgress {
    use event_log::NodeStatus as S;
    use pdo_pipeline::domain::mutation::{LoopLap, NodeProgress as P, RunProgress};
    RunProgress {
        nodes: run_state
            .nodes
            .iter()
            .map(|(id, node)| {
                let progress = match node.status {
                    S::Pending => P::Pending,
                    S::Waiting => P::Waiting,
                    S::Running => P::Running,
                    S::AwaitingUser => P::AwaitingUser,
                    S::Completed => P::Completed,
                    S::Skipped => P::Skipped,
                    S::Failed => P::Failed,
                    S::Stopped => P::Stopped,
                    S::Stale => P::Stale,
                    S::Interrupted => P::Interrupted,
                };
                (id.clone(), progress)
            })
            .collect(),
        laps: run_state
            .loop_states
            .iter()
            .map(|(id, lap)| {
                (
                    id.clone(),
                    LoopLap {
                        current_iter: lap.current_iter,
                        done: lap.done,
                    },
                )
            })
            .collect(),
    }
}

#[derive(Deserialize)]
struct SaveRunPipelineRequest {
    yaml: String,
    #[serde(default)]
    prompts: HashMap<String, String>,
}

async fn save_run_pipeline(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<SaveRunPipelineRequest>,
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
            // Old pipeline unparseable: skip validation, allow the overwrite.
            new_pipeline.clone()
        }
    };

    let rejections = pdo_pipeline::domain::mutation::validate_run_mutation(
        &old_pipeline,
        &new_pipeline,
        &run_progress(&run_state),
    );
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

    state.recent_writes.mark(&yaml_path);
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
        state.recent_writes.mark(&prompt_path);
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

    state.recent_writes.mark(&template_path);

    if let Err(e) = std::fs::write(&tmp_path, yaml) {
        warn!("sync_run_pipeline_to_template: write tmp failed: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &template_path) {
        warn!("sync_run_pipeline_to_template: rename failed: {e}");
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }

    // Write each prompt to the canonical `<dir>/<stem>.prompts/<id>.md` location every
    // reader consults. NOT a flat `<dir>/prompts/<id>.md` dir: no reader looks there,
    // so run-scoped prompt edits vanish in silence.
    for (node_id, content) in prompts {
        let prompt_path = pipeline::canonical_prompt_path(&template_path, node_id);
        if let Some(parent) = prompt_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        state.recent_writes.mark(&prompt_path);
        if let Err(e) = std::fs::write(&prompt_path, content) {
            warn!("sync_run_pipeline_to_template: prompt sync failed for {node_id}: {e}");
        }
    }

    info!(
        "Synced run {run_id} pipeline to template at {}",
        template_path.display()
    );
}

#[derive(Deserialize)]
struct ArtifactQuery {
    path: String,
}

async fn artifact(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<ArtifactQuery>,
) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    // Serve from the durable store for an archived run (worktree gone), from the
    // live worktree otherwise. The `.canonicalize()` guard below holds against
    // whichever base is resolved.
    let artifacts_dir = archived_or_live_artifacts_dir(&state.run, &run_state, &run_id);

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
        // An `.html` artifact DELIBERATELY falls through to `text/markdown`. Do NOT
        // add `Some("html") => "text/html"` (ADR-0028): serving agent-authored HTML as
        // a top-level document at the daemon origin — no auth, no CSP — would let it
        // execute arbitrary script. It is only ever rendered in a sandboxed iframe.
        _ => "text/markdown",
    };

    // #796: an artifact URL is stable across a same-iter re-spawn (same run,
    // node, iter and path) while its bytes change under it. Without a validator
    // the browser's heuristic cache may resurface the previous session's report
    // or screenshot; `no-store` makes every reload show the current bytes.
    let headers = [
        (header::CONTENT_TYPE, mime),
        (header::CACHE_CONTROL, "no-store"),
    ];
    if mime.starts_with("image/") {
        match std::fs::read(&resolved) {
            Ok(bytes) => (StatusCode::OK, headers, bytes).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    } else {
        match std::fs::read_to_string(&resolved) {
            Ok(content) => (StatusCode::OK, headers, content).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
        }
    }
}

/// Response of `POST /runs/{run_id}/manager/start` (manager on demand).
#[derive(Serialize, TS)]
#[ts(export)]
struct ManagerStartResponse {
    ok: bool,
    session: String,
    /// `true` when THIS call spawned the session; `false` when it already
    /// existed — a double-click Start is an idempotent re-answer, not an error.
    created: bool,
}

/// Start the Run's Pipeline Manager on demand (manager on demand).
///
/// The Manager tab's Start button is ALWAYS available — the `manager_enabled`
/// setting gates only the automatic spawn at Run creation, never this — so the
/// handler is gated on nothing but the Run existing and its worktree still
/// being on disk (an archived Run has none left to work in).
///
/// Idempotency and the racing discipline are the run shell's
/// create-then-verify rule: `session_exists` first (a benign re-answer), spawn,
/// then verify again — a concurrent start may have won the `new-session`, and a
/// spawn failure after our own reservation event must read as an error, not a
/// silent success (the projection ignores `ManagerStarted`; the wire's
/// `has_manager` stays the observed tmux fact).
async fn start_run_manager(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    match ensure_run_manager(&state, &run_id, &run_state).await {
        Ok(created) => (
            StatusCode::OK,
            Json(ManagerStartResponse {
                ok: true,
                session: pdo_session::domain::names::manager_session_name(&run_id),
                created,
            }),
        )
            .into_response(),
        Err((status, body)) => (status, Json(body)).into_response(),
    }
}

/// Make sure the Run's Pipeline Manager session is up, spawning it on demand
/// (manager on demand; #750 reuses it for "send to manager"). `Ok(true)` when
/// THIS call spawned it, `Ok(false)` when it already existed; `Err` carries the
/// status + JSON body the HTTP surfaces answer with (`409` no worktree, `500`
/// the session did not come up).
///
/// Idempotency and the racing discipline are the run shell's create-then-verify
/// rule: `session_exists` first (a benign re-answer), spawn, then verify again —
/// a concurrent start may have won the `new-session`, and a spawn failure after
/// our own reservation event must read as an error, not a silent success (the
/// projection ignores `ManagerStarted`; the wire's `has_manager` stays the
/// observed tmux fact).
async fn ensure_run_manager(
    state: &Arc<AppState>,
    run_id: &str,
    run_state: &event_log::RunState,
) -> Result<bool, (StatusCode, serde_json::Value)> {
    let session_name = pdo_session::domain::names::manager_session_name(run_id);
    let tmux = state.tmux();
    if tmux.session_exists(&session_name) {
        return Ok(false);
    }

    // The manager works in the Run's worktree; an archived/cleaned Run has none
    // left, and a session spawned into a dead cwd would only fail at tmux.
    let repo_root = effective_repo_root(state, run_state);
    let worktree_dir =
        pdo_workspace::adapters::git::worktrees::worktree_dir_for_run(&repo_root, run_id);
    if !worktree_dir.exists() {
        return Err((
            StatusCode::CONFLICT,
            serde_json::json!({
                "error": "this run's worktree no longer exists (archived or cleaned) — there is nothing for a manager to work in"
            }),
        ));
    }

    // The create path's name_hint discipline, reconstructed from the projected
    // state: a placeholder name still carries its rename instruction, a named
    // Run is left alone, and a nameless Run derives from its input. The FIRST
    // start (manual or automatic) names the Run best-effort, exactly as before.
    let name_hint = match run_state.name.as_deref() {
        Some(name) if name.starts_with("Untitled run") => {
            prompt_augmenter::RunNameHint::Placeholder
        }
        Some(_) => prompt_augmenter::RunNameHint::UserProvided,
        None => match run_state.input.as_deref() {
            Some(input) if !input.trim().is_empty() => {
                prompt_augmenter::RunNameHint::DeriveFromInput
            }
            _ => prompt_augmenter::RunNameHint::Placeholder,
        },
    };

    spawn_manager_session(
        state,
        run_id,
        &worktree_dir,
        name_hint,
        !run_state.sandbox.is_off(),
    )
    .await;

    // Create-then-verify (the run shell's benign-race rule).
    if !tmux.session_exists(&session_name) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "error": "the manager tmux session did not come up (see the daemon logs)"
            }),
        ));
    }
    Ok(true)
}

/// Stop the Run's Pipeline Manager (manager on demand). Cost control is the
/// point of the feature, so the session must be killable from the panel — not
/// only by the orphan sweep or a manual `tmux kill-session`.
///
/// Stop on a session that is already gone is a calm no-op that still answers
/// `200` ("nothing to stop"), and still writes its `ManagerStopped` event: the
/// projection ignores the kind, so the durable record never lies about state.
async fn stop_run_manager(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    // Durable first, kill after — the same order the start path keeps.
    emit_run_event(
        &state.run,
        &run_id,
        event_log::EventKind::ManagerStopped,
        None,
    )
    .await;

    let session_name = pdo_session::domain::names::manager_session_name(&run_id);
    let tmux = state.tmux();
    if !tmux.session_exists(&session_name) {
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "stopped": false })),
        )
            .into_response();
    }
    tmux.kill(&session_name);
    info!("Stopped manager session: {session_name}");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "stopped": true })),
    )
        .into_response()
}

/// Every descendant of `run_id` (children, grandchildren, …) resolved through
/// the same mechanical `parent_run_id` link as `list_run_children`, in
/// **post-order**: a run always comes after all of its own descendants, so a
/// cascade that walks the list front to back tears grandchildren down before
/// their parent. Pure over the projected store; `run_id` itself is not listed.
fn descendant_runs_post_order(
    runs: &[event_log::RunState],
    run_id: &str,
) -> Vec<event_log::RunState> {
    let mut out = Vec::new();
    for child in runs {
        if child.parent_run_id.as_deref() != Some(run_id) {
            continue;
        }
        out.extend(descendant_runs_post_order(runs, &child.run_id));
        out.push(child.clone());
    }
    out
}

/// Project every run of the store (the read model behind `list_run_children`).
/// Unreadable or empty logs are skipped, never fatal: the cascade must not be
/// blocked by an unrelated corrupt run.
async fn load_all_projected_runs(state: &AppState) -> Result<Vec<event_log::RunState>> {
    let mut states = Vec::new();
    for id in load_all_run_ids(&state.db).await? {
        let Ok(events) = load_events(&state.db, &id).await else {
            continue;
        };
        if let Some(run_state) = event_log::project(&events) {
            states.push(run_state);
        }
    }
    Ok(states)
}

/// `retry_all` (a Pipeline Manager command): archive a terminal Run, then start
/// its twin — same pipeline, input, variables, repositories, isolation,
/// harness, skills and preferences. Both are the Run's surfaces this crate
/// still serves; the command surface forwards this answer verbatim.
async fn retry_all_run(state: &Arc<AppState>, run_id: &str) -> Response {
    let run_id = run_id.to_string();
    let (events, run_state) = match load_projected(state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    // Deliberately NOT `RunStatus::is_terminal()`: this set omits `Archived`,
    // whose retry-ability is still an open question.
    let is_terminal = matches!(
        run_state.status,
        event_log::RunStatus::Completed
            | event_log::RunStatus::Failed
            | event_log::RunStatus::Skipped
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
    // Read the RESOLVED repo, not the raw field: an old run carries
    // `target_repo: null`, and forwarding that raw would 400 at the chokepoint
    // AFTER `cleanup_run` archived the original — archived with no replacement.
    let target_repo = Some(
        effective_repo_root(state, &run_state)
            .to_string_lossy()
            .into_owned(),
    );
    let source_branch = run_state.source_branch.clone();
    // Preserve the original Run's read-only secondaries so the retry runs in
    // the same multi-repo context. The primary must sit at [0]; the create
    // chokepoint re-freezes each secondary's SHA against its recorded base
    // branch.
    let target_repos: Vec<TargetRepoInput> = if run_state.target_repos.is_empty() {
        Vec::new()
    } else {
        let mut v = vec![TargetRepoInput {
            repo: target_repo.clone().unwrap_or_default(),
            base_branch: source_branch.clone(),
            // The primary is always writable (ADR-0047).
            read_only: false,
        }];
        v.extend(run_state.target_repos.iter().map(|p| TargetRepoInput {
            repo: p.repo.clone(),
            base_branch: p.base_branch.clone(),
            // Preserve the read-only opt-in across a retry (ADR-0047).
            read_only: p.read_only,
        }));
        v
    };

    let archive_resp = cleanup_run(state, &run_id).await;
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
        // Preserve the library pipeline id so the retried run stays in the
        // same "by pipeline" stats bucket rather than falling back to the name.
        pipeline_id: run_state.pipeline_id.clone(),
        target_repo,
        target_repos,
        source_branch,
        name: None,
        triggered_by: None,
        // A retry preserves the original Run's isolation mode. Wrapped in
        // `Some` so the chokepoint treats it as EXPLICIT: a changed instance
        // default must never silently re-sandbox (or un-sandbox) a retried Run.
        //
        // `run_state.sandbox_entries` is deliberately NOT forwarded. A retry is
        // a NEW Run with nothing staged, so there is no coherence to protect,
        // and re-resolving is what ADR-0031 §2 requires (a profile edited since
        // must take effect). Do not "fix" this by threading the frozen list: a
        // profile deleted since makes the retry 400 loudly, which is intended.
        sandbox: Some(run_state.sandbox.clone()),
        // A retry must reproduce the original's harness, or an A/B comparison
        // silently reverts to the instance default. `None` (the Run named no
        // harness) forwards as `None`. Unlike `sandbox` this needs no
        // `Some`-wrapping: the create chokepoint freezes `req.harness` verbatim.
        harness: run_state.harness.clone(),
        // #669: a retry reproduces the original Run tier's skills, same
        // reason as `harness` — the union must not silently shrink.
        skills: run_state.skills.clone(),
        // Reproduce the original Run's `AgentChoice`, like `harness`: `None`
        // forwards as `None`, so the retry re-resolves through the legacy
        // `harness` tier exactly as the original did.
        agent_choice: run_state.agent_choice.clone(),
        // A retry sets `name: None` and re-derives the name. `Some(true)`
        // reproduces that regardless of the instance default, so a changed
        // `default_auto_name` cannot silently alter how a retried Run is named.
        auto_name: Some(true),
        // Reproduce the original Run's `auto_fail`, like `harness`: `None`
        // forwards as `None` and re-resolves through project / instance.
        auto_fail: run_state.auto_fail,
        // Preserve the explicit Run tier. Instance and Project are resolved
        // afresh for this new Run, while its per-Run override remains stable.
        provisioning: run_state
            .provisioning_rules
            .iter()
            .find(|scoped| {
                scoped.scope == pdo_workspace::domain::provisioning::ProvisioningScope::Run
            })
            .map(|scoped| scoped.rules.clone())
            .unwrap_or_default(),
        // NOT carried over from the original (#804): the reason a fetch
        // failed minutes or days ago says nothing about this new cut, and
        // a retry is a manual gesture — it fetches no more than any other
        // `POST /runs` does.
        source_fetch_error: None,
    };
    // ADR-0064: a retried Run is created by the daemon, not from a node
    // session — it is a root even when the archived original had a parent.
    let new_run_resp = create_run_core(state, new_run_req, Vec::new(), None).await;

    info!("retry_all: archived run {run_id}, created new run");
    new_run_resp
}

/// `POST /runs/{run_id}/commands {"kind":"cleanup_run"}` — archive a run **and
/// its terminated descendants** (#815).
///
/// The daemon already treats children as belonging to their parent (run tree,
/// `GET /runs/{id}/children`, `pdo complete` refused while children are in
/// flight), so archiving the root must reclaim the children's worktrees and
/// run dirs too, recursively, through the same teardown path
/// ([`archive_single_run`]). Descendants are archived grandchildren-first, the
/// parent last, so a failure mid-cascade never leaves an archived parent above
/// a still-live subtree.
///
/// Live descendants (running, awaiting-user, paused) are **never** archived by
/// the cascade: the request is refused with `409` naming them, and nothing is
/// touched. That is the conservative choice of the two the issue allowed
/// (refuse vs. archive the parent and orphan the live children): killing a
/// running child is not something an archive of its parent should do
/// implicitly, and orphaning it would silently break the tree the UI renders.
/// Stop the child first, then archive. Already-archived descendants are
/// skipped silently (idempotent): only the root itself being archived is a
/// `409`, as before.
async fn cleanup_run(state: &AppState, run_id: &str) -> Response {
    let _page_mount_guard = state.platform.release_run_pages().hold().await;
    let (_, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    if run_state.status == event_log::RunStatus::Archived {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "run is already archived" })),
        )
            .into_response();
    }

    let all_runs = match load_all_projected_runs(state).await {
        Ok(runs) => runs,
        Err(e) => {
            error!("cleanup_run: cannot list runs to resolve the children of {run_id}: {e:#}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("error: {e}") })),
            )
                .into_response();
        }
    };
    let descendants = descendant_runs_post_order(&all_runs, run_id);

    let live: Vec<&str> = descendants
        .iter()
        .filter(|d| d.status.is_live())
        .map(|d| d.run_id.as_str())
        .collect();
    if !live.is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "run has {} live child run(s); stop them before archiving the parent",
                    live.len()
                ),
                "live_children": live,
            })),
        )
            .into_response();
    }

    let mut archived_children: Vec<String> = Vec::new();
    for child in &descendants {
        if child.status == event_log::RunStatus::Archived {
            continue;
        }
        if let Err(resp) = archive_single_run(state, child).await {
            return *resp;
        }
        archived_children.push(child.run_id.clone());
    }

    if let Err(resp) = archive_single_run(state, &run_state).await {
        return *resp;
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "archived",
            "archived_children": archived_children,
        })),
    )
        .into_response()
}

/// Tear ONE run down and mark it `Archived`: kill its tmux sessions, merge and
/// purge its sandbox, preserve its outputs to the durable store, remove its
/// worktrees, branches, secondary snapshots and run dir, then append
/// `RunArchived`. No cascade and no status precondition here — the caller
/// ([`cleanup_run`]) holds the page-mount lock and has already decided this
/// run must go.
async fn archive_single_run(
    state: &AppState,
    run_state: &event_log::RunState,
) -> Result<(), Box<Response>> {
    let run_id = run_state.run_id.as_str();

    if let Err(error) = state.platform.release_run_pages().execute(run_id).await {
        error!("cleanup_run: failed to remove page mounts for run {run_id}: {error}");
        return Err(Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "page mount cleanup failed" })),
            )
                .into_response(),
        ));
    }

    let tmux = state.tmux();
    for node in run_state.nodes.values() {
        let session_name =
            pdo_session::domain::names::node_session_name(run_id, &node.node_id, node.iter);
        tmux.kill(&session_name);
    }
    let mgr_session = pdo_session::domain::names::manager_session_name(run_id);
    tmux.kill(&mgr_session);
    // Kill any ad-hoc run shell before the worktree is torn down.
    let shell_session = pdo_session::domain::names::shell_session_name(run_id);
    tmux.kill(&shell_session);

    // Merge the transcripts back, then destroy the container and purge its staging,
    // all BEFORE any `git worktree remove` below: the container bind-mounts the repo,
    // so removing a live worktree under it hits a busy mount. The tmux sessions were
    // killed just above, so the staging is quiescent. `cleanup` merges FIRST —
    // harvest before purge — capturing post-terminal growth the detached terminal
    // merge missed; idempotent, so the double-merge is safe.
    if !run_state.sandbox.is_off() {
        match run_sandbox::sandbox_home_roots(&state.run) {
            Ok((home_root, sandbox_root)) => run_sandbox::cleanup(
                state.docker_cmd_override.as_deref().unwrap_or("docker"),
                &home_root,
                &sandbox_root,
                run_id,
            ),
            Err(e) => {
                warn!("cleanup_run: cannot merge/purge sandbox staging for run {run_id}: {e:#}")
            }
        }
    }

    let repo_root = effective_repo_root(state, run_state);
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);

    // Copy the run's outputs to the durable store BEFORE any worktree is destroyed,
    // so an archived run stays inspectable (ADR-0020). Ordering is load-bearing:
    // `git worktree remove` wipes `worktree/.pdo/artifacts` well before the
    // `remove_dir_all(&run_dir)` at the end.
    preserve_run_outputs_on_archive(&repo_root, &run_dir, run_id);

    // Sub-worktrees (nodes/) must go before the main worktree.
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

    // #672 / ADR-0062: take back the per-skill exclusions this Run wrote in the target
    // repo's `info/exclude` (its `# pdo <run-id>` marked lines only — every other
    // line, PDO's for another Run or the user's, stays intact).
    if let Err(e) = state.skills.withdraw(&repo_root, run_id) {
        warn!("cleanup_run: cannot remove skill exclusions for run {run_id}: {e:#}");
    }

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

    // Tear down every secondary snapshot BEFORE `remove_dir_all`, and from inside the
    // SECONDARY repo: the `--detach` registration lives in the secondary's `.git`,
    // OUTSIDE `repo_root`, so `remove_dir_all(run_dir)` cannot clear it. Without the
    // per-secondary `prune`, every multi-repo Run leaks a dangling registration and a
    // future `worktree add` at the same path fails 255.
    //
    // DISK-DRIVEN, not projection-driven: the scan must cover snapshots the projection
    // no longer lists — a secondary removed mid-run keeps its snapshot until now, and
    // an orphan snapshot whose event never landed has no pin at all. Iterating
    // `target_repos` would skip and leak both.
    let repos_dir = secondary_repos_dir(&repo_root, run_id);
    for entry in std::fs::read_dir(&repos_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let snapshot_dir = entry.path();
        if !snapshot_dir.is_dir() {
            continue;
        }
        match snapshot_owning_secondary(&snapshot_dir) {
            Some(secondary) => remove_secondary_snapshot(&secondary, &snapshot_dir),
            None => {
                warn!(
                    "cleanup_run: cannot resolve the owning secondary repo of snapshot {} \
                     (its git pointer is unreadable); removing the directory but leaving any \
                     dangling registration for `git worktree prune` in the secondary",
                    snapshot_dir.display()
                );
                let _ = std::fs::remove_dir_all(&snapshot_dir);
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
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunArchived,
        node_id: None,
        iter: None,
        payload: None,
    };
    if let Err(e) = append_event(state, &archived_event).await {
        error!("failed to append run_archived: {e}");
        return Err(Box::new(
            (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response(),
        ));
    }

    info!("Run {run_id} archived (cleanup complete)");
    Ok(())
}

/// DELETE /runs/{run_id} — permanently forget an archived run, removing every event
/// log row. Only allowed once the run is `Archived`, i.e. `cleanup_run` has already
/// torn down its worktrees and branches, or deleting the events would strand the
/// on-disk state.
async fn forget_run(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    let (_, run_state) = match load_projected(&state, &run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };

    if run_state.status != event_log::RunStatus::Archived {
        return (
            StatusCode::CONFLICT,
            "run must be archived (cleanup_run) before it can be forgotten",
        )
            .into_response();
    }

    // Tombstone + purge in ONE transaction: a late writer must never observe "events
    // gone, no tombstone yet" and resurrect the run (ADR-0024).
    let purge = async {
        let mut tx = state.db.begin().await?;
        sqlx::query("INSERT OR IGNORE INTO forgotten_runs (run_id, forgotten_at) VALUES (?, ?)")
            .bind(&run_id)
            .bind(pdo_kernel::clock::now_iso())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM events WHERE run_id = ?")
            .bind(&run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        anyhow::Ok(())
    };
    if let Err(e) = purge.await {
        error!("failed to delete events for {run_id}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "event log error").into_response();
    }

    // A forgotten run has nothing left to recover: reap its manager/shell sessions so
    // they can't keep POSTing commands.
    let tmux = state.tmux();
    for session in [
        pdo_session::domain::names::manager_session_name(&run_id),
        pdo_session::domain::names::shell_session_name(&run_id),
    ] {
        tmux.kill(&session);
    }

    // Forget also reclaims the durable output store, or the outputs preserved on
    // archive leak forever and contradict forget's "the run vanishes entirely"
    // contract. A missing dir is not an error.
    if let Some(durable) = durable_run_dir(&run_id) {
        if let Err(e) = std::fs::remove_dir_all(&durable) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "#315: failed to remove durable dir {}: {e}",
                    durable.display()
                );
            }
        }
    }

    info!("Run {run_id} forgotten (event log purged)");

    // Piggy-back on the pipeline-broadcast channel: the event channel only carries
    // typed `event_log::Event` records, and every event for this run was just deleted.
    let _ = state.bus_tx.send(serde_json::json!({
        "type": "run_forgotten",
        "run_id": run_id,
    }));

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "forgotten" })),
    )
        .into_response()
}

/// The message every write boundary returns when no target repo was named.
/// Actionable on purpose: the caller must learn *what* to pass.
const TARGET_REPO_REQUIRED: &str = "target_repo is required: name the absolute path of the git \
    repository this must work in — the daemon's own working directory is not a default";

/// Resolve a **required** target repo at a write boundary (ADR-0033).
///
/// Trims once and treats the trimmed value as canonical. The single source of the
/// rule for every write surface; the read side (`effective_repo_root`) deliberately
/// does NOT use it and keeps its fallback for historical runs.
fn required_target_repo(field: Option<&str>) -> Result<PathBuf, String> {
    match field.map(str::trim) {
        Some(p) if !p.is_empty() => validate_target_repo(p),
        _ => Err(TARGET_REPO_REQUIRED.to_string()),
    }
}

/// Validate and freeze the secondary repos of a multi-repo create (ADR-0042).
///
/// `inputs` is the full `target_repos` list: `[0]` is the primary, `[1..]` the
/// secondaries. Each `sha` is resolved NOW against the local `base_branch` (default
/// `HEAD`, no fetch) and each `alias` disambiguated on basename collision.
///
/// Fails the whole create on the FIRST bad secondary — resolving here, before
/// `RunStarted` and `create_worktree`, keeps the "a bad repo → no Run" guarantee.
fn resolve_secondary_pins(
    primary_root: &Path,
    inputs: &[TargetRepoInput],
) -> Result<Vec<event_log::RepoPin>, String> {
    if inputs.len() <= 1 {
        return Ok(Vec::new());
    }
    let primary_canon = canonical_or_self(primary_root);

    let mut pins: Vec<event_log::RepoPin> = Vec::new();
    let mut seen_canon: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut used_aliases: std::collections::HashSet<String> = std::collections::HashSet::new();
    for input in &inputs[1..] {
        // Both the create path and `patch_run_repos` freeze a secondary through this
        // ONE helper, so the two surfaces cannot drift.
        let pin =
            resolve_one_secondary_pin(input, &primary_canon, &mut seen_canon, &mut used_aliases)
                .map_err(|r| r.message())?;
        pins.push(pin);
    }
    Ok(pins)
}

/// Canonicalize for comparison, falling back to the path itself when it cannot be
/// resolved. Shared by the create and mid-run edit paths so "is this the primary / a
/// dup?" is answered identically on both.
fn canonical_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Validate and freeze **one** secondary repo into a [`event_log::RepoPin`] — the
/// single per-repo validator both `resolve_secondary_pins` and [`patch_run_repos`]
/// route through (ADR-0042).
///
/// `seen_canon` and `used_aliases` are the caller's running sets: create seeds them
/// empty, the edit path seeds them from the Run's SURVIVING secondaries **and the
/// snapshot folders present on disk**, so a removed-but-persistent snapshot's alias
/// is never reused. Mutating them here is what keeps a batch collision-free.
///
/// Writes nothing to disk, so the caller decides whether to project the typed refusal
/// or flatten it to a 400 string.
fn resolve_one_secondary_pin(
    input: &TargetRepoInput,
    primary_canon: &Path,
    seen_canon: &mut std::collections::HashSet<PathBuf>,
    used_aliases: &mut std::collections::HashSet<String>,
) -> Result<event_log::RepoPin, repo_edit_refusal::RepoEditRefusal> {
    use repo_edit_refusal::RepoEditRefusal;
    let raw = input.repo.trim();
    let repo = validate_target_repo(raw).map_err(|error| RepoEditRefusal::BadRepo {
        repo: raw.to_string(),
        error,
    })?;
    let repo_display = repo.to_string_lossy().to_string();
    let repo_canon = canonical_or_self(&repo);
    // A repo cannot be its own secondary: a self-snapshot registers a detached
    // worktree inside the repo it snapshots and buys nothing.
    if repo_canon == primary_canon {
        return Err(RepoEditRefusal::SelfReference { repo: repo_display });
    }
    // A repo already listed would produce two snapshots fighting over one alias.
    if !seen_canon.insert(repo_canon) {
        return Err(RepoEditRefusal::RepoAlreadyPinned { repo: repo_display });
    }
    let base = input
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let rev = base.unwrap_or("HEAD");
    let sha = rev_parse_verified(&repo, rev).map_err(|e| RepoEditRefusal::BadRepo {
        repo: repo_display.clone(),
        error: format!("cannot resolve base branch '{rev}': {e}"),
    })?;
    let alias = disambiguate_alias(&repo, used_aliases);
    Ok(event_log::RepoPin {
        repo: repo_display,
        alias,
        sha,
        base_branch: base.map(str::to_string),
        // Both construction paths route through here, so the flag cannot diverge
        // between the two surfaces.
        read_only: input.read_only,
    })
}

/// A filesystem-safe, collision-free directory name for a secondary snapshot. Starts
/// from the repo's basename and appends `-2`, `-3`, … until unused. `used` must
/// accumulate across the call: the snapshot paths must never collide, or one snapshot
/// clobbers another.
fn disambiguate_alias(repo: &Path, used: &mut std::collections::HashSet<String>) -> String {
    let base = repo
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("repo");
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// The directory that parents a Run's secondary snapshots: `<run_dir>/repos/`.
fn secondary_repos_dir(repo_root: &Path, run_id: &str) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("repos")
}

/// The snapshot folder names present on disk under `<run_dir>/repos/`.
///
/// The DISK is the source of truth for taken aliases: a removed-but-persistent
/// snapshot (its teardown is deferred to cleanup) still owns its folder name, so a
/// later `add` reusing it makes `git worktree add` fail on the existing directory.
fn existing_snapshot_dir_names(repo_root: &Path, run_id: &str) -> Vec<String> {
    std::fs::read_dir(secondary_repos_dir(repo_root, run_id))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect()
}

/// Resolve the **secondary repo** that owns a detached snapshot at `snapshot_dir`,
/// so [`remove_secondary_snapshot`] can `prune` its registration from inside it. Two
/// roads, git-authoritative first:
///
/// - `git -C <snapshot> rev-parse --path-format=absolute --git-common-dir` returns
///   the secondary's shared `.git`; its parent is the repo root.
/// - Fallback: the `gitdir:` pointer in `<snapshot>/.git` is
///   `<secondary>/.git/worktrees/<name>`; the root is the path before `/.git/`.
///
/// `None` when neither resolves (a snapshot whose secondary was moved/deleted) — the
/// caller then only removes the directory and warns; the registration is already
/// unreachable.
fn snapshot_owning_secondary(snapshot_dir: &Path) -> Option<PathBuf> {
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(snapshot_dir)
        .output()
    {
        if output.status.success() {
            let common = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(root) = Path::new(&common).parent() {
                if !root.as_os_str().is_empty() {
                    return Some(root.to_path_buf());
                }
            }
        }
    }
    let pointer = std::fs::read_to_string(snapshot_dir.join(".git")).ok()?;
    let raw = pointer.lines().next()?.strip_prefix("gitdir:")?.trim();
    let gitdir = Path::new(raw);
    let idx = gitdir
        .iter()
        .position(|c| c == std::ffi::OsStr::new(".git"))?;
    let root: PathBuf = gitdir.iter().take(idx).collect();
    (!root.as_os_str().is_empty()).then_some(root)
}

/// `PATCH /runs/{run_id}/repos` — add / remove secondary repos on a **live** Run
/// (ADR-0042).
///
/// `RunState.target_repos` is frozen at `RunStarted` and now also moves on
/// `RunReposEdited`. The contract is **spawn-time visibility**: an edit affects nodes
/// launched AFTER it, and already-live nodes keep the context frozen at their spawn —
/// preamble and `PDO_SECONDARY_REPOS` are written once at launch and never re-read.
///
/// Order mirrors `run_command`: forgotten (410) → project (404) → terminal (409) →
/// validate removes+adds → materialise adds → append → reproject.
async fn patch_run_repos(
    State(state): State<Arc<AppState>>,
    AxumPath(run_id): AxumPath<String>,
    Json(req): Json<PatchRunReposRequest>,
) -> Response {
    use repo_edit_refusal::{repo_edit_refusal_response, RepoEditRefusal};

    // (1) A forgotten Run is a tombstone — 410 before any side effect.
    match run_is_forgotten(&state.run, &run_id).await {
        Ok(true) => {
            return repo_edit_refusal_response(&RepoEditRefusal::RunForgotten { run_id });
        }
        Ok(false) => {}
        Err(e) => {
            return repo_edit_refusal_response(&RepoEditRefusal::Internal {
                error: format!("forgotten-run check failed: {e}"),
            });
        }
    }

    let Some((_, run_state)) = reload_run_state(&state, &run_id).await else {
        return repo_edit_refusal_response(&RepoEditRefusal::RunNotFound);
    };

    // (3) A terminal Run's list is frozen for good.
    if run_state.status.is_terminal() {
        return repo_edit_refusal_response(&RepoEditRefusal::RunTerminal {
            status: run_state.status,
        });
    }

    // An empty edit is a no-op: return the current projection without touching the log.
    if req.add.is_empty() && req.remove.is_empty() {
        return (StatusCode::OK, Json(run_state)).into_response();
    }

    let repo_root = effective_repo_root(&state, &run_state);
    let primary_canon = canonical_or_self(&repo_root);

    // (4) Removals first, by alias. An absent alias is a silent no-op: the UI removes
    //     by displayed alias, and a double-click must not 404.
    let remove_set: std::collections::HashSet<&str> = req
        .remove
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let mut active: Vec<event_log::RepoPin> = run_state
        .target_repos
        .iter()
        .filter(|p| !remove_set.contains(p.alias.as_str()))
        .cloned()
        .collect();

    // (5) Seed the disambiguation sets from the SURVIVING pins AND from the folders
    //     present on disk, so a removed-but-persistent snapshot's alias is not reused.
    //     `seen_canon` is seeded ONLY from survivors, so a repo removed in this same
    //     request may be re-added.
    let mut seen_canon: std::collections::HashSet<PathBuf> = active
        .iter()
        .map(|p| canonical_or_self(Path::new(&p.repo)))
        .collect();
    let mut used_aliases: std::collections::HashSet<String> =
        active.iter().map(|p| p.alias.clone()).collect();
    for name in existing_snapshot_dir_names(&repo_root, &run_id) {
        used_aliases.insert(name);
    }

    // (6) Validate every add BEFORE writing anything to disk, so a bad repo yields no
    //     snapshot and no event.
    let mut new_pins: Vec<event_log::RepoPin> = Vec::new();
    for input in &req.add {
        match resolve_one_secondary_pin(input, &primary_canon, &mut seen_canon, &mut used_aliases) {
            Ok(pin) => new_pins.push(pin),
            Err(r) => return repo_edit_refusal_response(&r),
        }
    }

    // (7) Materialise each add BEFORE any append, so the log can never carry a pin
    //     whose snapshot is missing. A snapshot orphaned by a later add's failure is
    //     reaped by the disk-driven `cleanup_run` and invisible to the projection.
    for pin in &new_pins {
        let dest = secondary_snapshot_path(&repo_root, &run_id, &pin.alias);
        if let Err(e) = create_secondary_snapshot(Path::new(&pin.repo), &dest, &pin.sha) {
            error!("patch_run_repos: snapshot failed for {}: {e:#}", pin.repo);
            return repo_edit_refusal_response(&RepoEditRefusal::SnapshotMaterializeFailed {
                repo: pin.repo.clone(),
                error: format!("{e:#}"),
            });
        }
    }

    active.extend(new_pins);

    // Skip the append when nothing actually changed. Adds always change the list
    // (each gets a fresh alias), so this only short-circuits genuine no-ops.
    let before: Vec<&str> = run_state
        .target_repos
        .iter()
        .map(|p| p.alias.as_str())
        .collect();
    let after: Vec<&str> = active.iter().map(|p| p.alias.as_str()).collect();
    if before == after {
        return (StatusCode::OK, Json(run_state)).into_response();
    }

    let event = event_log::Event {
        id: None,
        run_id: run_id.clone(),
        ts: pdo_kernel::clock::now_iso(),
        kind: event_log::EventKind::RunReposEdited,
        node_id: None,
        iter: None,
        payload: Some(serde_json::json!({ "target_repos": active })),
    };
    if let Err(e) = append_event(&state, &event).await {
        error!("patch_run_repos: append failed for run {run_id}: {e:#}");
        return repo_edit_refusal_response(&RepoEditRefusal::AppendFailed {
            error: format!("{e:#}"),
        });
    }

    match reload_run_state(&state, &run_id).await {
        Some((_, refreshed)) => (StatusCode::OK, Json(refreshed)).into_response(),
        None => repo_edit_refusal_response(&RepoEditRefusal::Internal {
            error: "run vanished immediately after a repos edit".into(),
        }),
    }
}

/// Every repository a Trigger fire is about to cut from, **primary first** (#804).
///
/// Takes the Trigger's two stored fields verbatim (the secondary list is raw JSON
/// TEXT, see [`trigger_store::Trigger::target_repos`]) and mirrors the very
/// normalisation `create_run_inner` applies: the scalar is the primary when it is
/// set, else row 0 of the list. Blanks and duplicates fall out — a repo named twice
/// is one repo to fetch, and the ORDER is what makes "primary first" decidable by
/// the caller, which reports only the primary's failure.
///
/// Pure, and separate from the fetch, so the list rule is testable without git: a
/// malformed `target_repos` deferring to a mono-repo fire is the same rule the fire
/// itself applies, and the two must not drift.
fn trigger_fetch_targets(
    target_repo: Option<&str>,
    target_repos_json: Option<&str>,
) -> Vec<String> {
    let secondaries = target_repos_json
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Vec<TargetRepoInput>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.repo);

    let mut targets: Vec<String> = Vec::new();
    for candidate in target_repo
        .map(str::to_string)
        .into_iter()
        .chain(secondaries)
    {
        let path = candidate.trim().to_string();
        if !path.is_empty() && !targets.contains(&path) {
            targets.push(path);
        }
    }
    targets
}

/// Fetch every remote of every repository a Trigger fire will cut from, before the
/// cut (#804, ADR-0070 §1). Answers the **primary**'s failure — the one the Run
/// surfaces — or `None` when it worked.
///
/// A failure NEVER stops the fire: the Run is cut from the last state known locally
/// and carries the reason, because losing a scheduled fire to a network blip is the
/// worse outcome by far (spec #801, story 21). Never a fast-forward either, in any
/// of these repos: there is nobody at a Trigger to arbitrate one (ADR-0070 §2).
///
/// A secondary's failure is logged, not surfaced: its base ref is pinned to a SHA
/// the moment after, and the Run detail states freshness for the source branch —
/// one repo, one sentence. A path that is not a repository is skipped rather than
/// reported, because the create chokepoint is about to refuse the fire naming that
/// very path, and a git-remote error here would bury that message under a fetch one.
async fn fetch_before_trigger_cut(targets: &[String]) -> Option<FetchError> {
    let mut primary_error = None;
    for (index, path) in targets.iter().enumerate() {
        let Ok(repo) = validate_target_repo(path) else {
            continue;
        };
        let outcome = fetch_all_remotes(&repo).await;
        if index == 0 {
            primary_error = outcome;
        } else if let Some(error) = outcome {
            warn!(
                "trigger fire: pre-cut fetch of secondary repo {path} failed: {}",
                error.message
            );
        }
    }
    primary_error
}

// ─────────────────────────────────────────────────────────────────────────────
// Dérive de la source d'un Run (#803, ADR-0070 §4)
// ─────────────────────────────────────────────────────────────────────────────

/// The Run's drift from its source branch since the fork point (#803, CONTEXT.md
/// § « Dérive de la source »).
///
/// A **measurement on local refs**, recomputed on every read and never a stored
/// snapshot: `ahead` is what the Run produced, `behind` what arrived on its source.
/// Nothing here fetches — ADR-0070 §1 is explicit that displaying a Run must not put
/// traffic on someone's repo; the Run view's fetch button is the only refresh, and it
/// is the #802 verb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(tag = "state", rename_all = "snake_case")]
enum SourceDrift {
    Available {
        /// The Run's source branch, verbatim as it was stored.
        source_branch: String,
        /// The fork point the Run was cut at, short.
        fork: String,
        /// Commits the Run's branch made since the fork.
        ahead: u32,
        /// Commits that arrived on the source since the fork — ONE number, the max
        /// of the local branch's and its tracking branch's (spec #801 Q3): the chip
        /// keeps the `n↑ m↓` shape of the form, and the tooltip splits the two.
        behind: u32,
        /// What the LOCAL source branch gained. `None` when the source is itself a
        /// remote-tracking ref (there is no local side to count).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        local_behind: Option<u32>,
        /// The source's tracking branch and what IT gained. `None` when the source
        /// tracks nothing (or already IS a tracking ref).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        upstream: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        upstream_behind: Option<u32>,
        /// When the repo last fetched successfully — the date the `behind` side of
        /// the remote is good as of. Same derivation as the form's (`FETCH_HEAD`).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        last_fetch_at: Option<String>,
    },
    /// The measurement cannot be made — an archived Run whose branch was cleaned
    /// up, a source branch that was deleted. **Not an error**: it is the expected
    /// end of a Run's life, and `reason` says which.
    Unavailable { reason: String },
}

/// Compute a Run's drift from its source (#803). Pure over a repo on disk, so the
/// tests drive it against real temporary repositories rather than a mocked git.
fn source_drift(repo: &Path, run_id: &str, run_state: &event_log::RunState) -> SourceDrift {
    let unavailable = |reason: &str| SourceDrift::Unavailable {
        reason: reason.to_string(),
    };

    let tip = run_refs::tip_branch(run_id);
    if structured_diff::rev_parse(repo, &tip).is_none() {
        return unavailable("the Run's branch no longer exists");
    }
    let fork_ref = run_refs::fork_base(run_state);
    let Some(fork_sha) = structured_diff::rev_parse(repo, fork_ref) else {
        return unavailable("the Run's fork point no longer resolves");
    };
    let Some(source) = run_state.source_branch.as_deref().filter(|s| !s.is_empty()) else {
        return unavailable("this Run recorded no source branch");
    };
    if structured_diff::rev_parse(repo, source).is_none() {
        return unavailable("the source branch no longer exists");
    }

    let Some(ahead) = count_commits(repo, &fork_sha, &tip) else {
        return unavailable("the Run's branch could not be counted");
    };

    // The source may be a local branch or a tracking ref (both are legitimate Run
    // sources, #571). Only a LOCAL one has an upstream to count separately.
    let is_local = ref_exists(repo, &format!("refs/heads/{source}"));
    let local_behind = count_commits(repo, &fork_sha, source);
    let upstream = is_local.then(|| upstream_of(repo, source)).flatten();
    let upstream_behind = upstream
        .as_deref()
        .and_then(|u| count_commits(repo, &fork_sha, &format!("refs/remotes/{u}")));

    SourceDrift::Available {
        source_branch: source.to_string(),
        fork: short_sha(&fork_sha),
        ahead,
        behind: local_behind.unwrap_or(0).max(upstream_behind.unwrap_or(0)),
        local_behind: is_local.then_some(local_behind).flatten(),
        upstream,
        upstream_behind,
        last_fetch_at: last_fetch_at(repo),
    }
}

/// The 7-char form every other PDO surface shows a SHA in.
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Recursively copy `src` into `dst`. Plain files and subdirectories ONLY — sound
/// because the artifacts tree the callers copy holds `output.md` files plus optional
/// images, with no symlinks or nested `.git`.
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Preserve a run's durable outputs from the soon-to-be-destroyed repo-local
/// `run_dir` into `~/.pdo/runs/<id>/`, so an archived run stays inspectable
/// (ADR-0020).
///
/// Best-effort throughout: any failure is logged and swallowed, because archival MUST
/// still succeed and append `RunArchived`. Called from `cleanup_run` **before** the
/// worktree is destroyed — copy first, delete after.
fn preserve_run_outputs_on_archive(
    repo_root: &std::path::Path,
    run_dir: &std::path::Path,
    run_id: &str,
) {
    let Some(durable) = durable_run_dir(run_id) else {
        warn!("#315: $HOME unset — skipping durable output preservation for run {run_id}");
        return;
    };

    // If the target repo *is* $HOME then `run_dir == durable`, and the copy would sit
    // inside the tree `remove_dir_all(run_dir)` is about to delete. A repo merely
    // *under* $HOME is disjoint and fine.
    let repo_is_home = match (
        repo_root.canonicalize(),
        dirs_next_home().and_then(|h| h.canonicalize().ok()),
    ) {
        (Ok(repo), Some(home)) => repo == home,
        _ => false,
    };
    if repo_is_home {
        warn!(
            "#315: repo root == $HOME for run {run_id} — durable store overlaps run dir, skipping preservation"
        );
        return;
    }

    let src_artifacts = run_dir.join("worktree").join(".pdo").join("artifacts");
    let src_yaml = run_dir.join("pipeline.yaml");
    let src_prompts = run_dir.join("pipeline.prompts");

    // Nothing on disk to preserve. Return WITHOUT creating an empty durable dir, or a
    // no-op archive litters `~/.pdo/runs/` with hollow directories.
    if !src_artifacts.is_dir() && !src_yaml.is_file() && !src_prompts.is_dir() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&durable) {
        warn!(
            "#315: failed to create durable dir {}: {e}",
            durable.display()
        );
        return;
    }

    if src_artifacts.is_dir() {
        if let Err(e) = copy_dir_all(&src_artifacts, &durable.join("artifacts")) {
            warn!("#315: failed to preserve artifacts for run {run_id}: {e}");
        }
    }

    if src_yaml.is_file() {
        if let Err(e) = std::fs::copy(&src_yaml, durable.join("pipeline.yaml")) {
            warn!("#315: failed to preserve pipeline.yaml for run {run_id}: {e}");
        }
    }

    if src_prompts.is_dir() {
        if let Err(e) = copy_dir_all(&src_prompts, &durable.join("pipeline.prompts")) {
            warn!("#315: failed to preserve pipeline.prompts for run {run_id}: {e}");
        }
    }
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
    // Always create the prompts dir, even when the template has none, so the file
    // watcher can attach at run creation: it watches non-recursively and there is no
    // later hook to pick the dir up.
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

/// Parse `git diff --numstat` output into a [`LocStat`].
///
/// numstat rows are `added\tdeleted\tpath`. For **binary** files git emits `-` in
/// the count columns, so their +/- contribution is skipped but the file is still
/// counted. An empty input yields `{0, 0, 0}` — the caller distinguishes that clean
/// diff from an uncomputable one (`None`).
fn parse_numstat(stdout: &str) -> event_log::LocStat {
    let mut insertions = 0u64;
    let mut deletions = 0u64;
    let mut files_changed = 0u64;
    for line in stdout.lines() {
        let mut cols = line.splitn(3, '\t');
        let added = cols.next().unwrap_or("");
        let deleted = cols.next().unwrap_or("");
        // A valid numstat row has a third (path) column.
        if cols.next().is_none() {
            continue;
        }
        files_changed += 1;
        if let Ok(a) = added.parse::<u64>() {
            insertions += a;
        }
        if let Ok(d) = deleted.parse::<u64>() {
            deletions += d;
        }
    }
    event_log::LocStat {
        insertions,
        deletions,
        files_changed,
    }
}

/// The base ref for a Run's "what changed" diff: the frozen fork SHA, else the
/// recorded source branch, else `HEAD`.
///
/// The call sites use a uniform 3-dot range: because `pdo/run-<id>` is cut from
/// `fork_sha` and only gains commits on top, `merge-base(fork_sha, run) == fork_sha`,
/// so one range form is correct for all three tiers and never re-introduces the
/// wandering-HEAD defect. NB: this is the Run's fork point, NOT the per-node
/// `NodeStarted.base_sha` (ADR-0036).
fn run_diff_base(run_state: &event_log::RunState) -> &str {
    run_refs::fork_base(run_state)
}

/// Lines changed for a Run, with `.pdo/` excluded. `None` when the diff is
/// uncomputable (git missing, not a repo, run branch gone after cleanup) so the UI
/// renders "—"; a successful but empty diff returns `Some({0, 0, 0})`.
///
/// `base_ref` must be the Run's stable fork point, not the shared checkout's `HEAD`.
/// The **three-dot** range makes the base the merge-base, so the count stays correct
/// as `main` advances past the fork — two-dot would report later main commits as
/// deletions.
fn compute_run_loc(
    repo_root: &std::path::Path,
    run_id: &str,
    base_ref: &str,
) -> Option<event_log::LocStat> {
    let range = format!("{base_ref}...pdo/run-{run_id}");
    let output = std::process::Command::new("git")
        // `:(exclude).pdo/` is a literal pathspec argv element, no shell. `.pdo/` is
        // gitignored here, but external target repos may commit it.
        .args(["diff", "--numstat", &range, "--", ".", ":(exclude).pdo/"])
        .current_dir(repo_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_numstat(&stdout))
}

fn augment_run_state_from_disk(
    run_state: &mut event_log::RunState,
    events: &[event_log::Event],
    projects_root: &std::path::Path,
    stores: &pdo_sandbox::adapters::staging::stores::HarnessStores,
    repo_root: &std::path::Path,
    prices: &price_table::PriceTable,
) {
    // LOC is independent of the run YAML: the run branch lives in `repo_root`, not
    // the run dir, so a missing/unparseable YAML must not suppress an otherwise-valid
    // LOC — hence computing it before the YAML early-return below. The base is
    // resolved into an owned `String` so the immutable borrow of `run_state` ends
    // before the mutable assignment to `.loc`.
    let base = run_diff_base(run_state).to_string();
    run_state.loc = compute_run_loc(repo_root, &run_state.run_id, &base);
    let breakdown = run_cost::compute_run_cost_breakdown_cached(
        events,
        projects_root,
        stores,
        repo_root,
        &run_state.run_id,
        prices,
    );
    run_state.cost = breakdown.cost;

    let mut node_costs: std::collections::HashMap<String, (event_log::NodeCost, bool)> =
        std::collections::HashMap::new();
    for contribution in breakdown
        .contributions
        .iter()
        .filter(|contribution| contribution.scope == run_cost::CostScope::Node)
    {
        let Some(node_id) = contribution.node_id.as_ref() else {
            continue;
        };
        let (cost, has_unknown) = node_costs.entry(node_id.clone()).or_insert_with(|| {
            (
                event_log::NodeCost {
                    usd: Some(0.0),
                    form: contribution.form,
                    // ANDed over the node's contributions below (#707): a node is
                    // "in dollars" only if every execution's reported cost is.
                    reported_in_usd: true,
                    partial: false,
                    unpriced_models: Vec::new(),
                    unavailable_reasons: Vec::new(),
                    executions: 0,
                    readable_executions: 0,
                },
                false,
            )
        });
        if cost.executions > 0 && cost.form != contribution.form {
            cost.form = None;
        } else if cost.executions == 0 {
            cost.form = contribution.form;
        }
        cost.reported_in_usd &= contribution.reported_in_usd;
        if let Some(usd) = contribution.usd {
            if !*has_unknown {
                cost.usd = Some(cost.usd.unwrap_or_default() + usd);
            }
        } else {
            *has_unknown = true;
            cost.usd = None;
            cost.form = None;
            cost.reported_in_usd = false;
        }
        cost.partial |= contribution.partial;
        cost.executions += contribution.executions;
        cost.readable_executions += contribution.readable_executions;
        cost.unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        cost.unavailable_reasons
            .extend(contribution.unavailable_reasons.iter().cloned());
    }
    for (node_id, (mut cost, _)) in node_costs {
        cost.unpriced_models.sort();
        cost.unpriced_models.dedup();
        cost.unavailable_reasons.sort();
        cost.unavailable_reasons.dedup();
        if let Some(node) = run_state.nodes.get_mut(&node_id) {
            node.cost = Some(cost);
        }
    }

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
        node_type: n.node_type.as_str().into(),
        // #653/ADR-0060: the isolation the Run's snapshot froze for this node —
        // what the completion path reads back to know whether a sub-worktree has
        // to be merged, without re-deriving it from a type that no longer says.
        isolated_worktree: n.isolated_worktree,
        // #723/#724/ADR-0064: the « Orchestrator » toggle, frozen at run start —
        // what the strong binding gate reads to know whether this node's NodeRun
        // is held while its child runs are non-terminal.
        orchestrator: n.orchestrator,
        view_x: n.view.as_ref().map(|v| v.x),
        view_y: n.view.as_ref().map(|v| v.y),
        inputs: n.inputs.iter().map(|p| port_brief(p, "left")).collect(),
        outputs: n.outputs.iter().map(|p| port_brief(p, "right")).collect(),
    }
}

fn edge_info_from_pipeline(e: &pipeline::EdgeDef) -> event_log::EdgeInfo {
    event_log::EdgeInfo {
        source_node: e.source.node.clone(),
        source_port: e.source.port().to_string(),
        target_node: e.target.node.clone(),
        target_port: e.target.port.clone(),
        halt_message: e.reason.clone(),
        when_clause: None,
    }
}

pub use guard_runner::GUARD_TIMEOUT_MS_OVERRIDE_ENV;
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

    let triggers = trigger_store::list(db)
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

/// Reap the worktree of a Run aborted at create, and what it left beside it.
///
/// #672: a Run aborted at create may already have snapshotted skills beside the
/// worktree and excluded their paths in the repo's `info/exclude`; take both back
/// before the (then empty) Run directory goes.
fn reap_orphan_run_worktree(repo_root: &Path, worktree_dir: &Path, branch: &str) {
    worktrees::reap_orphan_sub_worktree(repo_root, worktree_dir, branch);
    if let Some(run_id) = branch.strip_prefix("pdo/run-") {
        let _ = pdo_skill::adapters::delivery::remove_exclusions(repo_root, run_id);
        let _ = std::fs::remove_dir_all(skill_delivery::snapshot_root(repo_root, run_id));
    }
    if let Some(run_dir) = worktree_dir.parent() {
        let _ = std::fs::remove_dir(run_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_run::application::complete_node::{
        complete_node_iteration, CompletionAttempt, CompletionSource,
    };
    use pdo_run::application::events::append_event_with;
    use pdo_run::application::release_completion::release_node_completion;
    use pdo_run::application::reopen::embed_reopen_for_targeted_command;
    use pdo_run::application::scheduler::advance as run_advance;
    use pdo_run::domain::completion_refusal;
    use pdo_workspace::adapters::git::branches::list_branches;
    use pdo_workspace::adapters::git::worktrees::{sub_worktree_branch, sub_worktree_path};
    use pdo_workspace::domain::branches::BranchEntry;

    fn branch_named<'a>(branches: &'a [BranchEntry], name: &str) -> &'a BranchEntry {
        branches
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no branch {name} in {branches:?}"))
    }

    /// The merge-back on its **conservative** arm: `keep_conflict` off and no
    /// spawn base, so a conflict still reads `MergeResult::Conflict`.
    fn commit_and_merge_sub_worktree(
        sub_worktree_dir: &Path,
        pipeline_worktree_dir: &Path,
        sub_branch: &str,
        node_id: &str,
        iter: i64,
    ) -> Result<worktrees::MergeResult> {
        worktrees::commit_and_merge_sub_worktree_inner(
            sub_worktree_dir,
            pipeline_worktree_dir,
            sub_branch,
            node_id,
            iter,
            false,
            None,
        )
    }
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

    // --- POST /triggers/guard/test: dry-run a guard, zero side effects.
    // Router oneshots prove the route is mounted (the FP's "404 blocking" risk)
    // and maps the pass / empty-command / absent-repo / bad-repo branches to
    // 200 / 400 / 400 / 400 (#470 added the absent-repo refusal). Deeper
    // pass/skip/cwd + zero-side-effect proofs live in the layer-3a
    // `guard_dry_run.rs` (real daemon); the timeout path is isolated in its own
    // binary because `PDO_GUARD_TIMEOUT_MS` is process-global. ---

    #[tokio::test]
    async fn guard_test_route_is_mounted_and_returns_pass() {
        let state = test_state().await;
        // The dry-run now demands a named repo, like a real fire.
        let repo = state.repo_root.to_string_lossy().into_owned();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers/guard/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "guard_command": "echo hi",
                            "target_repo": repo,
                        }))
                        .unwrap(),
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
        assert_eq!(body["outcome"], "pass");
        assert_eq!(body["stdout"], "hi\n");
        assert_eq!(body["exit_code"], 0);
    }

    #[tokio::test]
    async fn guard_test_empty_command_is_400() {
        let app = build_router(test_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers/guard/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({ "guard_command": "   " }))
                            .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn guard_test_invalid_target_repo_is_400() {
        let app = build_router(test_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers/guard/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "guard_command": "echo hi",
                            "target_repo": "/no/such/path/pdo-guard-dry-run-xyz",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn guard_test_without_target_repo_is_400() {
        // The dry-run executes an arbitrary `sh -c`. Its "zero side effects"
        // invariant is about not creating a Run — it says nothing about what the
        // command does. So it must refuse an unnamed repo like a real fire does,
        // instead of running in the daemon's own working directory.
        let app = build_router(test_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers/guard/test")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "guard_command": "echo hi" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("target_repo"),
            "got: {json}"
        );
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

    /// The audit rows of a test state, newest first, through the platform's
    /// SQLite adapter.
    async fn audit_rows(
        state: &AppState,
        target_kind: Option<&str>,
        target_id: Option<&str>,
    ) -> Vec<pdo_platform::domain::audit::AuditEntry> {
        use pdo_platform::application::ports::AuditLog;
        let filter = pdo_platform::domain::audit::AuditFilter {
            target_kind: target_kind.map(str::to_string),
            target_id: target_id.map(str::to_string),
            ..Default::default()
        };
        pdo_platform::adapters::sqlite_audit_log::SqliteAuditLog::new(state.db.clone())
            .list(&filter)
            .await
            .unwrap()
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
            inner.session =
                contexts::test_session(weak.clone(), inner.repo_root.clone(), inner.port, None);
            inner.run = contexts::test_run(contexts::TestRun {
                state: weak.clone(),
                db: &inner.db,
                bus_tx: &inner.bus_tx,
                session: &inner.session,
                workspace: inner.workspace.clone(),
                skills: inner.skills.clone(),
                repo_root: inner.repo_root.clone(),
                port: inner.port,
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
                port: test_port,
                trigger_tick_lock: tokio::sync::Mutex::new(()),
                recent_writes: SelfWrites::new(),
                run_watcher: Arc::new(Mutex::new(None)),
                // Self-destructing no-op: should any test POST /runs and reach the
                // spawn path, the node session runs `true` and exits immediately —
                // never real claude, never a lingering session.
                tmux_cmd_override: Some("exec true".to_string()),
                docker_cmd_override: None,
                sandbox_home_override: None,
                last_trigger_tick_ms: Arc::new(std::sync::atomic::AtomicI64::new(0)),
                panic_on_trigger_name: None,
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
                port: test_port,
                trigger_tick_lock: tokio::sync::Mutex::new(()),
                recent_writes: SelfWrites::new(),
                run_watcher: Arc::new(Mutex::new(None)),
                tmux_cmd_override: Some("exec true".to_string()),
                docker_cmd_override: None,
                sandbox_home_override: None,
                last_trigger_tick_ms: Arc::new(std::sync::atomic::AtomicI64::new(0)),
                panic_on_trigger_name: None,
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

    // GET /runs/reapable (#128 Track A)
    //
    // Read-only surfacing of terminal runs whose on-disk worktree(s) still
    // exist, so a human or a janitor pipeline can reclaim the disk via the
    // explicit `cleanup_run` command. The runtime NEVER deletes here
    // (ADR-0012(a)); `reaper_never_deletes_worktree` below is the executable
    // pin of that doctrine.

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

    async fn get_reapable(state: &Arc<AppState>, query: &str) -> Vec<serde_json::Value> {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/reapable{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GET /runs/reapable should 200"
        );
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn reapable_lists_terminal_run_with_worktree_present() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "reap-completed";
        seed_run_with_run_events(
            &state,
            run_id,
            "test-pipe",
            &[event_log::EventKind::RunCompleted],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), run_id);

        let runs = get_reapable(&state, "").await;
        assert_eq!(
            runs.len(),
            1,
            "expected the completed worktree-present run: {runs:?}"
        );
        let e = &runs[0];
        assert_eq!(e["run_id"], run_id);
        assert_eq!(e["pipeline_name"], "test-pipe");
        assert_eq!(e["status"], "completed");
        assert_eq!(e["worktree_present"], true);
        assert!(
            e["completed_at"].is_string(),
            "completed_at must be surfaced"
        );
        assert!(
            e["age_secs"].as_i64().map(|a| a >= 0).unwrap_or(false),
            "age_secs must be a non-negative number: {:?}",
            e.get("age_secs")
        );
        // No size requested → the field is omitted (default stays fast).
        assert!(
            e.get("approx_disk_bytes").is_none() || e["approx_disk_bytes"].is_null(),
            "approx_disk_bytes must be absent without ?size=true"
        );
    }

    /// End-to-end janitor loop: a real `/runs/reapable?size=true`
    /// response parses into `reap_policy::ReapableRun`, the pure policy selects
    /// the terminal run, and `cleanup_run` archives it so a re-list is empty.
    /// Ties the two independently-tested halves — endpoint shape and pure policy
    /// — to the `cleanup_run` contract `pdo reap` depends on. Leak-free (all
    /// state is an in-memory DB + a tempdir).
    #[tokio::test]
    async fn reap_policy_selects_and_cleanup_removes_a_real_reapable_run() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "reap-e2e-completed";
        seed_run_with_run_events(
            &state,
            run_id,
            "some-pipe",
            &[event_log::EventKind::RunCompleted],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), run_id);

        // The real endpoint payload (with sizes) deserializes into the policy's
        // own type — no hand-written fixture.
        let raw = get_reapable(&state, "?size=true").await;
        let runs: Vec<crate::reap_policy::ReapableRun> =
            serde_json::from_value(serde_json::Value::Array(raw)).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, run_id);

        // A freshly-seeded run has age ≈ 0; a 0-second completed TTL selects it
        // (the graded default TTLs are pinned by the `reap_policy` unit tests).
        let policy = crate::reap_policy::ReapPolicy {
            completed_ttl_secs: 0,
            self_pipeline: None,
            ..Default::default()
        };
        let plan = policy.plan(&runs);
        assert_eq!(
            plan.reclaim.len(),
            1,
            "the completed worktree-present run must be selected"
        );
        assert_eq!(plan.reclaim[0].run_id, run_id);

        // Reclaim it and confirm the surface no longer lists it (archived + the
        // run dir removed) — the honest proof `pdo reap` re-lists for.
        let resp = cleanup_run(&state, run_id).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let after = get_reapable(&state, "?size=true").await;
        assert!(
            after.is_empty(),
            "a reclaimed run must no longer be reapable: {after:?}"
        );
    }

    #[tokio::test]
    async fn reapable_excludes_live_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        // Running (RunStarted only) and Paused are both live — never reapable,
        // even with a worktree on disk.
        seed_run_with_run_events(&state, "reap-running", "p", &[]).await;
        mk_run_worktree_dir(tmp.path(), "reap-running");
        seed_run_with_run_events(
            &state,
            "reap-paused",
            "p",
            &[event_log::EventKind::RunPaused],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), "reap-paused");

        let runs = get_reapable(&state, "").await;
        assert!(
            runs.is_empty(),
            "live runs must never be reapable: {runs:?}"
        );
    }

    #[tokio::test]
    async fn reapable_excludes_archived_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "reap-archived";
        seed_run_with_run_events(
            &state,
            run_id,
            "p",
            &[
                event_log::EventKind::RunCompleted,
                event_log::EventKind::RunArchived,
            ],
        )
        .await;
        // Even if a stray worktree dir lingers, an archived run never surfaces
        // (its disk was already reclaimed by cleanup_run).
        mk_run_worktree_dir(tmp.path(), run_id);

        let runs = get_reapable(&state, "").await;
        assert!(
            runs.is_empty(),
            "archived runs are already reclaimed: {runs:?}"
        );
    }

    #[tokio::test]
    async fn reapable_reflects_worktree_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "reap-gone";
        seed_run_with_run_events(&state, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
        mk_run_worktree_dir(tmp.path(), run_id);
        assert_eq!(
            get_reapable(&state, "").await.len(),
            1,
            "present worktree → listed"
        );

        // Reclaim the disk (what cleanup_run does to the worktree) → drops off.
        std::fs::remove_dir_all(tmp.path().join(".pdo").join("runs").join(run_id)).unwrap();
        assert!(
            get_reapable(&state, "").await.is_empty(),
            "gone worktree → de-listed"
        );
    }

    #[tokio::test]
    async fn reapable_tags_terminal_status_for_consumer_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        // Surfacing is read-only: Failed/Halted/Skipped all appear, each tagged
        // with its status, so the consumer (janitor recipe) applies its own
        // policy (e.g. completed-only). We do NOT pre-filter failures here.
        seed_run_with_run_events(
            &state,
            "reap-failed",
            "p",
            &[event_log::EventKind::RunFailed],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), "reap-failed");
        seed_run_with_run_events(
            &state,
            "reap-halted",
            "p",
            &[event_log::EventKind::RunHalted],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), "reap-halted");
        seed_run_with_run_events(
            &state,
            "reap-skipped",
            "p",
            &[event_log::EventKind::RunSkipped],
        )
        .await;
        mk_run_worktree_dir(tmp.path(), "reap-skipped");

        let runs = get_reapable(&state, "").await;
        let by_id: std::collections::HashMap<String, String> = runs
            .iter()
            .map(|e| {
                (
                    e["run_id"].as_str().unwrap().to_string(),
                    e["status"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(by_id.get("reap-failed").map(String::as_str), Some("failed"));
        assert_eq!(by_id.get("reap-halted").map(String::as_str), Some("halted"));
        assert_eq!(
            by_id.get("reap-skipped").map(String::as_str),
            Some("skipped")
        );
        assert_eq!(
            runs.len(),
            3,
            "all three terminal-non-archived runs surface"
        );
    }

    #[tokio::test]
    async fn reapable_size_query_reports_disk_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        let run_id = "reap-size";
        seed_run_with_run_events(&state, run_id, "p", &[event_log::EventKind::RunCompleted]).await;
        mk_run_worktree_dir(tmp.path(), run_id);
        std::fs::write(
            tmp.path()
                .join(".pdo")
                .join("runs")
                .join(run_id)
                .join("worktree")
                .join("blob.bin"),
            vec![0u8; 4096],
        )
        .unwrap();

        // ?size=true → a numeric size reflecting the on-disk bytes.
        let runs = get_reapable(&state, "?size=true").await;
        assert_eq!(runs.len(), 1);
        let bytes = runs[0]["approx_disk_bytes"].as_u64();
        assert!(
            bytes.map(|b| b >= 4096).unwrap_or(false),
            "expected approx_disk_bytes >= 4096, got {:?}",
            runs[0].get("approx_disk_bytes")
        );
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

    #[tokio::test]
    async fn get_run_payload_carries_sessions_spawned_and_cost() {
        // GET /runs/:id exposes `sessions_spawned` (a number). Both
        // `loc` and `cost` are derived-on-read and omitted here — the seeded run
        // has no `pdo/run-<id>` branch and no `~/.claude` transcript dir, so each
        // renders "—" in the UI (absent, not `Some(0)`). The positive-path cost
        // aggregation is covered by `run_cost::tests::compute_run_cost_*`.
        let state = test_state().await;
        let run_id = "stats-payload";
        seed_completed_run(&state, run_id).await; // exactly one NodeStarted

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

        assert_eq!(body["sessions_spawned"], 1);
        // No run branch for this seeded run → `loc` is omitted (None), not Some(0).
        assert!(body.get("loc").is_none() || body["loc"].is_null());
        // No transcript dir for this seeded run → `cost` is omitted (None) → "—".
        assert!(body.get("cost").is_none() || body["cost"].is_null());
    }

    #[tokio::test]
    async fn get_run_payload_carries_has_manager_as_an_observed_fact() {
        // Manager on demand: the wire's `has_manager` is an OBSERVED tmux fact,
        // probed at fetch time — never a projected flag. In the test env no tmux
        // session exists, so a freshly seeded run reads `false` even though no
        // event ever said otherwise; a spawned-then-swept session would read the
        // same way, which is exactly the desynchronisation a projected flag
        // cannot survive.
        let state = test_state().await;
        let run_id = "manager-payload";
        seed_completed_run(&state, run_id).await;

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
        assert_eq!(body["has_manager"], false);
    }

    #[tokio::test]
    async fn start_manager_on_a_run_without_a_worktree_is_refused() {
        // Manager on demand: the start endpoint is always available (the
        // `manager_enabled` setting gates only the automatic spawn), but a run
        // whose worktree is gone (archived/cleaned) has nothing for a manager
        // to work in — refused with a 409 naming why, never a spawn into a dead
        // cwd.
        let state = test_state().await;
        let run_id = "manager-start-noworktree";
        seed_completed_run(&state, run_id).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/manager/start"))
                    .body(Body::empty())
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
            body["error"]
                .as_str()
                .unwrap()
                .contains("worktree no longer exists"),
            "got {body}"
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

    #[tokio::test]
    async fn start_manager_on_an_unknown_run_is_a_404() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/no-such-run/manager/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stop_manager_on_a_dead_session_is_a_calm_noop() {
        // Manager on demand: stopping a session that is already gone answers
        // 200 with `stopped: false` — and still writes its ManagerStopped
        // event, which the projection ignores.
        let state = test_state().await;
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/manager-stop-ghost/manager/stop")
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
        assert_eq!(body["ok"], true);
        assert_eq!(body["stopped"], false);
    }

    #[test]
    fn run_detail_projects_the_same_cost_onto_its_parent_node() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude/projects");
        let run_id = "node-cost";
        let working_dir = worktrees::worktree_dir_for_run(repo.path(), run_id);
        let project = projects.join(pdo_harness::claude_transcript::cc_project_dirname(
            &working_dir,
        ));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("sid.jsonl"),
            "{\"type\":\"assistant\",\"requestId\":\"r\",\"message\":{\"id\":\"m\",\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":1000000}}}\n",
        )
        .unwrap();
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
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({
                    "node_type": "agent", "isolated_worktree": false,
                    "harness": "claude",
                    "session_id": "sid"
                })),
            },
        ];
        let mut run_state = event_log::project(&events).unwrap();

        augment_run_state_from_disk(
            &mut run_state,
            &events,
            &projects,
            &pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path()),
            repo.path(),
            &price_table::PriceTable::load(home.path()),
        );

        let node_cost = run_state.nodes["worker"].cost.as_ref().unwrap();
        assert_eq!(node_cost.form, Some(event_log::CostForm::Derived));
        assert!((node_cost.usd.unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(node_cost.executions, 1);
        assert_eq!(run_state.cost.as_ref().unwrap().usd, node_cost.usd.unwrap());
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

    /// Seed a run under `parent`: `RunStarted` (with `parent_run_id`), one
    /// node started, and — unless `live` — the node completed and the run
    /// completed. A live child stays `Running`.
    async fn seed_child_run(state: &Arc<AppState>, run_id: &str, parent: &str, live: bool) {
        let mut events = vec![
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "child-pipe",
                    "parent_run_id": parent,
                    "parent_node_id": "spawner",
                })),
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
        ];
        if !live {
            events.push(event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: None,
            });
            events.push(event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunCompleted,
                node_id: None,
                iter: None,
                payload: None,
            });
        }
        for ev in &events {
            append_event(state, ev).await.unwrap();
        }
    }

    async fn status_of(state: &Arc<AppState>, run_id: &str) -> event_log::RunStatus {
        let events = load_events(&state.db, run_id).await.unwrap();
        event_log::project(&events).unwrap().status
    }

    /// #815: archiving a parent archives its terminated descendants,
    /// recursively (grandchild included), reclaims their run dirs, and does
    /// not choke on a descendant that was already archived by hand.
    #[tokio::test]
    async fn cleanup_run_cascades_to_terminated_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);
        let state = test_state_with_dir(repo).await;

        let parent = "cascade-parent";
        seed_completed_run(&state, parent).await;
        seed_child_run(&state, "cascade-child-a", parent, false).await;
        seed_child_run(&state, "cascade-child-b", parent, false).await;
        seed_child_run(&state, "cascade-grandchild", "cascade-child-a", false).await;
        seed_child_run(&state, "cascade-archived", parent, false).await;
        // Archived by hand beforehand: the cascade must skip it silently.
        assert_eq!(
            post_cleanup_run(&state, "cascade-archived").await,
            StatusCode::OK
        );
        // An unrelated run must be left alone.
        seed_completed_run(&state, "cascade-stranger").await;

        // Real worktrees for two descendants: the cascade must reclaim them.
        let mut worktrees = Vec::new();
        for id in ["cascade-child-b", "cascade-grandchild"] {
            let wt_dir = repo.join(".pdo/runs").join(id).join("worktree");
            create_worktree(repo, &wt_dir, &format!("pdo/run-{id}"), "HEAD").unwrap();
            assert!(wt_dir.exists());
            worktrees.push(wt_dir);
        }

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{parent}/commands"))
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
        // Every non-archived descendant is reported exactly once (the
        // already-archived child is not), and in post-order: the grandchild
        // is torn down before its parent child-a. Sibling order is the
        // store's, not asserted.
        let archived: Vec<&str> = body["archived_children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let mut sorted = archived.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            ["cascade-child-a", "cascade-child-b", "cascade-grandchild"]
        );
        let pos = |id: &str| archived.iter().position(|r| *r == id).unwrap();
        assert!(
            pos("cascade-grandchild") < pos("cascade-child-a"),
            "grandchild must be archived before its parent: {archived:?}"
        );

        for id in [
            parent,
            "cascade-child-a",
            "cascade-child-b",
            "cascade-grandchild",
            "cascade-archived",
        ] {
            assert_eq!(
                status_of(&state, id).await,
                event_log::RunStatus::Archived,
                "{id} must be archived"
            );
        }
        assert_eq!(
            status_of(&state, "cascade-stranger").await,
            event_log::RunStatus::Completed,
            "an unrelated run must not be archived by the cascade"
        );
        for wt_dir in &worktrees {
            assert!(
                !wt_dir.exists(),
                "cascade must remove the descendant worktree {}",
                wt_dir.display()
            );
        }
        for id in ["cascade-child-b", "cascade-grandchild"] {
            assert!(
                !repo.join(".pdo/runs").join(id).exists(),
                "cascade must remove the descendant run dir of {id}"
            );
        }
    }

    /// #815: a live descendant (even a grandchild) refuses the whole archive
    /// with 409 and nothing is archived — not the parent, not the terminated
    /// siblings. Stop the child first, then archive.
    #[tokio::test]
    async fn cleanup_run_refuses_when_a_descendant_is_live() {
        let state = test_state().await;
        let parent = "live-parent";
        seed_completed_run(&state, parent).await;
        seed_child_run(&state, "live-child-done", parent, false).await;
        seed_child_run(&state, "live-child-mid", parent, false).await;
        seed_child_run(&state, "live-grandchild", "live-child-mid", true).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{parent}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind": "cleanup_run"}"#))
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
        assert_eq!(
            body["live_children"],
            serde_json::json!(["live-grandchild"])
        );
        assert!(body["error"].as_str().unwrap().contains("live child run"));

        assert_eq!(
            status_of(&state, parent).await,
            event_log::RunStatus::Completed
        );
        assert_eq!(
            status_of(&state, "live-child-done").await,
            event_log::RunStatus::Completed
        );
        assert_eq!(
            status_of(&state, "live-child-mid").await,
            event_log::RunStatus::Completed
        );
        assert_eq!(
            status_of(&state, "live-grandchild").await,
            event_log::RunStatus::Running
        );

        // Archiving the live grandchild's own terminated sibling directly still works:
        // the cascade only constrains the subtree being archived.
        assert_eq!(
            post_cleanup_run(&state, "live-child-done").await,
            StatusCode::OK
        );
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

        // Forget is durable — a late writer cannot resurrect the run.
        let late = event_log::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event_log::EventKind::NodeStopped,
            node_id: Some("n1".into()),
            iter: Some(1),
            payload: None,
        };
        let err = append_event(&state, &late).await.unwrap_err();
        assert!(
            err.to_string().contains("has been forgotten"),
            "unexpected error: {err}"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(events.is_empty(), "late append must not write any event");

        // GET /runs never re-lists the forgotten run.
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/runs")
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
        assert!(
            !body.to_string().contains(run_id),
            "forgotten run must not reappear in GET /runs: {body}"
        );
    }

    #[tokio::test]
    async fn command_on_forgotten_run_returns_410() {
        let state = test_state().await;
        let run_id = "forget-410";
        seed_completed_run(&state, run_id).await;
        post_cleanup_run(&state, run_id).await;

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

        // extend_cycle used to append CommandIssued before any existence check
        // — the exact resurrection vector of #328.
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"kind": "extend_cycle", "node_id": "n1", "additional_iter": 1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::GONE);

        // node_done is also rejected before any side effect.
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/n1/done"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"iter": 1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::GONE);

        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(events.is_empty(), "log must stay empty: {events:?}");
    }

    #[tokio::test]
    async fn forget_is_atomic_tombstone() {
        let state = test_state().await;
        let run_id = "forget-tombstone";
        seed_completed_run(&state, run_id).await;
        post_cleanup_run(&state, run_id).await;

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

        assert!(run_is_forgotten(&state.run, run_id).await.unwrap());

        // Re-forgetting a forgotten run: the log is gone, so it 404s (unchanged).
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
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
        // The archive is the Run's legacy surface, reached through a `Weak` on
        // the state: the state must outlive the request.
        let app = build_router(state.clone());

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

    /// The Runs list is where a user first meets a failure, and the entire
    /// signal there was a coloured dot. Every `run_failed` had always carried a
    /// `reason`; nothing read it, so nothing could title the dot.
    #[tokio::test]
    async fn the_runs_list_carries_a_failed_run_reason() {
        let state = test_state().await;
        let failed = "failed-with-reason";
        let green = "green-run";
        seed_completed_run(&state, green).await;
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: failed.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "test-pipe" })),
            },
        )
        .await
        .unwrap();
        append_event(
            &state,
            &event_log::Event {
                id: None,
                run_id: failed.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(
                    serde_json::json!({ "reason": "merge conflict on ship: 20 conflicting file(s)" }),
                ),
            },
        )
        .await
        .unwrap();

        let resp = build_router(state.clone())
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

        let entry = body.iter().find(|r| r["run_id"] == failed).unwrap();
        assert_eq!(
            entry["failure_reason"],
            "merge conflict on ship: 20 conflicting file(s)"
        );
        // A green run carries no reason and no empty key to render.
        let entry = body.iter().find(|r| r["run_id"] == green).unwrap();
        assert!(entry.get("failure_reason").is_none());
    }

    #[tokio::test]
    async fn create_run_with_name() {
        let state = test_state().await;
        let run_id = "named-create";

        // Seed a run with a name in the RunStarted payload
        let event = event_log::Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
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
    async fn archived_run_with_null_target_repo_stays_readable() {
        // #470/AC6: the write boundary hardens, the READ side does not. An archived
        // run whose `run_started` legitimately carries `target_repo: null` (≈46 of
        // 101 dev runs) must stay fully readable forever — detail and list — with
        // its repo resolved to the daemon root. This is the ADR-0033 asymmetry.
        let state = test_state().await;
        let run_id = "legacy-null-archived";
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

        // Detail: 200, not a 4xx/5xx from a "required" field that isn't there.
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
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "an archived null-target run must stay readable"
        );

        // List: present, and grouped under the daemon root — no "Unassigned".
        let app = build_router(state.clone());
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
        let row = runs
            .iter()
            .find(|r| r["run_id"] == run_id)
            .expect("the archived run must still be listed");
        assert_eq!(row["status"], "archived");
        assert_eq!(
            row["effective_repo"].as_str(),
            state.repo_root.to_str(),
            "a null-target archived run resolves to the daemon root, not Unassigned"
        );
    }

    #[tokio::test]
    async fn cleanup_run_removes_surviving_sub_worktrees_and_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let run_id = "cleanup-sub-wt";
        let state = test_state_with_dir(repo).await;

        seed_run_with_node(&state, run_id, "impl-1", true).await;
        for ev in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("impl-1".into()),
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
        assert!(matches!(result, worktrees::MergeResult::Success));

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

    /// Shared POST-cleanup helper for the #315 tests. Returns the HTTP status so
    /// callers assert the archive succeeded.
    async fn post_cleanup_run(state: &Arc<AppState>, run_id: &str) -> StatusCode {
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
        resp.status()
    }

    /// R1: archiving a run copies `worktree/.pdo/artifacts` + `pipeline.yaml` to
    /// the durable global dir `$HOME/.pdo/runs/<id>/` BEFORE the worktree is
    /// destroyed, and the run still projects `Archived`.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn cleanup_run_preserves_artifacts_to_durable_dir() {
        let home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "preserve-durable";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        // Sanity: the live artifacts exist before archive; the durable copy does not.
        let run_dir = tmp.path().join(".pdo").join("runs").join(run_id);
        assert!(run_dir
            .join("worktree/.pdo/artifacts/planner/iter-1/plan/output.md")
            .exists());
        let durable = home.path().join(".pdo").join("runs").join(run_id);
        assert!(
            !durable.exists(),
            "durable dir must not exist before archive"
        );

        assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

        // The repo-local run dir (worktree + artifacts) is gone …
        assert!(
            !run_dir.exists(),
            "cleanup must remove the repo-local run dir"
        );
        // … but the durable copy holds the artifacts + pipeline.yaml.
        let preserved_plan = durable.join("artifacts/planner/iter-1/plan/output.md");
        assert!(
            preserved_plan.exists(),
            "planner output must be preserved to the durable dir"
        );
        assert_eq!(
            std::fs::read_to_string(&preserved_plan).unwrap(),
            "# Plan\nDo stuff."
        );
        assert!(
            durable
                .join("artifacts/implementer/iter-1/summary/output.md")
                .exists(),
            "implementer output must be preserved"
        );
        assert!(
            durable.join("pipeline.yaml").exists(),
            "pipeline.yaml must be preserved to the durable dir"
        );

        // Events survive; run projects Archived.
        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            event_log::project(&events).unwrap().status,
            event_log::RunStatus::Archived
        );
    }

    /// #408 (F4): `cleanup_run` merges a sandboxed Run's staged transcripts into
    /// `~/.claude/projects/` (recursively — subagent jsonl included, non-jsonl
    /// siblings excluded) BEFORE the staging is purged, via the real route. This
    /// is the tmux-independent proof that `cleanup_run` → `run_sandbox::cleanup` →
    /// `merge_back` is wired; the synchronous cleanup merge means no waiting.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn cleanup_run_merges_sandbox_transcripts_to_host() {
        let home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "cleanup-merge";
        // `docker rm -f` → `true` (no-op), so no real docker is ever spawned.
        let state = test_state_with_dir_and_docker(tmp.path(), Some("true".to_string())).await;

        // Seed a sandboxed (`full`) run: RunStarted projects `sandbox=full`.
        append_event_with(
            &state.run,
            &event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "sbx-obs",
                    "input": "x",
                    "sandbox": "full",
                })),
            },
        )
        .await
        .unwrap();

        // Plant a staged transcript tree: a top-level jsonl, a nested subagent
        // jsonl (locks the recursion), and a non-jsonl sibling that must NOT land.
        let staged_projects = home
            .path()
            .join(".pdo/sandbox")
            .join(run_id)
            .join("claude-home/projects");
        let enc = "-some-enc-worktree";
        let proj = staged_projects.join(enc);
        std::fs::create_dir_all(proj.join("uuid-1/subagents")).unwrap();
        std::fs::write(proj.join("main.jsonl"), "{\"line\":1}\n").unwrap();
        std::fs::write(proj.join("uuid-1/subagents/side.jsonl"), "{\"line\":2}\n").unwrap();
        std::fs::write(proj.join("notes.txt"), "not a transcript").unwrap();

        assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

        // Merged to the host projects dir, recursively, jsonl-only.
        let host = home.path().join(".claude/projects").join(enc);
        assert_eq!(
            std::fs::read_to_string(host.join("main.jsonl")).unwrap(),
            "{\"line\":1}\n",
            "top-level transcript must be merged verbatim"
        );
        assert_eq!(
            std::fs::read_to_string(host.join("uuid-1/subagents/side.jsonl")).unwrap(),
            "{\"line\":2}\n",
            "nested subagent transcript must be merged (recursion)"
        );
        assert!(
            !host.join("notes.txt").exists(),
            "non-jsonl siblings must NOT be merged"
        );
        // Staging purged after the merge; run Archived.
        assert!(
            !home.path().join(".pdo/sandbox").join(run_id).exists(),
            "cleanup must purge the staging after merging"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        assert_eq!(
            event_log::project(&events).unwrap().status,
            event_log::RunStatus::Archived
        );
    }

    /// R2: after archive (worktree gone), `GET /nodes/<n>/io` serves the correct
    /// payload from the durable store — the shape matches the live-run contract.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn node_io_serves_archived_run_from_durable_dir() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "io-archived";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);
        // The live worktree is gone — the only source left is the durable dir.
        assert!(!tmp
            .path()
            .join(".pdo/runs")
            .join(run_id)
            .join("worktree")
            .exists());

        let app = build_router(state.clone());
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

        // Input (planner's plan) resolves from the preserved tree.
        let inputs = io["inputs"].as_array().unwrap();
        assert_eq!(inputs[0]["port"], "plan");
        assert_eq!(inputs[0]["files"][0]["exists"], true);
        // Output (summary) with its frontmatter parsed from the preserved file.
        let outputs = io["outputs"].as_array().unwrap();
        assert_eq!(outputs[0]["port"], "summary");
        assert_eq!(outputs[0]["files"][0]["exists"], true);
        assert_eq!(outputs[0]["files"][0]["frontmatter"]["verdict"], "PASS");
        assert_eq!(outputs[0]["files"][0]["frontmatter"]["score"], 9);
    }

    /// R3: after archive, `GET /artifact` serves the preserved bytes with the
    /// right MIME, and the path-traversal guard still holds against the durable base.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn artifact_serves_archived_run_content() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "artifact-archived";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);

        let app = build_router(state.clone());
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
        // #796: never cached — a re-spawned node rewrites the same URL.
        assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("# Plan"));

        // Traversal is still rejected against the durable base.
        let app = build_router(state.clone());
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
        assert!(
            resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
            "traversal against durable base must be rejected, got {}",
            resp.status()
        );
    }

    /// R4: forgetting an archived run reclaims the durable store — no leak.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn forget_run_removes_durable_dir() {
        let home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "forget-durable";
        seed_io_test(tmp.path(), run_id);
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        assert_eq!(post_cleanup_run(&state, run_id).await, StatusCode::OK);
        let durable = home.path().join(".pdo").join("runs").join(run_id);
        assert!(durable.exists(), "durable dir must exist after archive");

        // Forget the run.
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

        assert!(
            !durable.exists(),
            "forget must reclaim the durable dir — no leak"
        );
        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(events.is_empty(), "events must be purged after forget");
    }

    // -----------------------------------------------------------------------
    // Declared wait (#588 / ADR-0069)
    // -----------------------------------------------------------------------

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

    #[tokio::test]
    async fn halted_run_appears_in_list_with_correct_status() {
        let state = test_state().await;
        let run_id = "halt-test-1";

        // Seed: run_started → node_started → node_completed → run_halted
        for event in [
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "pipeline_name": "halt-pipe" })),
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
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("reviewer".into()),
                iter: Some(3),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
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

    fn run_git(dir: &std::path::Path, args: &[&str]) {
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
    }

    /// A `git clone` of a freshly-seeded origin, so `work` carries real
    /// remote-tracking refs (`origin/main` — the twin of the local `main` — and
    /// `origin/feature-remote-only`) PLUS the `origin/HEAD` symref every clone
    /// writes, covering the symref-filter case for free. Returns `(origin,
    /// work)`; keep both alive so the refs keep resolving.
    fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir) {
        let origin = tempfile::tempdir().unwrap();
        run_git(origin.path(), &["init", "-b", "main"]);
        run_git(origin.path(), &["config", "user.email", "test@test.com"]);
        run_git(origin.path(), &["config", "user.name", "Test"]);
        std::fs::write(origin.path().join("README.md"), "# test\n").unwrap();
        run_git(origin.path(), &["add", "."]);
        run_git(origin.path(), &["commit", "-m", "init"]);
        run_git(origin.path(), &["checkout", "-b", "feature-remote-only"]);
        std::fs::write(origin.path().join("x.txt"), "x\n").unwrap();
        run_git(origin.path(), &["add", "."]);
        run_git(origin.path(), &["commit", "-m", "feat"]);
        run_git(origin.path(), &["checkout", "main"]);

        let work = tempfile::tempdir().unwrap();
        run_git(
            origin.path(),
            &[
                "clone",
                "--quiet",
                origin.path().to_str().unwrap(),
                work.path().to_str().unwrap(),
            ],
        );
        run_git(work.path(), &["config", "user.email", "test@test.com"]);
        run_git(work.path(), &["config", "user.name", "Test"]);
        (origin, work)
    }

    #[test]
    fn parse_numstat_sums_text_and_counts_binary_without_lines() {
        // Text rows sum +/- ; binary rows show `-` so contribute 0 lines
        // but still count as a changed file. A trailing blank line is ignored.
        let out = "3\t1\tsrc/a.rs\n2\t1\tsrc/b.rs\n-\t-\tassets/logo.png\n\n";
        let loc = parse_numstat(out);
        assert_eq!(
            loc,
            event_log::LocStat {
                insertions: 5,
                deletions: 2,
                files_changed: 3,
            }
        );
    }

    #[test]
    fn parse_numstat_empty_is_zero() {
        assert_eq!(
            parse_numstat(""),
            event_log::LocStat {
                insertions: 0,
                deletions: 0,
                files_changed: 0,
            }
        );
    }

    #[test]
    fn compute_run_loc_three_dot_excludes_pdo_and_drops_when_branch_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };

        // Base file on HEAD so the run branch can delete a line from it.
        std::fs::write(repo.join("code.txt"), "keep1\nremove\nkeep2\n").unwrap();
        git(&["add", "code.txt"]);
        git(&["commit", "-m", "add code"]);

        let default_branch =
            String::from_utf8_lossy(&git(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
                .trim()
                .to_string();
        let run_id = "loc-test";
        let branch = format!("pdo/run-{run_id}");

        // Run branch: a +3/-1 source diff PLUS a tracked `.pdo/` change that must
        // be excluded (so files_changed stays 1, not 2).
        git(&["checkout", "-b", &branch]);
        std::fs::write(repo.join("code.txt"), "keep1\nadd1\nadd2\nadd3\nkeep2\n").unwrap();
        std::fs::create_dir_all(repo.join(".pdo")).unwrap();
        std::fs::write(repo.join(".pdo/noise.txt"), "a\nb\nc\nd\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "run changes"]);
        git(&["checkout", &default_branch]);

        // Tier-3 legacy path: no fork_sha/source_branch recorded, so the base is "HEAD".
        // This test never parks HEAD off the fork line, so the behaviour is unchanged.
        let loc = compute_run_loc(repo, run_id, "HEAD").expect("branch present -> Some");
        assert_eq!(
            loc,
            event_log::LocStat {
                insertions: 3,
                deletions: 1,
                files_changed: 1,
            },
            ".pdo/ excluded so only code.txt counts"
        );

        // Three-dot base = fork point: advancing HEAD after the fork must NOT
        // change the run's LOC (two-dot would report the new commit as deletions).
        std::fs::write(repo.join("other.txt"), "x\ny\n").unwrap();
        git(&["add", "other.txt"]);
        git(&["commit", "-m", "advance main"]);
        let loc_after = compute_run_loc(repo, run_id, "HEAD").expect("still Some");
        assert_eq!(
            loc_after, loc,
            "three-dot base stays at the fork point as HEAD advances"
        );

        // Branch gone (cleanup) -> None -> UI renders "—" (not "0").
        git(&["branch", "-D", &branch]);
        assert!(
            compute_run_loc(repo, run_id, "HEAD").is_none(),
            "missing run branch -> None"
        );
    }

    #[test]
    fn compute_run_loc_clean_diff_is_some_zero_not_none() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);
        let run_id = "loc-clean";
        // Branch at HEAD with no divergence: a successful but EMPTY diff must be
        // Some({0,0,0}) (UI "0"), distinct from None (UI "—").
        let out = std::process::Command::new("git")
            .args(["branch", &format!("pdo/run-{run_id}")])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            compute_run_loc(repo, run_id, "HEAD").expect("branch present -> Some"),
            event_log::LocStat {
                insertions: 0,
                deletions: 0,
                files_changed: 0,
            }
        );
    }

    #[test]
    fn compute_run_loc_ignores_parked_checkout_and_uses_run_source_branch() {
        // #417 regression: the shared checkout is parked on a divergent branch that
        // forks BEFORE the run's fork point. The pre-fix `HEAD...` range collapsed the
        // merge-base onto the common ancestor and swept in every commit the fork branch
        // gained since (the phantom). Basing the diff on the Run's own fork point
        // (`default_branch`, standing in for the frozen `fork_sha`) must report ONLY the
        // run's own 1-line change, regardless of where HEAD is parked.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo); // c0 = "initial" (README.md) on the default branch
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };

        // `init_test_repo` is a bare `git init` → recover the default branch name.
        let default_branch =
            String::from_utf8_lossy(&git(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout)
                .trim()
                .to_string();

        // A divergent branch off c0 with its own commit — the head the operator parks on.
        git(&["checkout", "-b", "integration/prd-403-sandbox"]);
        std::fs::write(repo.join("sandbox.txt"), "sandbox\n").unwrap();
        git(&["add", "sandbox.txt"]);
        git(&["commit", "-m", "parked: unrelated work"]);

        // Advance the default branch two commits PAST the fork — this is the phantom
        // payload the buggy merge-base would sweep in.
        git(&["checkout", &default_branch]);
        std::fs::write(repo.join("file_a.txt"), "a1\na2\na3\n").unwrap();
        git(&["add", "file_a.txt"]);
        git(&["commit", "-m", "main: +3 in file_a"]);
        std::fs::write(repo.join("file_b.txt"), "b1\nb2\n").unwrap();
        git(&["add", "file_b.txt"]);
        git(&["commit", "-m", "main: +2 in file_b"]); // <- the run's fork point

        // Cut the run branch from the default-branch tip and add ONE real line.
        let run_id = "loc-parked-fork";
        let branch = format!("pdo/run-{run_id}");
        git(&["checkout", "-b", &branch]);
        std::fs::write(repo.join("run_only.txt"), "the only real change\n").unwrap();
        git(&["add", "run_only.txt"]);
        git(&["commit", "-m", "run change"]);

        // PARK the checkout on the divergent branch (the exact #417 / #451 condition).
        git(&["checkout", "integration/prd-403-sandbox"]);

        // Based on the run's fork point, ONLY run_only.txt counts — never file_a/file_b.
        // Under the pre-fix `HEAD...` the parked checkout would inflate this to {6,0,3}.
        assert_eq!(
            compute_run_loc(repo, run_id, &default_branch).expect("branch present -> Some"),
            event_log::LocStat {
                insertions: 1,
                deletions: 0,
                files_changed: 1,
            },
            "parked divergent HEAD must not displace the fork-point base"
        );

        // Doc-only variant: a run cut at the fork tip with NO commit changed nothing.
        // With HEAD still parked on the divergent branch, the honest answer is {0,0,0}
        // (the triage's "even non-isolated, zero files" case), not the phantom.
        let doc_run = "loc-parked-non-isolated";
        git(&["branch", &format!("pdo/run-{doc_run}"), &default_branch]);
        assert_eq!(
            compute_run_loc(repo, doc_run, &default_branch).expect("branch present -> Some"),
            event_log::LocStat {
                insertions: 0,
                deletions: 0,
                files_changed: 0,
            },
            "a run that changed nothing reads 0, not the parked-checkout phantom"
        );
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
                port: test_port,
                trigger_tick_lock: tokio::sync::Mutex::new(()),
                recent_writes: SelfWrites::new(),
                run_watcher: Arc::new(Mutex::new(None)),
                // Self-destructing no-op: should any test POST /runs and reach the
                // spawn path, the node session runs `true` and exits immediately —
                // never real claude, never a lingering session.
                tmux_cmd_override: Some("exec true".to_string()),
                docker_cmd_override,
                sandbox_home_override: None,
                last_trigger_tick_ms: Arc::new(std::sync::atomic::AtomicI64::new(0)),
                panic_on_trigger_name: None,
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

    fn write_test_pipeline(dir: &std::path::Path, name: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {name}\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join(format!("{name}.yaml")), yaml).unwrap();
    }

    /// A prompt-optional repo pipeline, so trigger create/repoint never trips the
    /// prompt-required reject rule in these route tests.
    fn write_optional_pipeline(dir: &std::path::Path, id: &str) {
        let pipelines_dir = dir.join(".pdo").join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).unwrap();
        let yaml = format!(
            "name: {id}\nversion: \"1.0\"\nprompt_required: false\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: result\n"
        );
        std::fs::write(pipelines_dir.join(format!("{id}.yaml")), yaml).unwrap();
    }

    #[tokio::test]
    async fn sync_run_pipeline_writes_canonical_prompt_path() {
        // Run-scoped sync must write each prompt to the canonical
        // `<stem>.prompts/<id>.md` (where every reader looks) — never the flat
        // `prompts/<id>.md` dir that no reader consults.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_test_pipeline(dir, "auto-issue");
        let state = test_state_with_dir(dir).await;

        let mut prompts = HashMap::new();
        prompts.insert("worker".to_string(), "the role prompt".to_string());

        let yaml =
            std::fs::read_to_string(dir.join(".pdo").join("pipelines").join("auto-issue.yaml"))
                .unwrap();
        sync_run_pipeline_to_template(&state, "run-xyz", "auto-issue", &yaml, &prompts);

        let pipelines = dir.join(".pdo").join("pipelines");
        let canonical = pipelines.join("auto-issue.prompts").join("worker.md");
        let flat = pipelines.join("prompts").join("worker.md");

        assert_eq!(
            std::fs::read_to_string(&canonical).unwrap(),
            "the role prompt",
            "prompt must be written to the canonical <stem>.prompts/ dir"
        );
        assert!(
            !flat.exists(),
            "the flat prompts/ dir must never be written"
        );
    }

    /// RAII guard that sets HOME to a temporary directory and restores it on drop.
    /// Holds HOME_TEST_LOCK to prevent concurrent env var mutation.
    /// Serializes the tests that point `$HOME` at a temporary directory.
    static HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct FakeHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: tempfile::TempDir,
        prev: Option<String>,
    }

    impl FakeHome {
        fn new() -> Self {
            // Poison-tolerant: the lock only serializes HOME mutation; a panic
            // in another test must not cascade here.
            let lock = HOME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    async fn patch_trigger_repoints_to_another_pipeline() {
        // PATCH /triggers/{id} {pipeline_id} must move the trigger to the
        // new pipeline — id *and* denormalised name — and persist it, instead of
        // serde silently dropping an unknown field. Driven end-to-end through the
        // router so the deserialization path itself is under test.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        write_optional_pipeline(tmp.path(), "weekly-pipe");
        // A Trigger cannot be created without a target repo any more.
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state.clone());

        // Create a trigger bound to watch-pipe.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "PDO Weekly",
                            "pipeline_id": "watch-pipe",
                            "cron": "0 4 * * 1",
                            "input_template": "x",
                            "target_repo": tmp.path().to_string_lossy(),
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(created["pipeline_id"], "watch-pipe");
        assert_eq!(created["pipeline_name"], "watch-pipe");

        // Repoint it to weekly-pipe.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/triggers/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "pipeline_id": "weekly-pipe" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["pipeline_id"], "weekly-pipe");
        assert_eq!(updated["pipeline_name"], "weekly-pipe");

        // Persisted in the store, not merely echoed back.
        let after = trigger_store::get(&state.db, &id).await.unwrap().unwrap();
        assert_eq!(after.pipeline_id, "weekly-pipe");
        assert_eq!(after.pipeline_name, "weekly-pipe");
        // The repoint revived the next fire from the trigger's own cron.
        assert!(after.next_fire_at.is_some());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn patch_trigger_rejects_repoint_to_unknown_pipeline() {
        // A repoint to a non-existent pipeline is a loud 400, never a
        // silent success that leaves the trigger dangling.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        // A Trigger cannot be created without a target repo any more.
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "T",
                            "pipeline_id": "watch-pipe",
                            "cron": "0 4 * * 1",
                            "input_template": "x",
                            "target_repo": tmp.path().to_string_lossy(),
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/triggers/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "pipeline_id": "ghost-pipe" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // The trigger is left exactly as it was.
        let after = trigger_store::get(&state.db, &id).await.unwrap().unwrap();
        assert_eq!(after.pipeline_id, "watch-pipe");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_trigger_writes_one_audit_row() {
        // A successful create leaves exactly one `trigger.created` row whose
        // `after` is the FULL trigger (not a bare id) and whose `before` is empty.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        let rows = audit_rows(&state, None, None).await;
        assert_eq!(rows.len(), 1, "create leaves exactly one audit row");
        let e = &rows[0];
        assert_eq!(e.action, "trigger.created");
        assert_eq!(e.target_kind.as_deref(), Some("trigger"));
        assert_eq!(e.target_id.as_deref(), Some(id.as_str()));
        // No header on `create_trigger_ok`'s request → the honest default.
        assert_eq!(e.actor_hint, "unknown");
        assert!(e.before.is_none(), "create has no before");
        let after = e.after.as_ref().expect("create carries an after snapshot");
        assert_eq!(after["id"], serde_json::json!(id));
        assert_eq!(after["name"], serde_json::json!("T"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn delete_trigger_audits_the_full_before_snapshot() {
        // Load-bearing (#507 §6): `trigger_store::delete` returns `bool`, so the
        // handler must `get()` the row BEFORE deleting or the audit degrades to a
        // dead UUID. Assert the `before` is the WHOLE row, `after` empty.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/triggers/{id}"))
                    .header("X-PDO-Actor", "ui")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let rows = audit_rows(&state, Some("trigger"), Some(&id)).await;
        let del = rows
            .iter()
            .find(|e| e.action == "trigger.deleted")
            .expect("a delete row");
        assert_eq!(del.actor_hint, "ui", "X-PDO-Actor header plumbed through");
        assert!(del.after.is_none(), "delete has no after");
        let before = del
            .before
            .as_ref()
            .expect("delete carries the pre-image, NOT a bare UUID");
        assert_eq!(before["id"], serde_json::json!(id));
        assert_eq!(before["name"], serde_json::json!("T"));
        assert_eq!(before["cron"], serde_json::json!("0 4 * * 1"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn patch_disable_audits_before_true_after_false() {
        // The #505 discriminator: a manual disable must leave a row where
        // before.enabled=true and after.enabled=false — that is what tells a
        // "disabled by a gesture" apart from "no longer scheduled".
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/triggers/{id}"))
                    .header("content-type", "application/json")
                    .header("X-PDO-Actor", "ui")
                    .body(Body::from(
                        serde_json::json!({ "enabled": false }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rows = audit_rows(&state, Some("trigger"), Some(&id)).await;
        let upd = rows
            .iter()
            .find(|e| e.action == "trigger.updated")
            .expect("an update row");
        assert_eq!(upd.actor_hint, "ui");
        assert_eq!(
            upd.before.as_ref().unwrap()["enabled"],
            serde_json::json!(true)
        );
        assert_eq!(
            upd.after.as_ref().unwrap()["enabled"],
            serde_json::json!(false)
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn pause_triggers_audits_the_flag_flip() {
        // Pause is the instance-scoped seam: target_kind=instance, no target_id,
        // and both directions of the flip are captured from `req.paused`.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers/pause")
                    .header("content-type", "application/json")
                    .header("X-PDO-Actor", "ui")
                    .body(Body::from(
                        serde_json::json!({ "paused": true }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rows = audit_rows(&state, Some("instance"), None).await;
        let p = rows
            .iter()
            .find(|e| e.action == "triggers.pause_changed")
            .expect("a pause row");
        assert_eq!(p.actor_hint, "ui");
        assert_eq!(p.target_kind.as_deref(), Some("instance"));
        assert!(p.target_id.is_none(), "instance-global has no target_id");
        assert_eq!(
            p.before.as_ref().unwrap()["paused"],
            serde_json::json!(false)
        );
        assert_eq!(p.after.as_ref().unwrap()["paused"], serde_json::json!(true));
    }

    /// Create a Trigger through the router, returning its id. `extra` is merged
    /// into the body so callers can vary one axis at a time.
    async fn create_trigger_ok(state: &Arc<AppState>, repo: &std::path::Path) -> String {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "T",
                            "pipeline_id": "watch-pipe",
                            "cron": "0 4 * * 1",
                            "input_template": "x",
                            "target_repo": repo.to_string_lossy(),
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn patch_trigger_resp(
        state: &Arc<AppState>,
        id: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/triggers/{id}"))
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
    #[allow(clippy::await_holding_lock)]
    async fn create_trigger_without_target_repo_is_400() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let app = build_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/triggers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "T",
                            "pipeline_id": "watch-pipe",
                            "cron": "0 4 * * 1",
                            "input_template": "x"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("target_repo"),
            "got: {json}"
        );
        assert!(
            trigger_store::list(&state.db).await.unwrap().is_empty(),
            "no Trigger may be persisted on refusal"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn patch_trigger_clearing_target_repo_is_400() {
        // An explicit `null` is now REACHABLE (double-option wiring) — precisely
        // so it can be refused. Before #470 serde swallowed it into a no-op, so
        // the UI's "blank the repo field" appeared to work and did nothing.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        let (status, json) =
            patch_trigger_resp(&state, &id, serde_json::json!({ "target_repo": null })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("target_repo"),
            "got: {json}"
        );

        // Unchanged in the store — a refused PATCH applies nothing at all.
        let after = trigger_store::get(&state.db, &id).await.unwrap().unwrap();
        assert_eq!(after.target_repo.as_deref(), tmp.path().to_str());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn patch_trigger_with_blank_target_repo_is_400() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        for blank in ["", "   "] {
            let (status, json) =
                patch_trigger_resp(&state, &id, serde_json::json!({ "target_repo": blank })).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "blank {blank:?}: {json}");
            let after = trigger_store::get(&state.db, &id).await.unwrap().unwrap();
            assert_eq!(after.target_repo.as_deref(), tmp.path().to_str());
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn patch_trigger_without_target_repo_leaves_it_untouched() {
        // The correct reading of AC4: PATCH is a partial MERGE. An absent
        // `target_repo` is not a clear — that is what makes `{"enabled": false}`
        // from the list toggle a legal body. This test exists to stop a future
        // "fix" from reading AC4 literally and breaking the toggle.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_optional_pipeline(tmp.path(), "watch-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;
        let id = create_trigger_ok(&state, tmp.path()).await;

        let (status, json) =
            patch_trigger_resp(&state, &id, serde_json::json!({ "enabled": false })).await;
        assert_eq!(status, StatusCode::OK, "body: {json}");

        let after = trigger_store::get(&state.db, &id).await.unwrap().unwrap();
        assert!(!after.enabled, "the toggle must have applied");
        assert_eq!(
            after.target_repo.as_deref(),
            tmp.path().to_str(),
            "an absent target_repo leaves the stored value alone"
        );
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
    fn run_create_skills_arg_accepts_ids_and_json() {
        // Comma-separated ids become the wire shape of the Run tier's SkillRef
        // list — `{id}` objects whose `name` the daemon defaults (#668).
        let parsed = parse_skills_arg("alpha, beta ,,gamma").unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([{ "id": "alpha" }, { "id": "beta" }, { "id": "gamma" }])
        );
        // A JSON list passes through verbatim — `{id, name}` labels survive.
        let parsed = parse_skills_arg(r#"[{"id":"a","name":"A"}]"#).unwrap();
        assert_eq!(parsed, serde_json::json!([{ "id": "a", "name": "A" }]));
        assert!(parse_skills_arg("[broken").is_err());
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
                "name: io-test-pipe\nnodes:\n{START_END_YAML}  - id: planner\n    name: planner\n    type: agent\n    isolated_worktree: false\n    inputs:\n      - name: task\n    outputs:\n      - name: plan\n  - id: implementer\n    name: implementer\n    type: agent\n    isolated_worktree: true\n    inputs:\n      - name: plan\n    outputs:\n      - name: summary\nedges:\n  - source: {{ node: planner, port: plan }}\n    target: {{ node: implementer, port: plan }}\n"
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
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({
                    "pipeline_name": "io-test-pipe",
                    "input": "test",
                    "node_defs": [
                        { "id": "planner", "node_type": "agent", "isolated_worktree": false, "inputs": ["task"], "outputs": ["plan"] },
                        { "id": "implementer", "node_type": "agent", "isolated_worktree": true, "inputs": ["plan"], "outputs": ["summary"] }
                    ],
                    "edges": [
                        { "source_node": "planner", "source_port": "plan", "target_node": "implementer", "target_port": "plan" }
                    ]
                })),
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeStarted,
                node_id: Some("planner".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event_log::EventKind::NodeCompleted,
                node_id: Some("planner".into()),
                iter: Some(1),
                payload: None,
            },
            event_log::Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
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

    // #333 / ADR-0028 security invariant: an `output.html` artifact is served as
    // `text/markdown`, NEVER `text/html`. Serving it as a top-level HTML
    // document at the daemon origin (no auth, no CSP) would execute
    // agent-authored (prompt-injectable) script. The html artifact is only ever
    // rendered client-side inside a sandboxed iframe. This test fails loudly if
    // a future contributor "helpfully" maps `.html → text/html`.
    #[tokio::test]
    async fn artifact_serves_html_as_text_markdown_never_text_html() {
        let tmp = tempfile::tempdir().unwrap();
        let run_id = "artifact-html-mime";
        seed_io_test(tmp.path(), run_id);
        // Seed a real html artifact under a port dir (canonicalize needs it).
        let html_body = "<!doctype html><html><body><h1>Report</h1>\
             <script>window.__pdo_pwned=1</script></body></html>";
        std::fs::write(
            tmp.path()
                .join(".pdo/runs")
                .join(run_id)
                .join("worktree/.pdo/artifacts/planner/iter-1/plan/output.html"),
            html_body,
        )
        .unwrap();
        let state = test_state_with_dir(tmp.path()).await;
        seed_io_run_events(&state, run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/artifact?path=planner/iter-1/plan/output.html"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert_eq!(ct, "text/markdown");
        assert_ne!(ct, "text/html");
        // The body is returned verbatim (the daemon does not sanitize) — the
        // safety comes entirely from the non-executable content type + the
        // client-side sandboxed iframe.
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(text, html_body);
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

    // --- Transition guard wiring (#212, closes #195 #196 #197 #198 #201) ---

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

    /// POST a command and return `(status, content-type, body)` — the triplet
    /// that IS the wire contract. Order matters: `into_body()` consumes the
    /// response, so status and headers must be read first.
    async fn command_triplet(
        state: &Arc<AppState>,
        run_id: &str,
        body: &str,
    ) -> (StatusCode, String, String) {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/commands"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = String::from_utf8_lossy(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .into_owned();
        (status, content_type, body)
    }

    const JSON_CT: &str = "application/json";
    // Every `/commands` error body is JSON now — no text/plain constant
    // remains, which is itself the invariant (a text body would need re-introducing
    // the constant, a visible diff).

    #[tokio::test]
    async fn every_kind_status_and_content_type_on_an_unstarted_run() {
        // Against a run with an empty event log this surface answers in two shapes:
        //   * four kinds answer 200 and APPEND — they are designed to work before the
        //     run exists (`inject_artifact` writes the file the run has not yet
        //     produced; `rename_run` appends blind);
        //   * the rest answer 404, all in JSON `{"error":"run not found"}`.
        //
        // Hoisting the `load_events -> project` preamble to the top of the handler —
        // the single most tempting refactor here — flips the first group to 404, and
        // no other test in the repo would notice.
        //
        // One run_id per row: the 200 rows APPEND, so a shared id would let row N
        // poison row N+1's projection.
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state_with_dir(tmp.path()).await;

        // (body, status, content-type, body, does it leave events behind?)
        let matrix: [(&str, StatusCode, &str, &str, bool); 14] = [
            (
                r#"{"kind":"mark_node_done","node_id":"n1"}"#,
                StatusCode::OK,
                JSON_CT,
                r#"{"ok":true}"#,
                true,
            ),
            (
                r#"{"kind":"extend_cycle","node_id":"n1","additional_iter":1}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"bump_region","region_id":"r1","additional_iter":1}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"end_region","region_id":"r1"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"pause_run"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"resume_run"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"kill_node","node_id":"n1"}"#,
                StatusCode::OK,
                JSON_CT,
                r#"{"ok":true}"#,
                true,
            ),
            // 404, and since #489 WITHOUT A TRACE. Until then this arm killed
            // the (absent) tmux session and appended its `CommandIssued` BEFORE
            // re-projecting, so the "run not found" it answered arrived after two
            // side effects. The `false` in the last column IS the fix: ADR-0037 §3
            // extends ADR-0025's "validate before writing" to the KILL, so every
            // knowable refusal — this one included — is raised ahead of both.
            // (`project` still returns `None` here because the log carries no
            // `RunStarted`, #328 / `event_log.rs`, not because it is empty.)
            // Body is JSON since #491/#601.
            (
                r#"{"kind":"restart_node","node_id":"n1"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"start_node","node_id":"n1"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                true,
            ),
            (
                r#"{"kind":"inject_artifact","path":"a/b.md","content":"x"}"#,
                StatusCode::OK,
                JSON_CT,
                r#"{"ok":true}"#,
                true,
            ),
            (
                r#"{"kind":"rename_run","name":"nn"}"#,
                StatusCode::OK,
                JSON_CT,
                r#"{"ok":true}"#,
                true,
            ),
            (
                r#"{"kind":"cleanup_run"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"retry_all"}"#,
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#,
                false,
            ),
            (
                r#"{"kind":"bogus_command"}"#,
                StatusCode::BAD_REQUEST,
                JSON_CT,
                r#"{"error":"unknown command: bogus_command"}"#,
                false,
            ),
        ];

        for (i, (req_body, want_status, want_ct, want_body, want_events)) in
            matrix.iter().enumerate()
        {
            let run_id = format!("unstarted-{i}");
            let (status, ct, body) = command_triplet(&state, &run_id, req_body).await;
            assert_eq!(
                (status, ct.as_str(), body.as_str()),
                (*want_status, *want_ct, *want_body),
                "row {i} {req_body}"
            );
            let left_events = !load_events(&state.db, &run_id).await.unwrap().is_empty();
            assert_eq!(
                left_events, *want_events,
                "row {i} {req_body}: side-effect trace changed"
            );
        }
    }

    #[tokio::test]
    async fn every_kind_is_410_on_a_forgotten_run() {
        // ADR-0024 / #328: the tombstone gate runs BEFORE the kind match, so a
        // forgotten run answers 410 to everything — including a kind nobody
        // knows and a body missing its required fields. Move the parse ahead of
        // the gate and those two become 400. One kind was covered before this.
        let state = test_state().await;
        let run_id = "forgotten-all-kinds";
        seed_completed_run(&state, run_id).await;
        post_cleanup_run(&state, run_id).await;
        let resp = build_router(state.clone())
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

        for req_body in [
            r#"{"kind":"extend_cycle","node_id":"n1","additional_iter":1}"#,
            r#"{"kind":"cleanup_run"}"#,
            r#"{"kind":"inject_artifact","path":"a/b.md","content":"x"}"#,
            r#"{"kind":"rename_run","name":"nn"}"#,
            // No `node_id` — the 400 for a missing field must NOT outrank 410.
            r#"{"kind":"kill_node"}"#,
            // Unknown kind — likewise.
            r#"{"kind":"bogus_command"}"#,
            // The tombstone must outrank every NEW projection this arm
            // gained too — the `409`s, the `400 node_not_found` and the `500`.
            r#"{"kind":"restart_node","node_id":"n1","iter":1}"#,
        ] {
            let (status, ct, body) = command_triplet(&state, run_id, req_body).await;
            assert_eq!(
                (status, ct.as_str(), body.as_str()),
                (
                    StatusCode::GONE,
                    JSON_CT,
                    format!(r#"{{"error":"run {run_id} has been forgotten"}}"#).as_str()
                ),
                "{req_body}"
            );
        }

        let events = load_events(&state.db, run_id).await.unwrap();
        assert!(events.is_empty(), "log must stay empty: {events:?}");
    }

    #[tokio::test]
    async fn cleanup_run_error_bodies_are_json() {
        // `cleanup_run`'s error bodies are JSON like every sibling on
        // this surface — the front's `resp.json().catch()` reads them instead of
        // swallowing text/plain. The 200/409/404 status split is unchanged (it is
        // an operator contract the disk-janitor recipe reads as "reclaimed /
        // already archived / gone"); only the wire shape of the two error bodies
        // moved from text to JSON.
        let state = test_state().await;
        let run_id = "cleanup-bodies";
        seed_completed_run(&state, run_id).await;

        let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"cleanup_run"}"#).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (
                StatusCode::OK,
                JSON_CT,
                r#"{"archived_children":[],"status":"archived"}"#
            )
        );

        let (status, ct, body) = command_triplet(&state, run_id, r#"{"kind":"cleanup_run"}"#).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (
                StatusCode::CONFLICT,
                JSON_CT,
                r#"{"error":"run is already archived"}"#
            )
        );

        let (status, ct, body) =
            command_triplet(&state, "no-such-run", r#"{"kind":"cleanup_run"}"#).await;
        assert_eq!(
            (status, ct.as_str(), body.as_str()),
            (
                StatusCode::NOT_FOUND,
                JSON_CT,
                r#"{"error":"run not found"}"#
            )
        );
    }

    // The truthful-response matrix (ADR-0025), without a daemon: 400 unknown target,
    // 409 wrong mechanism, 200 noop, 200 spawned — all reachable from the in-crate
    // router. Case D does attempt a real `tmux` call, but its failure is swallowed, so
    // the verdict does not depend on tmux SUCCEEDING.
    //
    // The pipeline is written RUN-SCOPED on purpose: `resolve_run_pipeline_path`
    // prefers it, so these tests can never silently pick up a real pipeline out of the
    // developer's `$HOME/.pdo/pipelines`, which has no daemon override.

    #[test]
    fn required_target_repo_rejects_absent() {
        let err = required_target_repo(None).unwrap_err();
        assert!(err.contains("target_repo"), "message was: {err}");
        // Actionable on purpose: the caller must learn what to pass.
        assert!(err.contains("absolute path"), "message was: {err}");
    }

    #[test]
    fn required_target_repo_rejects_empty() {
        let err = required_target_repo(Some("")).unwrap_err();
        assert!(err.contains("target_repo"), "message was: {err}");
    }

    #[test]
    fn required_target_repo_rejects_whitespace() {
        let err = required_target_repo(Some("   \t ")).unwrap_err();
        assert!(err.contains("target_repo"), "message was: {err}");
    }

    #[test]
    fn required_target_repo_trims_and_accepts() {
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        let padded = format!("  {}  ", tmp.path().to_str().unwrap());
        // The trimmed value is the canonical one: what we validate is what the
        // caller gets back, so nothing downstream can diverge from it.
        assert_eq!(required_target_repo(Some(&padded)).unwrap(), tmp.path());
    }

    // ── #802 / ADR-0070: écart amont, dates, and the fetch verb ──────────────
    //
    // Every test below drives a REAL pair of repos (a clone of a filesystem
    // "origin"), because the whole point of the feature is what git reports about
    // an actual divergence. The prior art is `validate_source_branch`'s suite.

    /// Add `count` commits on `dir`'s current branch. Used to move an origin ahead
    /// of its clone without a push (the origin here has a worktree, so a push to
    /// its checked-out branch would be refused).
    fn commit_n(dir: &std::path::Path, count: usize, prefix: &str) {
        for i in 0..count {
            let file = dir.join(format!("{prefix}-{i}.txt"));
            std::fs::write(&file, format!("{prefix} {i}\n")).unwrap();
            run_git(dir, &["add", "."]);
            run_git(dir, &["commit", "-m", &format!("{prefix} {i}")]);
        }
    }

    // ── #804: the pre-cut fetch of a Trigger fire ───────────────────────────────

    #[test]
    fn trigger_fetch_targets_leads_with_the_primary_and_fetches_a_repo_once() {
        let targets = trigger_fetch_targets(
            Some("/repos/primary"),
            Some(r#"[{"repo":"/repos/primary"},{"repo":"/repos/lib"}]"#),
        );
        assert_eq!(
            targets,
            vec!["/repos/primary".to_string(), "/repos/lib".to_string()],
            "the primary leads (its failure is the one the Run shows) and the row-0 \
             echo of it is not a second repo to fetch"
        );
    }

    #[test]
    fn trigger_fetch_targets_takes_row_zero_as_the_primary_when_the_scalar_is_blank() {
        // The multi-repo modal carries the primary in row 0; `create_run_inner`
        // normalises exactly this way, and the fetch order must agree with it.
        let targets = trigger_fetch_targets(
            Some("   "),
            Some(r#"[{"repo":"/repos/a"},{"repo":"/repos/b"}]"#),
        );
        assert_eq!(
            targets,
            vec!["/repos/a".to_string(), "/repos/b".to_string()]
        );
    }

    #[test]
    fn trigger_fetch_targets_defers_to_a_mono_repo_fire_on_an_unreadable_list() {
        // Same rule the fire itself applies to `target_repos`: blank, absent or
        // malformed all mean "mono-repo", never "no repo at all".
        for stored in [None, Some(""), Some("not json")] {
            assert_eq!(
                trigger_fetch_targets(Some("/repos/only"), stored),
                vec!["/repos/only".to_string()],
                "stored list {stored:?} must leave the primary standing"
            );
        }
    }

    #[tokio::test]
    async fn the_pre_cut_fetch_makes_the_source_fresh_at_the_fire() {
        let (origin, work) = init_repo_with_remote();
        // Someone pushed since the last fetch — the very case a scheduled fire
        // used to miss, cutting an "up to date" worktree from a stale ref.
        commit_n(origin.path(), 2, "pushed-since");
        assert_eq!(
            branch_named(&list_branches(work.path()).unwrap(), "main").behind,
            Some(0),
            "stale refs still look level before the fire"
        );

        let targets = trigger_fetch_targets(Some(work.path().to_str().unwrap()), None);
        assert_eq!(fetch_before_trigger_cut(&targets).await, None);

        assert_eq!(
            branch_named(&list_branches(work.path()).unwrap(), "main").behind,
            Some(2),
            "the cut that follows sees the two commits pushed since"
        );
    }

    #[tokio::test]
    async fn a_failed_pre_cut_fetch_names_the_reason_and_still_lets_the_fire_through() {
        let (_origin, work) = init_repo_with_remote();
        run_git(
            work.path(),
            &["remote", "set-url", "origin", "/nonexistent/repo.git"],
        );

        let targets = trigger_fetch_targets(Some(work.path().to_str().unwrap()), None);
        let error = fetch_before_trigger_cut(&targets)
            .await
            .expect("the reason the Run will carry");
        assert!(
            !error.message.is_empty(),
            "git's own words are what the Run detail quotes"
        );
        // ADR-0070 §1 / story 21: nothing here refuses the fire. The source ref is
        // still resolvable, so the cut that follows happens on local state.
        assert!(validate_source_branch(work.path(), "main").is_ok());
    }

    #[tokio::test]
    async fn the_pre_cut_fetch_refreshes_secondaries_but_only_the_primary_is_reported() {
        let (_primary_origin, primary) = init_repo_with_remote();
        let (secondary_origin, secondary) = init_repo_with_remote();
        commit_n(secondary_origin.path(), 1, "secondary-move");

        let targets = trigger_fetch_targets(
            Some(primary.path().to_str().unwrap()),
            Some(&format!(
                r#"[{{"repo":"{}"}}]"#,
                secondary.path().to_str().unwrap()
            )),
        );
        assert_eq!(fetch_before_trigger_cut(&targets).await, None);

        assert_eq!(
            branch_named(&list_branches(secondary.path()).unwrap(), "main").behind,
            Some(1),
            "a secondary is cut from the same repo state as the primary, so it \
             is refreshed by the same gesture"
        );
    }

    #[tokio::test]
    async fn the_pre_cut_fetch_stays_silent_about_a_path_that_is_not_a_repository() {
        // The create chokepoint is about to refuse this fire naming the path; a
        // `git remote` error here would bury that message under a fetch one.
        let tmp = tempfile::tempdir().unwrap();
        let targets = trigger_fetch_targets(Some(tmp.path().to_str().unwrap()), None);
        assert_eq!(fetch_before_trigger_cut(&targets).await, None);
    }

    fn head_sha(repo: &std::path::Path) -> String {
        structured_diff::rev_parse(repo, "HEAD").expect("HEAD resolves")
    }

    // ── Dérive de la source (#803, ADR-0070 §4) ──────────────────────────────

    /// A repo carrying a Run: a `pdo/run-<id>` branch cut from `main`, with the
    /// fork SHA frozen in the projected state the way `RunStarted` does.
    fn repo_with_run(
        run_id: &str,
        source: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, event_log::RunState) {
        let (origin, work) = init_repo_with_remote();
        let fork = head_sha(work.path());
        run_git(
            work.path(),
            &["branch", &run_refs::tip_branch(run_id), &fork],
        );
        let mut run_state = event_log::RunState::new(run_id.into(), "pipe".into());
        run_state.target_repo = Some(work.path().display().to_string());
        run_state.source_branch = Some(source.into());
        run_state.fork_sha = Some(fork);
        (origin, work, run_state)
    }

    /// Commit `n` times on a branch without disturbing the checkout: a worktree in
    /// a temp dir, committed in, then removed.
    fn commit_on_branch(repo: &std::path::Path, branch: &str, n: usize, prefix: &str) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("wt");
        run_git(
            repo,
            &["worktree", "add", "-q", path.to_str().unwrap(), branch],
        );
        commit_n(&path, n, prefix);
        run_git(
            repo,
            &["worktree", "remove", "--force", path.to_str().unwrap()],
        );
    }

    #[test]
    fn source_drift_counts_both_sides_since_the_fork() {
        let run_id = "eaea593";
        let (origin, work, run_state) = repo_with_run(run_id, "main");
        // One commit on the Run's branch, one on the local source, three on origin.
        commit_on_branch(work.path(), &run_refs::tip_branch(run_id), 1, "run-side");
        commit_n(work.path(), 1, "source-side");
        commit_n(origin.path(), 3, "origin-side");
        run_git(work.path(), &["fetch", "--quiet", "origin"]);

        let drift = source_drift(work.path(), run_id, &run_state);

        match drift {
            SourceDrift::Available {
                ahead,
                behind,
                local_behind,
                upstream,
                upstream_behind,
                source_branch,
                ..
            } => {
                assert_eq!(source_branch, "main");
                assert_eq!(ahead, 1, "what the Run produced");
                // ONE number on the chip (spec #801 Q3): the MAX of the two sides,
                // never their sum — the local commit and the three on origin are
                // different commits off the same fork, and "4 arrived" would claim
                // a distance nobody has to travel. The split lives in the tooltip,
                // which is exactly the design's `main +1 · origin/main +3` line.
                assert_eq!(behind, 3);
                assert_eq!(local_behind, Some(1));
                assert_eq!(upstream.as_deref(), Some("origin/main"));
                assert_eq!(upstream_behind, Some(3));
            }
            other => panic!("expected an available drift, got {other:?}"),
        }
    }

    #[test]
    fn source_drift_is_zero_for_a_run_that_just_started() {
        let run_id = "fresh01";
        let (_origin, work, run_state) = repo_with_run(run_id, "main");

        match source_drift(work.path(), run_id, &run_state) {
            SourceDrift::Available { ahead, behind, .. } => {
                assert_eq!((ahead, behind), (0, 0));
            }
            other => panic!("expected an available drift, got {other:?}"),
        }
    }

    #[test]
    fn source_drift_is_unavailable_once_the_run_branch_is_gone() {
        let run_id = "cleaned";
        let (_origin, work, run_state) = repo_with_run(run_id, "main");
        run_git(
            work.path(),
            &["branch", "-D", &run_refs::tip_branch(run_id)],
        );

        match source_drift(work.path(), run_id, &run_state) {
            // An archived Run is the expected end of a Run's life: named, not an
            // error, and never a fabricated zero.
            SourceDrift::Unavailable { reason } => {
                assert!(reason.contains("branch"), "reason was: {reason}");
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    #[test]
    fn source_drift_of_a_remote_source_has_no_local_side() {
        let run_id = "remotesrc";
        let (_origin, work, mut run_state) = repo_with_run(run_id, "origin/feature-remote-only");
        // The fork of this Run is `main`'s tip; `origin/feature-remote-only` is one
        // commit past it (the clone's fixture), so the source side counts 1.
        run_state.fork_sha = Some(head_sha(work.path()));

        match source_drift(work.path(), run_id, &run_state) {
            SourceDrift::Available {
                behind,
                local_behind,
                upstream,
                ..
            } => {
                assert_eq!(behind, 1);
                assert!(
                    local_behind.is_none(),
                    "a tracking ref has no local branch to count"
                );
                assert!(upstream.is_none(), "and it IS the upstream");
            }
            other => panic!("expected an available drift, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn source_drift_endpoint_answers_200_even_when_it_cannot_measure() {
        // The contract the Run view leans on: `unavailable` is a 200 with a reason,
        // never a 4xx/5xx. An archived Run whose branch was cleaned up is the
        // expected end of a Run's life, and an error path there would make the
        // panel shout about something that is simply over.
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let run_id = "drift-run";
        let state = test_state_with_dir(daemon_dir.path()).await;
        seed_run_in_other_repo(&state, target_dir.path(), run_id).await;

        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/source-drift"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        // This fixture records no source branch, so the measurement cannot be made.
        assert_eq!(json["state"], "unavailable");
        assert!(
            json["reason"].as_str().is_some_and(|r| !r.is_empty()),
            "the reason is what the panel shows instead of numbers: {json}"
        );
    }

    #[tokio::test]
    async fn effective_repo_root_falls_back_only_for_legacy_null_target_runs() {
        let state = test_state().await;

        let mut run_state = event_log::RunState::new("test".into(), "pipe".into());

        // #470/AC7: the fallback is READ-side ONLY. A `target_repo: null` here can
        // only come from a `run_started` recorded before the create boundary was
        // hardened — `POST /runs` now answers 400 rather than defaulting (see
        // `create_run_without_target_repo_is_400`). Resolving those old records to
        // the daemon root is what keeps the archive, the cost axes and the liveness
        // sweeps readable; do not symmetrise it away.
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

    #[tokio::test]
    async fn file_at_ref_returns_content_defaults_to_tip_and_404s_missing() {
        // #748 AC3: the content endpoint the next ticket's context expansion uses.
        let daemon_dir = tempfile::tempdir().unwrap();
        init_test_repo(daemon_dir.path());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path();
        let run_id = "file-at-ref";
        let state = test_state_with_dir(daemon_dir.path()).await;
        let wt_dir = seed_run_in_other_repo(&state, target, run_id).await;
        std::fs::create_dir_all(wt_dir.join("src")).unwrap();
        std::fs::write(wt_dir.join("src/new file.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        git_in(&wt_dir, &["add", "-A"]);
        git_in(&wt_dir, &["commit", "-q", "-m", "add"]);

        let app = build_router(state);
        // Default ref = the Run tip.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/file?path=src%2Fnew%20file.rs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/plain"));
        assert_eq!(body_string(resp).await, "fn a() {}\nfn b() {}\n");

        // At the fork point the file does not exist yet → 404.
        let fork = git_in(target, &["rev-parse", "HEAD"]);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/runs/{run_id}/file?path=src%2Fnew%20file.rs&ref={fork}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // README exists at the fork point.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/file?path=README.md&ref={fork}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "# test\n");

        // Parent-escaping paths are refused.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/runs/{run_id}/file?path=..%2Fetc%2Fpasswd"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
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
    fn sanitize_attachment_filename_accepts_any_extension_and_strips_paths() {
        assert_eq!(
            sanitize_attachment_filename("SPEC-779.md"),
            Ok("SPEC-779.md".into())
        );
        assert_eq!(
            sanitize_attachment_filename("../../etc/fixtures.json"),
            Ok("fixtures.json".into())
        );
        assert_eq!(
            sanitize_attachment_filename("/tmp/dump.sql"),
            Ok("dump.sql".into())
        );
        assert_eq!(sanitize_attachment_filename("noext"), Ok("noext".into()));
    }

    #[test]
    fn sanitize_attachment_filename_refuses_empty_dots_and_the_prompt_name() {
        assert!(sanitize_attachment_filename("").is_err());
        assert!(sanitize_attachment_filename("..").is_err());
        assert!(sanitize_attachment_filename("/").is_err());
        let err = sanitize_attachment_filename("output.md").unwrap_err();
        assert!(err.contains("reserved"), "{err}");
    }

    #[test]
    fn resolve_max_attachments_mb_stored_beats_default() {
        assert_eq!(resolve_max_attachments_mb(Some(120)), 120);
        // A stored value below 1 cannot be persisted, but the resolver still
        // treats it as unset rather than a 0-byte budget.
        assert!(resolve_max_attachments_mb(Some(0)) >= 1);
    }

    #[test]
    fn image_files_stored_in_input_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let input_dir = tmp.path().join("_input");
        std::fs::create_dir_all(&input_dir).unwrap();
        std::fs::write(input_dir.join("output.md"), "hello").unwrap();

        let images = vec![
            InputFile {
                filename: "screenshot.png".into(),
                data: vec![0x89, 0x50, 0x4E, 0x47],
                is_image: true,
            },
            InputFile {
                filename: "diagram.jpg".into(),
                data: vec![0xFF, 0xD8, 0xFF],
                is_image: true,
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
            ts: pdo_kernel::clock::now_iso(),
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
            ts: pdo_kernel::clock::now_iso(),
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
    #[allow(clippy::await_holding_lock)]
    async fn retry_all_of_a_legacy_null_target_run_lands_on_the_resolved_repo() {
        // `retry_all` archives the original BEFORE creating the replacement.
        // A run recorded before the create boundary was hardened carries
        // `target_repo: null`; forwarding that raw would 400 at the now-mandatory
        // chokepoint — after the archive — i.e. archived with no replacement, work
        // unreachable. So the retry reads the RESOLVED repo and pins it explicitly.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        init_test_repo(tmp.path());
        write_test_pipeline(tmp.path(), "retry-pipe");
        let state = test_state_with_dir(tmp.path()).await;

        for (kind, payload) in [
            (
                event_log::EventKind::RunStarted,
                Some(serde_json::json!({
                    "pipeline_name": "retry-pipe",
                    "input": "legacy work",
                    "edges": [],
                    "node_defs": [],
                    // NOTE: no `target_repo` — the whole point.
                })),
            ),
            (event_log::EventKind::RunFailed, None),
        ] {
            append_event(
                &state,
                &event_log::Event {
                    id: None,
                    run_id: "legacy-null-target".into(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind,
                    node_id: None,
                    iter: None,
                    payload,
                },
            )
            .await
            .unwrap();
        }

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs/legacy-null-target/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "retry_all" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            status,
            StatusCode::CREATED,
            "the retry must not 400 after archiving; body: {json}"
        );

        let new_run_id = json["run_id"].as_str().unwrap();
        let new_state = event_log::project(&load_events(&state.db, new_run_id).await.unwrap())
            .expect("the replacement run must exist");
        assert_eq!(
            new_state.target_repo.as_deref(),
            state.repo_root.to_str(),
            "the retry lands where the original actually ran, now recorded explicitly"
        );
    }

    #[tokio::test]
    async fn retry_all_rejected_on_running_state() {
        let state = test_state().await;

        let run_event = event_log::Event {
            id: None,
            run_id: "retry-running".into(),
            ts: pdo_kernel::clock::now_iso(),
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

    #[test]
    fn placeholder_name_is_deterministic_from_run_id_timestamp() {
        assert_eq!(
            placeholder_run_name("20260629-150013-4a2f1c0"),
            "Untitled run 20260629-150013"
        );
    }

    #[test]
    fn run_name_hint_matrix() {
        use prompt_augmenter::RunNameHint;
        // The hint is now gated on the RESOLVED `auto_name`, not on the name.
        // With auto-naming OFF the manager is never told to rename — `UserProvided`
        // regardless of name or input (the display name is chosen by the caller).
        assert_eq!(
            run_name_hint(false, Some("My Run"), ""),
            RunNameHint::UserProvided
        );
        assert_eq!(
            run_name_hint(false, Some("My Run"), "some input"),
            RunNameHint::UserProvided
        );
        assert_eq!(run_name_hint(false, None, ""), RunNameHint::UserProvided);
        assert_eq!(
            run_name_hint(false, None, "do a thing"),
            RunNameHint::UserProvided
        );

        // With auto-naming ON the pre-#338 behaviour holds, gated on input only.
        // No (meaningful) input → deterministic placeholder, renamed best-effort later.
        assert_eq!(run_name_hint(true, None, ""), RunNameHint::Placeholder);
        assert_eq!(
            run_name_hint(true, None, "   \n\t "),
            RunNameHint::Placeholder
        );
        assert_eq!(run_name_hint(true, Some(""), ""), RunNameHint::Placeholder);
        // Real input → manager derives the name from `_input` immediately.
        assert_eq!(
            run_name_hint(true, None, "do a thing"),
            RunNameHint::DeriveFromInput
        );
        assert_eq!(
            run_name_hint(true, Some("ignored when auto"), "do a thing"),
            RunNameHint::DeriveFromInput
        );
    }

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
            "name: dangling-pipe\nversion: \"1.0\"\nnodes:\n{START_END_YAML}  - id: worker\n    name: worker\n    type: agent\n    isolated_worktree: false\n    outputs:\n      - name: result\nedges:\n  - source: {{ node: worker, port: resullt }}\n    target: {{ node: end, port: result }}\n"
        );
        std::fs::write(pipelines_dir.join("dangling-pipe.yaml"), yaml).unwrap();
        // The target repo is now checked FIRST in `create_run_inner`, so this
        // test must name a valid one — otherwise it would 400 for the wrong reason
        // and silently stop testing #211.
        init_test_repo(tmp.path());

        let state = test_state_with_dir(tmp.path()).await;

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"pipeline": "dangling-pipe", "input": "do the thing", "target_repo": "{}"}}"#,
                        tmp.path().display()
                    )))
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

    /// Assert that a `POST /runs` body was refused *for the target-repo reason*, and
    /// that the refusal happened before ANY effect: no Run, and — the point of the
    /// 2026-07-29 incident — no `.pdo/runs/` tree under the daemon's own root.
    async fn assert_create_run_refused_for_target_repo(state: &Arc<AppState>, body: &str) {
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a Run without a named target repo must be refused; body was: {body}"
        );
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let error = json["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("target_repo"),
            "the error must name the field; got: {error}"
        );
        assert!(
            error.contains("absolute path"),
            "the error must be actionable — say WHAT to pass; got: {error}"
        );
        assert_no_run_and_no_worktree(state).await;
    }

    /// The AC1 pair: zero Runs listed, and zero worktree scaffolding under the
    /// daemon's storage root.
    async fn assert_no_run_and_no_worktree(state: &Arc<AppState>) {
        let app = build_router(state.clone());
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
        // The `.pdo/runs` dir may pre-exist (the pipeline watcher creates it to
        // watch run-scoped edits), so assert on its CONTENTS: nothing was staged.
        let runs_dir = state.repo_root.join(".pdo").join("runs");
        let entries: Vec<_> = std::fs::read_dir(&runs_dir)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(
            entries.is_empty(),
            "no worktree scaffolding may appear under the daemon's own root: {entries:?}"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_without_target_repo_is_400() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        // Everything else about this body is valid — only the target repo is
        // missing. Before #470 this created a Run in the daemon's own repo.
        assert_create_run_refused_for_target_repo(
            &state,
            r#"{"pipeline": "some-pipe", "input": "do the thing"}"#,
        )
        .await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_with_unknown_field_is_400_naming_it() {
        // An unknown top-level field is a named `400`, never silently dropped
        // (the symptom the issue cites is a `target_repos` typo thrown away, launching
        // the run mono-repo). The check runs before any effect: no Run is created.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let repo = tmp.path().to_string_lossy().into_owned();
        let state = test_state_with_dir(tmp.path()).await;

        let app = build_router(state.clone());
        let body = format!(
            r#"{{"pipeline":"some-pipe","input":"x","target_repo":"{repo}","target_repoz":[]}}"#
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let error = json["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("target_repoz"),
            "the error must name the unknown field; got: {error}"
        );
        assert_no_run_and_no_worktree(&state).await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_with_unknown_field_multipart_is_400() {
        // The symmetric fix on the multipart (browser) path: an unrecognised form
        // field is named, not silently ignored by the old `_ => {}` arm.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        const BOUNDARY: &str = "----pdoTestBoundary601";
        let mut body = String::new();
        for (name, value) in [("pipeline", "some-pipe"), ("input", "x"), ("bogus", "v")] {
            body.push_str(&format!("--{BOUNDARY}\r\n"));
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str(&format!("--{BOUNDARY}--\r\n"));

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={BOUNDARY}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            json["error"].as_str().unwrap_or_default().contains("bogus"),
            "the multipart error must name the unknown field; got: {json}"
        );
        assert_no_run_and_no_worktree(&state).await;
    }

    #[test]
    fn create_run_known_fields_cover_the_struct() {
        // Guards the allow-list against drift: a payload carrying EXACTLY the
        // accepted keys must pass the unknown-field gate AND deserialize into the
        // typed request. A field added to `CreateRunRequest` but not to
        // `CREATE_RUN_FIELDS` would be rejected by the gate at runtime; one added
        // to the list but not the struct would be a dead entry. Values are
        // type-appropriate (`#[serde(default)]` only fills an ABSENT key, so a
        // present `null` on a `Vec`/`HashMap` field would fail — hence `{}`/`[]`).
        let value = serde_json::json!({
            "pipeline": "p",
            "input": "i",
            "variables": {},
            "pipeline_id": null,
            "target_repo": null,
            "target_repos": [],
            "source_branch": null,
            "name": null,
            "triggered_by": null,
            "sandbox": null,
            "harness": null,
            "agent_choice": null,
            "skills": [],
            "auto_name": null,
            "auto_fail": null,
            "provisioning": {},
        });
        // The payload's keys are exactly the allow-list (pins the lock-step).
        let keys: std::collections::BTreeSet<String> =
            value.as_object().unwrap().keys().cloned().collect();
        let allowed: std::collections::BTreeSet<String> =
            CREATE_RUN_FIELDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            keys, allowed,
            "the test payload must list exactly CREATE_RUN_FIELDS"
        );

        assert!(
            reject_unknown_create_run_fields(&value).is_ok(),
            "every listed field must be accepted by the gate"
        );
        let parsed: Result<CreateRunRequest, _> = serde_json::from_value(value);
        assert!(
            parsed.is_ok(),
            "the accepted field set must deserialize into CreateRunRequest: {:?}",
            parsed.err()
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_with_blank_target_repo_is_400() {
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        for blank in ["", "   "] {
            assert_create_run_refused_for_target_repo(
                &state,
                &format!(
                    r#"{{"pipeline": "some-pipe", "input": "do the thing", "target_repo": "{blank}"}}"#
                ),
            )
            .await;
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_with_blank_target_repo_multipart_is_400() {
        // Distinct case: the multipart path NORMALISES an empty field to `None`
        // (`if !v.is_empty()`), so a blank there is not the same input shape the
        // JSON path sees. Both must land on the same refusal.
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        const BOUNDARY: &str = "----pdoTestBoundary470";
        let mut body = String::new();
        for (name, value) in [
            ("pipeline", "some-pipe"),
            ("input", "do the thing"),
            ("target_repo", "   "),
        ] {
            body.push_str(&format!("--{BOUNDARY}\r\n"));
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str(&format!("--{BOUNDARY}--\r\n"));

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={BOUNDARY}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a blank multipart target_repo must be refused"
        );
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let error = json["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("target_repo"),
            "the error must name the field; got: {error}"
        );
        assert_no_run_and_no_worktree(&state).await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn create_run_with_non_git_target_repo_is_400() {
        // Non-regression on `validate_target_repo`: naming a directory is not
        // enough, it must be a git repository (AC3).
        let _home = FakeHome::new();
        let tmp = tempfile::tempdir().unwrap();
        write_test_pipeline(tmp.path(), "some-pipe");
        init_test_repo(tmp.path());
        let state = test_state_with_dir(tmp.path()).await;

        let not_a_repo = tempfile::tempdir().unwrap();
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"pipeline": "some-pipe", "input": "x", "target_repo": "{}"}}"#,
                        not_a_repo.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let error = json["error"].as_str().unwrap_or_default();
        assert!(error.contains("not a git repository"), "got: {error}");
        assert_no_run_and_no_worktree(&state).await;
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

    #[test]
    fn no_pin_leaves_the_follow_the_run_combo_untouched() {
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: Some("gpt-5".into()),
            effort: Some("high".into()),
        };
        let (combo, warning) = apply_manager_profile_pin(None, Ok(None), follow_run.clone());
        assert_eq!(combo, follow_run);
        assert_eq!(warning, None);
    }

    #[test]
    fn a_resolved_pin_fully_replaces_the_follow_the_run_combo() {
        // Strong pin (design §2.2, decision #3): harness, model AND effort come
        // from the profile — the Run tier never leaks through.
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: Some("gpt-5".into()),
            effort: Some("high".into()),
        };
        let profile = agent_profile::AgentProfile {
            id: "p1".into(),
            name: "CheapManager".into(),
            harness: "claude".into(),
            model: Some("haiku".into()),
            effort: None,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("CheapManager"), Ok(Some(profile)), follow_run);
        assert_eq!(combo.harness, "claude");
        assert_eq!(combo.model.as_deref(), Some("haiku"));
        assert_eq!(combo.effort, None);
        assert_eq!(warning, None);
    }

    #[test]
    fn a_pin_to_a_missing_profile_falls_back_with_a_warning_naming_it() {
        // #432 tombstone discipline: the profile was deleted after the pin was
        // stored — the spawn falls back to « Follow the Run », loudly, and the
        // stored value is never rewritten (nothing here persists anything).
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "codex".into(),
            model: None,
            effort: None,
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("ghost"), Ok(None), follow_run.clone());
        assert_eq!(combo, follow_run);
        let warning = warning.expect("a missing pin must warn");
        assert!(warning.contains("`ghost`"), "got: {warning}");
        assert!(warning.contains("Follow the Run"), "got: {warning}");
    }

    #[test]
    fn a_pin_that_cannot_be_looked_up_falls_back_with_a_warning() {
        let follow_run = pdo_harness::agent_choice::ResolvedCombo {
            harness: "claude".into(),
            model: None,
            effort: None,
        };
        let (combo, warning) =
            apply_manager_profile_pin(Some("p"), Err("db locked".into()), follow_run.clone());
        assert_eq!(combo, follow_run);
        let warning = warning.expect("a failed lookup must warn");
        assert!(warning.contains("db locked"), "got: {warning}");
        assert!(warning.contains("Follow the Run"), "got: {warning}");
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

    // ADR-0035 — a completion refusal is never a 2xx. The structural invariant
    // lives in `completion_refusal.rs`; what follows pins the WIRE on BOTH
    // surfaces, because the original bug survived precisely because `POST …/done`
    // and `POST /runs/:id/commands` were only ever tested one at a time.

    /// `command_triplet`'s sibling for `POST …/nodes/:id/done`. Status **and**
    /// content-type, so the two surfaces are pinned by the same shape of assertion
    /// (net of #236).
    async fn done_triplet(
        state: &Arc<AppState>,
        run_id: &str,
        node_id: &str,
        body: &str,
    ) -> (StatusCode, String, String) {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/runs/{run_id}/nodes/{node_id}/done"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = String::from_utf8_lossy(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .into_owned();
        (status, content_type, body)
    }

    /// ADR-0024's tombstone keeps its `410` on this route: "never a 2xx" is not
    /// "always a 409", and flattening these would be the wrong reading of #490.
    #[tokio::test]
    async fn a_forgotten_run_still_answers_410_on_the_done_route() {
        let state = test_state().await;
        let run_id = "done-forgotten";
        seed_completed_run(&state, run_id).await;
        post_cleanup_run(&state, run_id).await;
        let resp = build_router(state.clone())
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

        let (status, ct, raw) = done_triplet(&state, run_id, "worker", "{}").await;
        assert_eq!((status, ct.as_str()), (StatusCode::GONE, JSON_CT));
        let body: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(body["error"], "run_forgotten");
        assert_eq!(body["recoverable"], false);
    }

    #[test]
    fn reap_orphan_run_worktree_removes_empty_run_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_test_repo(repo);

        let worktree = worktree_dir_for_run(repo, "failed-run");
        reap_orphan_run_worktree(repo, &worktree, "pdo/run-failed-run");

        assert!(
            !worktree.parent().unwrap().exists(),
            "an aborted Run must not leave its empty directory behind"
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
}
