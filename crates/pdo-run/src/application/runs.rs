//! The run context, assembled: its ports, its knobs, and the state its
//! lifecycle shares across requests (the locks, the sweep's gauges, the test
//! seams).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64};
use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::event_bus::EventBus;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_sandbox::application::ports::SandboxProfileRepository;
use pdo_session::application::ports::Tmux;

use super::append_event::AppendEvent;
use pdo_pipeline::adapters::self_writes::SelfWrites;
use pdo_pipeline::application::ports::SkillBank;

use super::ports::{
    DeclaredWaits, EventStore, InstanceSettings, LiveFacts, ManagerSessions, NodeSkills,
    PipelineTriggers, Projects, RunDirWatch, RunPages, SourcePipelines,
};
use super::read_run::ReadRun;

/// Every port the context needs, as the composition root plugs them in.
pub struct RunsPorts {
    pub store: Arc<dyn EventStore>,
    /// The kernel's bus: every appended event is published there, as `/ws`
    /// relays it.
    pub bus: Arc<dyn EventBus>,
    /// What a Run's detail reads from disk (`adapters::disk_live_facts`).
    pub live_facts: Arc<dyn LiveFacts>,
    pub managers: Arc<dyn ManagerSessions>,
    /// tmux on this daemon's private socket (the session context's port).
    pub tmux: Arc<dyn Tmux>,
    /// The declared wait (the session context).
    pub waits: Arc<dyn DeclaredWaits>,
    /// The Projets a Run's repositories belong to (the workspace context).
    pub projects: Arc<dyn Projects>,
    /// The bank and the delivery of a node's skills (the skill context).
    pub skills: Arc<dyn NodeSkills>,
    /// The staging profiles a sandboxed Run freezes.
    pub sandbox_profiles: Arc<dyn SandboxProfileRepository>,
    /// The instance settings a spawn reads (implemented by the legacy side).
    pub settings: Arc<dyn InstanceSettings>,
    /// The skill bank as the pipeline context reads it: the skills sidecar of
    /// a Run's pipeline.
    pub bank: Arc<dyn SkillBank>,
    /// The page mounts a Run published (the platform context), released at
    /// archive.
    pub pages: Arc<dyn RunPages>,
    /// The pipeline watcher a new Run's directory is registered with.
    pub run_dirs: Arc<dyn RunDirWatch>,
    /// The daemon's own writes, which the pipeline watcher does not announce:
    /// an edit of the run-scoped pipeline marks what it writes.
    pub self_writes: SelfWrites,
    /// The shared pipelines a Run's snapshot may overwrite (the pipeline
    /// context's registry, ADR-0085).
    pub source_pipelines: Arc<dyn SourcePipelines>,
    /// The Triggers that fire a pipeline (the trigger context).
    pub pipeline_triggers: Arc<dyn PipelineTriggers>,
    /// Asked before every gesture of an actor, never before a transition.
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The daemon's knobs the context reads.
#[derive(Debug, Clone, Default)]
pub struct RunsOptions {
    pub daemon_port: u16,
    /// Extra WebSocket origins (`PDO_ALLOWED_WS_ORIGINS`), parsed at boot.
    pub allowed_ws_origins: Vec<String>,
    /// The daemon's repo root (its cwd): the default repository of a Run
    /// that names none.
    pub repo_root: PathBuf,
    /// Replaces the harness tail of a node session (per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
    /// Replaces the `docker` binary of the sandbox wiring (per-daemon test seam).
    pub docker_cmd_override: Option<String>,
    /// Replaces `$HOME` for the sandbox staging and the harness descriptors.
    pub sandbox_home_override: Option<PathBuf>,
    /// Debug-only: the first stale sweep panics (see [`Runs::arm_stale_panic`]).
    pub panic_on_stale_sweep: bool,
    /// Debug-only: the first spawn panics in its post-worktree span.
    pub panic_on_spawn: bool,
}

/// The context, assembled: its use cases over its ports.
pub struct Runs {
    pub(crate) store: Arc<dyn EventStore>,
    pub(crate) bus: Arc<dyn EventBus>,
    pub(crate) options: RunsOptions,
    append_event: AppendEvent,
    read_run: ReadRun,
    pub(crate) tmux: Arc<dyn Tmux>,
    pub(crate) waits: Arc<dyn DeclaredWaits>,
    pub(crate) projects: Arc<dyn Projects>,
    pub(crate) skills: Arc<dyn NodeSkills>,
    pub(crate) sandbox_profiles: Arc<dyn SandboxProfileRepository>,
    pub(crate) settings: Arc<dyn InstanceSettings>,
    pub(crate) bank: Arc<dyn SkillBank>,
    pub(crate) live_facts: Arc<dyn LiveFacts>,
    pub(crate) pages: Arc<dyn RunPages>,
    pub(crate) run_dirs: Arc<dyn RunDirWatch>,
    pub(crate) self_writes: SelfWrites,
    pub(crate) source_pipelines: Arc<dyn SourcePipelines>,
    pub(crate) pipeline_triggers: Arc<dyn PipelineTriggers>,
    extensions: Arc<dyn ExtensionHost>,
    pub(crate) repo_root: PathBuf,
    pub(crate) port: u16,
    pub(crate) tmux_cmd_override: Option<String>,
    pub(crate) docker_cmd_override: Option<String>,
    pub(crate) sandbox_home_override: Option<PathBuf>,
    /// Serializes the merge-back of a completion into the Run's branch.
    pub(crate) merge_lock: tokio::sync::Mutex<()>,
    /// Serializes admission so the slot check is atomic check-and-reserve. A
    /// spawn holds this from the moment it counts live sessions until it has
    /// appended the reservation event (`NodeStarted` / `NodeWaiting`). Without
    /// it, concurrent spawns all observe the same free slot and overshoot the cap.
    pub(crate) admission_lock: tokio::sync::Mutex<()>,
    /// Epoch-millis of the **start** of the most recent stale-detection sweep, or
    /// `0` if it has never swept. Written at sweep start so a wedged sweep
    /// freezes the value; an isolated panic still advances it. Surfaced by
    /// `GET /stale/health`.
    pub(crate) last_stale_tick_ms: Arc<AtomicI64>,
    /// Count of live nodes blocked on Claude Code's usage-limit menu as of the
    /// most recent sweep. A *level*: recomputed each sweep, so a node leaving the
    /// live set is simply not recounted. Observability-only — the sweep never
    /// changes a flagged node's status.
    pub(crate) blocked_on_limit: Arc<AtomicI64>,
    /// Debug-only one-shot poison pill: the next stale sweep `panic!`s, then
    /// disarms itself, proving the *next* sweep recovers.
    pub(crate) panic_on_stale_sweep: Arc<AtomicBool>,
    /// Debug-only one-shot poison pill: the next `spawn_node` panics inside its
    /// post-worktree span, then disarms itself, exercising the spawn window's
    /// `catch_unwind` (orphaned sub-worktree reaped, run failed loud instead of
    /// wedging `Running`).
    pub(crate) panic_on_spawn: Arc<AtomicBool>,
    /// Test-only gate: when armed, the detached terminal tail (reap + run-advance
    /// spawned by `done`/`fail`/`skip`) parks until [`Runs::release_node_done_gate`]
    /// fires. Don't move it off the FIRST instruction of the detached future: its
    /// position is what encodes the detach-vs-inline divergence the regression
    /// test discriminates on (inline, a client disconnect cancels the wait and
    /// the advance is lost).
    pub(crate) node_done_tail_gate: Arc<tokio::sync::Notify>,
    /// Whether the tail gate is currently armed.
    pub(crate) node_done_tail_gate_armed: Arc<AtomicBool>,
    /// Who has which Run open (API 0.12), by connection of the event stream.
    pub(crate) presences: std::sync::Mutex<crate::domain::presence::PresenceBook>,
    /// The id of the next connection of the event stream.
    pub(crate) next_connection: AtomicU64,
}

impl Runs {
    pub fn new(ports: RunsPorts, options: RunsOptions) -> Arc<Self> {
        let RunsPorts {
            store,
            bus,
            live_facts,
            managers,
            tmux,
            waits,
            projects,
            skills,
            sandbox_profiles,
            settings,
            bank,
            pages,
            run_dirs,
            self_writes,
            source_pipelines,
            pipeline_triggers,
            extensions,
        } = ports;
        Arc::new(Self {
            append_event: AppendEvent::new(store.clone(), bus.clone()),
            read_run: ReadRun::new(store.clone(), live_facts.clone(), managers),
            live_facts,
            store,
            bus,
            tmux,
            waits,
            projects,
            skills,
            sandbox_profiles,
            settings,
            bank,
            pages,
            run_dirs,
            self_writes,
            source_pipelines,
            pipeline_triggers,
            extensions,
            repo_root: options.repo_root.clone(),
            port: options.daemon_port,
            tmux_cmd_override: options.tmux_cmd_override.clone(),
            docker_cmd_override: options.docker_cmd_override.clone(),
            sandbox_home_override: options.sandbox_home_override.clone(),
            merge_lock: tokio::sync::Mutex::new(()),
            admission_lock: tokio::sync::Mutex::new(()),
            last_stale_tick_ms: Arc::new(AtomicI64::new(0)),
            blocked_on_limit: Arc::new(AtomicI64::new(0)),
            panic_on_stale_sweep: Arc::new(AtomicBool::new(options.panic_on_stale_sweep)),
            panic_on_spawn: Arc::new(AtomicBool::new(options.panic_on_spawn)),
            node_done_tail_gate: Arc::new(tokio::sync::Notify::new()),
            node_done_tail_gate_armed: Arc::new(AtomicBool::new(false)),
            presences: std::sync::Mutex::new(Default::default()),
            next_connection: AtomicU64::new(1),
            options,
        })
    }

    /// The event log.
    pub fn store(&self) -> &Arc<dyn EventStore> {
        &self.store
    }

    /// The bus `/ws` relays.
    pub fn bus(&self) -> &Arc<dyn EventBus> {
        &self.bus
    }

    pub fn options(&self) -> &RunsOptions {
        &self.options
    }

    /// The one way an event enters a Run's log.
    pub fn append_event(&self) -> &AppendEvent {
        &self.append_event
    }

    pub fn read_run(&self) -> &ReadRun {
        &self.read_run
    }

    /// tmux, on this daemon's private socket (`-L pdo-<port>`): the session
    /// context's port. Every tmux call of a Run goes through it, or parallel
    /// daemons collide.
    pub fn tmux(&self) -> &dyn Tmux {
        self.tmux.as_ref()
    }

    /// Serializes admission (see the field): the create path holds it too.
    pub fn admission_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.admission_lock
    }

    /// Epoch-millis of the start of the last stale sweep, `0` before the first.
    pub fn last_stale_tick_ms(&self) -> i64 {
        self.last_stale_tick_ms
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Live nodes blocked on a usage-limit menu at the last sweep.
    pub fn blocked_on_limit(&self) -> i64 {
        self.blocked_on_limit
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Debug seam: the next stale sweep panics, once.
    pub fn arm_stale_panic(&self) {
        self.panic_on_stale_sweep
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Debug seam: the next spawn panics in its post-worktree span, once.
    pub fn arm_spawn_panic(&self) {
        self.panic_on_spawn
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Test seam: park every detached terminal tail until
    /// [`Self::release_node_done_gate`].
    pub fn arm_node_done_gate(&self) {
        self.node_done_tail_gate_armed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Test seam: let the parked terminal tails run.
    pub fn release_node_done_gate(&self) {
        self.node_done_tail_gate_armed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.node_done_tail_gate.notify_waiters();
    }

    /// The extension host: the attributes of the Runs.
    pub fn extensions(&self) -> &dyn ExtensionHost {
        self.extensions.as_ref()
    }

    /// Ask the extensions whether `actor` may run `operation`: every gesture
    /// of an actor does, never a transition of the scheduler (D10). An
    /// Operation on a Run or a node is asked in the Projets of its Run
    /// ([`Runs::scoped`]).
    pub async fn authorize(&self, actor: &Actor, operation: &Operation) -> Result<(), Denied> {
        let operation = self.scoped(operation.clone()).await;
        authorize(self.extensions.as_ref(), actor, &operation)
    }
}
