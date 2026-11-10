//! What the start primitive takes and hands back, and the reads it makes of
//! the Run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdo_session::application::ports::Tmux;
use pdo_session::domain::launch::SessionTail;

use crate::application::sandbox::SandboxExec;
use crate::domain::state::NodeStatus;
use crate::domain::{blackboard, event, state};
use pdo_harness::registry;
use pdo_pipeline::domain::pipeline::{self, PipelineDef};

use super::PrimitiveOutcome;

pub struct StartNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub overrides: Option<HashMap<String, PathBuf>>,
    pub pipeline: &'a PipelineDef,
    pub run_state: &'a state::RunState,
    pub artifacts_dir: &'a Path,
    pub worktree_dir: &'a Path,
    pub repo_root: &'a Path,
    pub pipeline_path: &'a Path,
    pub resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    pub daemon_port: u16,
    /// `None` → real claude.
    pub tmux_cmd_override: Option<&'a str>,
    /// `None` → real `docker`. Used only when `run_state.sandbox != off`.
    pub docker_cmd_override: Option<&'a str>,
    /// Instance-wide default model, pre-resolved by the caller: `start_node` is
    /// sync and DB-less, so it cannot read it itself. The node's own `model:`
    /// still wins over it.
    pub default_model: Option<String>,
    /// Instance default **harness** (ADR-0046). Same DB-less contract as
    /// [`Self::default_model`]; the node's `pin_harness` still wins over it.
    pub default_harness: Option<String>,
    /// Fallback tier of the model resolution for the winning harness.
    pub default_harness_models: std::collections::BTreeMap<String, String>,
    /// The harness carried by the **Projet** this Run was launched in (ADR-0046,
    /// ADR-0087), pre-resolved like [`Self::default_harness`]. Feeds the `project`
    /// tier, between the Run and the instance default; `None` makes the tier
    /// transparent. No repository of the Run sways it.
    pub project_harness: Option<String>,
    /// Turn-end auto-completion (ADR-0043), pre-resolved like
    /// [`Self::default_model`]. `start_node` ANDs `!is_script` so a `script` node
    /// never arms the `Stop` hook. Must be wired here as well as in `spawn_node`,
    /// or a manual force-spawn silently ignores the instance default.
    pub inject_hook: bool,
    /// The harness registry to resolve the winning harness name against. `Some`
    /// honours the **disk tier**, so a force-spawn reaches a user-declared harness
    /// exactly like the scheduler does; `None` is the embedded floor only.
    pub harness_registry: Option<&'a registry::HarnessRegistry>,
    /// Host `$HOME` for a sandboxed Run (#708): the root under which the resolved
    /// harness's staging set is filled at this spawn, mirroring `spawn_node`. `None`
    /// on the host path and when the daemon cannot resolve `$HOME` — the set is then
    /// not filled here (a scheduler spawn or the manager already did, idempotently).
    pub sandbox_home_root: Option<&'a Path>,
    /// What the extensions add to this launch (API 0.8), pre-resolved by the
    /// caller like [`Self::default_model`]: `start_node` is sync.
    pub extension: pdo_kernel::extension_host::NodeContribution,
}

/// Everything needed to launch the node's tmux session, once its reservation has
/// been appended (#485, ADR-0038).
///
/// **Why this type exists at all.** A live tmux session with no reservation in
/// the event log is exactly the state the orphan sweep kills on sight. Making the
/// spawn an *intention* the caller executes only after the append puts that order
/// in the type, so the wrong order is not expressible. Don't collapse it back
/// into a direct spawn.
///
/// Owns its data because [`SessionTail`] and the sandbox wrap borrow; both are
/// rebuilt in [`StartNodeSpawn::execute`].
pub struct StartNodeSpawn {
    pub session_name: String,
    pub(super) prompt: String,
    pub(super) working_dir: PathBuf,
    pub(super) run_id: String,
    pub(super) node_id: String,
    pub(super) iter: i64,
    pub(super) daemon_port: u16,
    pub(super) tmux_cmd_override: Option<String>,
    pub(super) tail: StartNodeTail,
    /// The `docker exec` into the Run's container, when sandboxed.
    pub(super) sandbox: Option<SandboxExec>,
    /// Arm the turn-end `Stop` hook (already ANDed with `!is_script`).
    pub(super) inject_hook: bool,
    /// The extensions' variables (API 0.8).
    pub(super) extension_env: Vec<(String, String)>,
}

/// Owned mirror of [`SessionTail`] — the borrowing enum
/// cannot be carried across the append.
pub(super) enum StartNodeTail {
    Agent {
        /// The resolved harness descriptor. Boxed so the wide descriptor does not
        /// bloat every `StartNodeTail` (`clippy::large_enum_variant`).
        harness: Box<registry::HarnessDescriptor>,
        model: Option<String>,
        effort: Option<String>,
        /// The pinned Claude Code session id. `None` for a script node.
        session_id: Option<String>,
    },
    Script {
        timeout_secs: u64,
        env: Vec<(String, String)>,
    },
}

impl StartNodeSpawn {
    /// Launch the session. **Call this only after the `NodeStarted` event has been
    /// appended** (ADR-0038) — see the type's doc-comment.
    ///
    /// The deliberate trade: if the append succeeds and this fails, the run has a
    /// `NodeStarted` with no session — a *visible* failure (ADR-0032 makes session
    /// death loud) rather than the silent orphan of the other order.
    pub fn execute(&self, tmux: &dyn Tmux) -> anyhow::Result<()> {
        let tail = match &self.tail {
            StartNodeTail::Agent {
                harness,
                model,
                effort,
                session_id,
            } => SessionTail::Agent {
                harness: harness.as_ref(),
                model: model.as_deref(),
                effort: effort.as_deref(),
                session_id: session_id.as_deref(),
            },
            StartNodeTail::Script { timeout_secs, env } => SessionTail::Script {
                timeout_secs: *timeout_secs,
                env,
            },
        };
        pdo_session::application::launch::spawn(
            tmux,
            &self.session_name,
            &self.prompt,
            &self.working_dir,
            &self.run_id,
            &self.node_id,
            self.iter,
            self.daemon_port,
            self.tmux_cmd_override.as_deref(),
            tail,
            self.sandbox.as_ref().map(SandboxExec::wrap).as_ref(),
            self.inject_hook,
            &self.extension_env,
        )
    }
}

pub struct StartNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event::Event>,
    /// The session to launch, or `None` when there is nothing to launch. Execute
    /// it **after** appending `events` (ADR-0038).
    pub spawn: Option<StartNodeSpawn>,
}

pub(super) fn has_node_started_event(
    run_state: &state::RunState,
    node_id: &str,
    iter: i64,
) -> bool {
    if let Some(node) = run_state.nodes.get(node_id) {
        if node.iter == iter && node.status != NodeStatus::Pending {
            return true;
        }
        if node
            .iterations
            .iter()
            .any(|it| it.iter == iter && it.status != NodeStatus::Pending)
        {
            return true;
        }
    }
    false
}

pub(super) fn resolve_inputs(
    params: &StartNodeParams<'_>,
    node: &pipeline::NodeDef,
) -> HashMap<String, String> {
    // Don't re-derive the iteration decision here: it lives in
    // `input_resolution::resolve_consumer_inputs` (latest-COMPLETED iter for a
    // single wire, COMPLETED iters for a `repeated` pool — never a raw `iter-*`
    // disk glob). This path only layers overrides and the entry-node `task`
    // fallback on top; a `repeated` pool flattens to a `\n`-joined string.
    let source_iters = crate::domain::input_resolution::resolved_source_iters(
        params.pipeline,
        params.run_state,
        params.node_id,
        params.iter,
    );
    let repeated_iters = crate::domain::input_resolution::resolved_repeated_iters(
        params.pipeline,
        params.run_state,
        params.node_id,
    );
    let resolved = crate::domain::input_resolution::resolve_consumer_inputs(
        params.pipeline,
        params.artifacts_dir,
        params.node_id,
        params.iter,
        &source_iters,
        &repeated_iters,
    );
    // First resolved input per target port — preserves the previous
    // first-matching-edge semantics when several edges share a target port.
    let mut by_port: HashMap<&str, &crate::domain::input_resolution::ResolvedInput> =
        HashMap::new();
    for r in &resolved {
        by_port.entry(r.port.as_str()).or_insert(r);
    }

    let mut input_paths = HashMap::new();

    // #486 / #600: overrides win over any edge resolution AND cover **emergent**
    // input ports. An `Agent`/`Script` node declares no `inputs`
    // (its inputs are derived from incoming edges), so keying overrides only off
    // `node.inputs` would silently drop an operator's dummy input on exactly those
    // nodes. Insert every override first — the declared-port loop below then fills
    // the rest without clobbering a port an override already set.
    if let Some(ov) = params.overrides.as_ref() {
        for (port, path) in ov {
            input_paths.insert(port.clone(), path.to_string_lossy().to_string());
        }
    }

    for input_port in &node.inputs {
        if input_paths.contains_key(&input_port.name) {
            continue;
        }

        if let Some(r) = by_port.get(input_port.name.as_str()) {
            let joined = r
                .paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            input_paths.insert(input_port.name.clone(), joined);
        } else if input_port.name == "task" {
            let path = blackboard::input_path(params.artifacts_dir);
            input_paths.insert(input_port.name.clone(), path.to_string_lossy().to_string());
        }
    }

    input_paths
}

pub(super) fn node_type_str(nt: &pipeline::NodeType) -> &'static str {
    nt.as_str()
}
