use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::event_log::{self, EventKind, NodeStatus};
use crate::pipeline::{self, PipelineDef};
use crate::worktree_ops::{ensure_sub_worktree, sub_worktree_branch, sub_worktree_path};
use crate::{blackboard, harness_registry, harness_resolver, tmux_session_manager};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrimitiveOutcome {
    Executed,
    AlreadyDone,
    Rejected { reason: String },
}

pub(crate) struct StartNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub overrides: Option<HashMap<String, PathBuf>>,
    pub pipeline: &'a PipelineDef,
    pub run_state: &'a event_log::RunState,
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
    /// The harness carried by the **Projet** of this Run's primary repo (ADR-0046),
    /// pre-resolved like [`Self::default_harness`]. Feeds the `project` tier,
    /// between the Run and the instance default; `None` makes the tier transparent.
    /// Resolve it from the **primary** repo only, so a secondary (ADR-0042) never
    /// sways it.
    pub project_harness: Option<String>,
    /// Turn-end auto-completion (ADR-0043), pre-resolved like
    /// [`Self::default_model`]. `start_node` ANDs `!is_script` so a `script` node
    /// never arms the `Stop` hook. Must be wired here as well as in `spawn_node`,
    /// or a manual force-spawn silently ignores the instance default.
    pub inject_hook: bool,
    /// The harness registry to resolve the winning harness name against. `Some`
    /// honours the **disk tier**, so a force-spawn reaches a user-declared harness
    /// exactly like the scheduler does; `None` is the embedded floor only.
    pub harness_registry: Option<&'a harness_registry::HarnessRegistry>,
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
/// Owns its data because [`tmux_session_manager::SessionTail`] and
/// [`tmux_session_manager::SandboxWrap`] borrow; both are rebuilt in
/// [`StartNodeSpawn::execute`].
pub(crate) struct StartNodeSpawn {
    session_name: String,
    prompt: String,
    working_dir: PathBuf,
    run_id: String,
    node_id: String,
    iter: i64,
    daemon_port: u16,
    tmux_cmd_override: Option<String>,
    tail: StartNodeTail,
    sandbox: Option<StartNodeSandbox>,
    /// Arm the turn-end `Stop` hook (already ANDed with `!is_script`).
    inject_hook: bool,
}

/// Owned mirror of [`tmux_session_manager::SessionTail`] — the borrowing enum
/// cannot be carried across the append.
enum StartNodeTail {
    Agent {
        /// The resolved harness descriptor. Boxed so the wide descriptor does not
        /// bloat every `StartNodeTail` (`clippy::large_enum_variant`).
        harness: Box<harness_registry::HarnessDescriptor>,
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

/// Owned mirror of [`tmux_session_manager::SandboxWrap`]. `marker` and `workdir`
/// are not stored: they are always the session name and the working dir, which
/// [`StartNodeSpawn`] already owns.
struct StartNodeSandbox {
    docker_bin: String,
    uid: u32,
    gid: u32,
}

impl StartNodeSpawn {
    /// Launch the session. **Call this only after the `NodeStarted` event has been
    /// appended** (ADR-0038) — see the type's doc-comment.
    ///
    /// The deliberate trade: if the append succeeds and this fails, the run has a
    /// `NodeStarted` with no session — a *visible* failure (ADR-0032 makes session
    /// death loud) rather than the silent orphan of the other order.
    pub(crate) fn execute(&self) -> anyhow::Result<()> {
        let tail = match &self.tail {
            StartNodeTail::Agent {
                harness,
                model,
                effort,
                session_id,
            } => tmux_session_manager::SessionTail::Agent {
                harness: harness.as_ref(),
                model: model.as_deref(),
                effort: effort.as_deref(),
                session_id: session_id.as_deref(),
            },
            StartNodeTail::Script { timeout_secs, env } => {
                tmux_session_manager::SessionTail::Script {
                    timeout_secs: *timeout_secs,
                    env,
                }
            }
        };
        let sandbox_wrap = self
            .sandbox
            .as_ref()
            .map(|sbx| tmux_session_manager::SandboxWrap {
                docker_bin: &sbx.docker_bin,
                uid: sbx.uid,
                gid: sbx.gid,
                marker: &self.session_name,
                workdir: &self.working_dir,
            });
        tmux_session_manager::spawn(
            &self.session_name,
            &self.prompt,
            &self.working_dir,
            &self.run_id,
            &self.node_id,
            self.iter,
            self.daemon_port,
            self.tmux_cmd_override.as_deref(),
            tail,
            sandbox_wrap.as_ref(),
            self.inject_hook,
        )
    }
}

pub(crate) struct StartNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
    /// The session to launch, or `None` when there is nothing to launch. Execute
    /// it **after** appending `events` (ADR-0038).
    pub spawn: Option<StartNodeSpawn>,
}

pub(crate) fn start_node(params: &StartNodeParams<'_>) -> StartNodeResult {
    let node = match params
        .pipeline
        .nodes
        .iter()
        .find(|n| n.id == params.node_id)
    {
        Some(n) => n,
        None => {
            return StartNodeResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("node '{}' not found in pipeline", params.node_id),
                },
                events: vec![],
                spawn: None,
            }
        }
    };

    if has_node_started_event(params.run_state, params.node_id, params.iter) {
        return StartNodeResult {
            outcome: PrimitiveOutcome::AlreadyDone,
            events: vec![],
            spawn: None,
        };
    }

    let input_paths = resolve_inputs(params, node);

    // #653 / ADR-0060: where this NodeRun works, read off the document. Unlike
    // `node_spawn`, this path never re-poses a live iteration — the
    // `has_node_started_event` guard above already answered `AlreadyDone` for any
    // iteration that has started — so there is no frozen value to prefer here.
    let has_sub_worktree = node.is_isolated();

    // ADR-0036: the commit the sub-worktree was cut from, recorded on
    // `NodeStarted` below. Without it a merge-back conflict on this iteration can
    // never be resolved in the node's favour. Record it on this path too, not just
    // in `node_spawn`.
    let mut spawn_base_sha: Option<String> = None;
    let working_dir = if has_sub_worktree {
        let sub_wt_dir =
            sub_worktree_path(params.repo_root, params.run_id, params.node_id, params.iter);
        let sub_branch = sub_worktree_branch(params.run_id, params.node_id, params.iter);
        let pipeline_branch = format!("pdo/run-{}", params.run_id);

        // No `previous_base_sha` to carry: `has_node_started_event` above already
        // returned `AlreadyDone` for any started iteration, so the `Reusable` arm
        // is unreachable here — this site only creates or recycles.
        match ensure_sub_worktree(
            params.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
            None,
        ) {
            Ok(ensured) => spawn_base_sha = ensured.base_sha,
            Err(e) => {
                return StartNodeResult {
                    outcome: PrimitiveOutcome::Rejected {
                        reason: format!("failed to ensure sub-worktree: {e:#}"),
                    },
                    events: vec![],
                    spawn: None,
                };
            }
        }
        sub_wt_dir
    } else {
        params.worktree_dir.to_path_buf()
    };

    let canonical_path = pipeline::canonical_prompt_path(params.pipeline_path, params.node_id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    // The manual start/retry endpoints produce the entry-node preamble, so they
    // must read this too — same gating and error posture as the live spawn site.
    let start_prompt_present = if params.pipeline.prompt_required {
        false
    } else {
        match crate::prompt_augmenter::read_start_prompt_present(params.artifacts_dir) {
            Ok(present) => present,
            Err(e) => {
                tracing::warn!(
                    "entry-node input read failed (run {} node {} iter {}): {e}; \
                     assuming a prompt is present",
                    params.run_id,
                    params.node_id,
                    params.iter
                );
                true
            }
        }
    };

    // A sandboxed node's session execs into the container, where `localhost` is
    // the container — hence the same resolver as the manager preamble. Defensive:
    // no node preamble prints `AugmentContext.daemon_url` yet.
    let sandboxed = !params.run_state.sandbox.is_off();

    let aug_ctx = crate::prompt_augmenter::AugmentContext {
        pipeline: params.pipeline,
        node,
        run_id: params.run_id,
        iter: params.iter,
        artifacts_dir: params.artifacts_dir,
        variables: params.resolved_vars,
        daemon_url: &crate::sandbox_container::daemon_url(params.daemon_port, sandboxed),
        foreach_context: None,
        source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
        // #654: the same section, from the other isolation. This site only
        // ever builds a preamble for a node that spawns a session, so the
        // two are exhaustive and mutually exclusive.
        shared_worktree_dir: (!has_sub_worktree).then_some(working_dir.as_path()),
        input_images: Vec::new(),
        start_prompt_present,
        source_iters: crate::input_resolution::resolved_source_iters(
            params.pipeline,
            params.run_state,
            params.node_id,
            params.iter,
        ),
        repeated_iters: crate::input_resolution::resolved_repeated_iters(
            params.pipeline,
            params.run_state,
            params.node_id,
        ),
        // Absolute snapshot paths: the sub-worktree does not inherit the snapshot
        // files.
        secondary_repos: crate::prompt_augmenter::secondary_repo_contexts(
            params.repo_root,
            params.run_id,
            &params.run_state.target_repos,
        ),
        // Constant by construction here: the `Reusable` arm of
        // `ensure_sub_worktree` is unreachable on this path, so no interrupted-op
        // notice is ever routed.
        reused_sub_worktree: false,
        interrupted_git_ops: &[],
        // This path only runs for a never-started iteration, so no partial output
        // can survive; restart-with-artifacts is `node_spawn`'s concern.
        partial_outputs: &[],
    };

    let full_prompt = crate::prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

    // A `script` node (ADR-0017) runs the author's bash instead of Claude: its I/O
    // arrives as env vars (a script can't read the prose preamble) and its declared
    // output dirs must exist before the body's `>` redirect runs. The file `bash`
    // executes must be the RAW body, never the augmented prompt.
    let is_script = node.node_type == pipeline::NodeType::Script;

    // An empty script body would `bash <empty>` → exit 0 → a silent no-op that
    // masquerades as success. `create_run` refuses it at launch; this guards the
    // manual force-spawn / restart door.
    if is_script && role_prompt.trim().is_empty() {
        return StartNodeResult {
            outcome: PrimitiveOutcome::Rejected {
                reason: format!("script node '{}' has an empty body", params.node_id),
            },
            events: vec![],
            spawn: None,
        };
    }

    let spawn_prompt: &str = if is_script {
        &role_prompt
    } else {
        &full_prompt
    };
    let script_env = if is_script {
        crate::prompt_augmenter::precreate_output_dirs(&aug_ctx);
        crate::prompt_augmenter::build_script_env(&aug_ctx)
    } else {
        Vec::new()
    };
    // ADR-0046 harness resolution; mirrors `node_spawn`. A `script` node resolves
    // no harness.
    let resolved_harness = if is_script {
        None
    } else {
        // Back-compat fold, mirroring `stored_default_harness_models`: keep it here
        // so this primitive stays self-contained given its inputs.
        let mut default_models = params.default_harness_models.clone();
        if !default_models.contains_key(harness_registry::CLAUDE) {
            if let Some(m) = params.default_model.as_deref().filter(|s| !s.is_empty()) {
                default_models.insert(harness_registry::CLAUDE.to_string(), m.to_string());
            }
        }
        let tiers = harness_resolver::HarnessTiers {
            node_pin: node.pin_harness.as_deref(),
            // The Run tier: the harness frozen in this Run's `RunStarted`.
            run: params.run_state.harness.as_deref(),
            // An empty string must never win a tier (the `Some("")` trap).
            project: params.project_harness.as_deref().filter(|s| !s.is_empty()),
            instance_default: params.default_harness.as_deref(),
        };
        Some(harness_resolver::resolve(
            &tiers,
            &node.harnesses,
            &default_models,
        ))
    };
    let harness_descriptor = match &resolved_harness {
        None => None,
        // An unresolvable name must be a rejection that names it — never a silent
        // claude fallback.
        Some(r) => {
            let resolved = match params.harness_registry {
                Some(reg) => reg.resolve(&r.harness),
                None => harness_registry::resolve(&r.harness),
            };
            match resolved {
                Some(d) => Some(d),
                None => {
                    return StartNodeResult {
                        outcome: PrimitiveOutcome::Rejected {
                            reason: format!(
                                "node '{}': unknown harness '{}'",
                                params.node_id, r.harness
                            ),
                        },
                        events: vec![],
                        spawn: None,
                    };
                }
            }
        }
    };
    // A missing harness binary is a spawn that cannot happen: reject, never a 2xx.
    if let Some(d) = &harness_descriptor {
        if params.tmux_cmd_override.is_none() && !tmux_session_manager::binary_available(&d.binary)
        {
            return StartNodeResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!(
                        "node '{}': harness '{}' binary '{}' not found in PATH {} \
                         (ADR-0055: the user's interactive PATH, not the service's) — \
                         install it or check the name; this is not \"not installed\" unless \
                         the binary is truly absent from that PATH",
                        params.node_id,
                        d.name,
                        d.binary,
                        tmux_session_manager::harness_probe_path()
                    ),
                },
                events: vec![],
                spawn: None,
            };
        }
    }
    // Model + effort feed both the tail and the `NodeStarted` payload, which the
    // resume path reads back to re-pose `--effort`.
    let resolved_model = resolved_harness.as_ref().and_then(|r| r.model.clone());
    let resolved_effort = resolved_harness.as_ref().and_then(|r| r.effort.clone());
    // Pin a session id only for a harness that can honour it (`claude`).
    let session_id: Option<String> = harness_descriptor
        .as_ref()
        .filter(|d| d.pins_session_id())
        .map(|_| uuid::Uuid::new_v4().to_string());
    let tail = if is_script {
        StartNodeTail::Script {
            timeout_secs: tmux_session_manager::SCRIPT_TIMEOUT_SECS,
            env: script_env,
        }
    } else {
        StartNodeTail::Agent {
            harness: Box::new(
                harness_descriptor
                    .clone()
                    .expect("a non-script node resolved a harness descriptor"),
            ),
            model: resolved_model.clone(),
            effort: resolved_effort.clone(),
            session_id: session_id.clone(),
        }
    };

    let node_started = event_log::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: event_log::now_iso(),
        kind: EventKind::NodeStarted,
        node_id: Some(params.node_id.to_string()),
        iter: Some(params.iter),
        payload: Some(serde_json::json!({
            "prompt_preview": full_prompt.chars().take(500).collect::<String>(),
            "node_type": node_type_str(&node.node_type),
            // #653/ADR-0060: FREEZE where this NodeRun works. Mirrors the
            // `spawn_node` payload — every later reader asks this event.
            "isolated_worktree": has_sub_worktree,
            "input_paths": input_paths,
            // Launch-time model + effort, **resolved**. Mirrors the `spawn_node`
            // payload.
            "model": resolved_model.as_deref(),
            "effort": resolved_effort.as_deref(),
            // FROZEN so the resume path re-poses what was launched (ADR-0007).
            "harness": resolved_harness.as_ref().map(|r| r.harness.as_str()),
            // Read back by the sweep (transcript resolution) and the resume path.
            // `null` for a script node and for legacy rows.
            "session_id": session_id,
            // #503: the sub-worktree's base commit. Absent for a node with no
            // sub-worktree (a non-isolated agent/script), which never merges back.
            "base_sha": spawn_base_sha,
        })),
    };

    let session_name =
        tmux_session_manager::node_session_name(params.run_id, params.node_id, params.iter);
    // **Do not add a spawn path that spawns before the caller appends
    // `NodeStarted`** (ADR-0038): the reaper's "absent ⇒ orphan" verdict is only
    // sound because no session can exist before its reservation.
    let sandbox = sandboxed.then(|| StartNodeSandbox {
        docker_bin: params.docker_cmd_override.unwrap_or("docker").to_string(),
        uid: crate::sandbox_container::host_uid(),
        gid: crate::sandbox_container::host_gid(),
    });
    let spawn = StartNodeSpawn {
        session_name,
        prompt: spawn_prompt.to_string(),
        working_dir,
        run_id: params.run_id.to_string(),
        node_id: params.node_id.to_string(),
        iter: params.iter,
        daemon_port: params.daemon_port,
        tmux_cmd_override: params.tmux_cmd_override.map(str::to_string),
        tail,
        sandbox,
        // Gated by `!is_script`: a script node runs bash, so it carries no hook.
        inject_hook: params.inject_hook && !is_script,
    };

    let mut events = vec![node_started];

    if node.interactive {
        events.push(event_log::Event {
            id: None,
            run_id: params.run_id.to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeAwaitingUser,
            node_id: Some(params.node_id.to_string()),
            iter: Some(params.iter),
            payload: None,
        });
    }

    StartNodeResult {
        outcome: PrimitiveOutcome::Executed,
        events,
        spawn: Some(spawn),
    }
}

fn has_node_started_event(run_state: &event_log::RunState, node_id: &str, iter: i64) -> bool {
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

fn resolve_inputs(
    params: &StartNodeParams<'_>,
    node: &pipeline::NodeDef,
) -> HashMap<String, String> {
    // Don't re-derive the iteration decision here: it lives in
    // `input_resolution::resolve_consumer_inputs` (latest-COMPLETED iter for a
    // single wire, COMPLETED iters for a `repeated` pool — never a raw `iter-*`
    // disk glob). This path only layers overrides and the entry-node `task`
    // fallback on top; a `repeated` pool flattens to a `\n`-joined string.
    let source_iters = crate::input_resolution::resolved_source_iters(
        params.pipeline,
        params.run_state,
        params.node_id,
        params.iter,
    );
    let repeated_iters = crate::input_resolution::resolved_repeated_iters(
        params.pipeline,
        params.run_state,
        params.node_id,
    );
    let resolved = crate::input_resolution::resolve_consumer_inputs(
        params.pipeline,
        params.artifacts_dir,
        params.node_id,
        params.iter,
        &source_iters,
        &repeated_iters,
    );
    // First resolved input per target port — preserves the previous
    // first-matching-edge semantics when several edges share a target port.
    let mut by_port: HashMap<&str, &crate::input_resolution::ResolvedInput> = HashMap::new();
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

fn node_type_str(nt: &pipeline::NodeType) -> &'static str {
    nt.as_str()
}

pub(crate) struct StopNodeParams<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub iter: i64,
    pub tmux_socket: &'a str,
}

#[allow(dead_code)]
pub(crate) struct StopNodeResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
}

pub(crate) fn stop_node(params: &StopNodeParams<'_>) -> StopNodeResult {
    let session_name =
        tmux_session_manager::node_session_name(params.run_id, params.node_id, params.iter);

    tmux_session_manager::kill(params.tmux_socket, &session_name);

    let stopped_event = event_log::Event {
        id: None,
        run_id: params.run_id.to_string(),
        ts: event_log::now_iso(),
        kind: EventKind::NodeStopped,
        node_id: Some(params.node_id.to_string()),
        iter: Some(params.iter),
        payload: Some(serde_json::json!({
            "reason": "stopped_by_user",
        })),
    };

    StopNodeResult {
        outcome: PrimitiveOutcome::Executed,
        events: vec![stopped_event],
    }
}

pub(crate) struct InvalidateNodesParams<'a> {
    pub run_id: &'a str,
    pub node_ids: &'a [String],
    pub artifacts_dir: &'a Path,
}

#[allow(dead_code)]
pub(crate) struct InvalidateNodesResult {
    pub outcome: PrimitiveOutcome,
    pub events: Vec<event_log::Event>,
    pub deleted_dirs: Vec<PathBuf>,
}

pub(crate) fn invalidate_nodes(params: &InvalidateNodesParams<'_>) -> InvalidateNodesResult {
    if params.node_ids.is_empty() {
        return InvalidateNodesResult {
            outcome: PrimitiveOutcome::Executed,
            events: vec![],
            deleted_dirs: vec![],
        };
    }

    let mut events = Vec::new();
    let mut deleted_dirs = Vec::new();

    for node_id in params.node_ids {
        events.push(event_log::Event {
            id: None,
            run_id: params.run_id.to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeInvalidated,
            node_id: Some(node_id.clone()),
            iter: None,
            payload: Some(serde_json::json!({
                "reason": "invalidated",
            })),
        });

        let artifact_dir = params.artifacts_dir.join(node_id);
        if artifact_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&artifact_dir) {
                tracing::warn!("failed to remove artifacts for {node_id}: {e}");
            } else {
                deleted_dirs.push(artifact_dir);
            }
        }
    }

    InvalidateNodesResult {
        outcome: PrimitiveOutcome::Executed,
        events,
        deleted_dirs,
    }
}

pub(crate) struct InjectOutputsParams<'a> {
    pub(crate) node_id: &'a str,
    pub(crate) iter: i64,
    pub(crate) artifacts: &'a HashMap<String, String>,
    pub(crate) artifacts_dir: &'a Path,
}

#[allow(dead_code)]
pub(crate) struct InjectOutputsResult {
    pub(crate) outcome: PrimitiveOutcome,
    pub(crate) written_paths: Vec<PathBuf>,
}

#[allow(dead_code)]
pub(crate) fn inject_outputs(params: &InjectOutputsParams<'_>) -> InjectOutputsResult {
    if params.artifacts.is_empty() {
        return InjectOutputsResult {
            outcome: PrimitiveOutcome::Executed,
            written_paths: vec![],
        };
    }

    let mut written_paths = Vec::new();

    for (port_name, content) in params.artifacts {
        let port_d =
            blackboard::port_dir(params.artifacts_dir, params.node_id, params.iter, port_name);
        if let Err(e) = std::fs::create_dir_all(&port_d) {
            return InjectOutputsResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("failed to create port directory for {port_name}: {e}"),
                },
                written_paths,
            };
        }

        let file_path = port_d.join("output.md");
        if let Err(e) = std::fs::write(&file_path, content) {
            return InjectOutputsResult {
                outcome: PrimitiveOutcome::Rejected {
                    reason: format!("failed to write artifact for {port_name}: {e}"),
                },
                written_paths,
            };
        }

        written_paths.push(file_path);
    }

    InjectOutputsResult {
        outcome: PrimitiveOutcome::Executed,
        written_paths,
    }
}

/// Deposits a *skipped* node's outputs so a downstream resolver finds a concrete
/// (if empty) artifact rather than a missing file that reads as "not produced".
/// The port set is every declared output port plus every distinct source port the
/// node's outgoing edges read; a node with neither still gets a single default
/// `output` port.
pub(crate) fn write_skip_outputs(
    pipeline: &PipelineDef,
    node_id: &str,
    iter: i64,
    overrides: &HashMap<String, String>,
    artifacts_dir: &Path,
) -> Result<Vec<String>, String> {
    let mut ports: Vec<String> = Vec::new();
    if let Some(node) = pipeline.nodes.iter().find(|n| n.id == node_id) {
        for out in &node.outputs {
            if !ports.contains(&out.name) {
                ports.push(out.name.clone());
            }
        }
    }
    for e in pipeline.edges.iter().filter(|e| e.source.node == node_id) {
        if !ports.contains(&e.source.port) {
            ports.push(e.source.port.clone());
        }
    }
    if ports.is_empty() {
        ports.push("output".to_string());
    }

    let artifacts: HashMap<String, String> = ports
        .iter()
        .map(|port| {
            (
                port.clone(),
                overrides.get(port).cloned().unwrap_or_default(),
            )
        })
        .collect();
    let inject = inject_outputs(&InjectOutputsParams {
        node_id,
        iter,
        artifacts: &artifacts,
        artifacts_dir,
    });
    match inject.outcome {
        PrimitiveOutcome::Rejected { reason } => Err(reason),
        _ => Ok(ports),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{IterationInfo, NodeState, RunState};
    use crate::pipeline::{EdgeDef, EdgeEndpoint, NodeDef, NodeType, Port, PortType};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn make_node(id: &str, node_type: NodeType, inputs: &[&str], outputs: &[&str]) -> NodeDef {
        NodeDef {
            // #653: these fixtures exercise graph wiring, not worktrees — a
            // shared-worktree node keeps them out of `git worktree add` on a
            // scratch dir. Isolation-sensitive tests state `Some(true)`.
            isolated_worktree: Some(false),
            id: id.into(),
            name: id.into(),
            node_type,
            inputs: inputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|n| Port {
                    name: (*n).into(),
                    repeated: false,
                    side: None,
                    port_type: PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                })
                .collect(),
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        }
    }

    fn make_node_with_repeated_input(id: &str, port_name: &str) -> NodeDef {
        NodeDef {
            isolated_worktree: None,
            id: id.into(),
            name: id.into(),
            node_type: NodeType::Agent,
            inputs: vec![Port {
                name: port_name.into(),
                repeated: true,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![Port {
                name: "out".into(),
                repeated: false,
                side: None,
                port_type: PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
        }
    }

    fn make_edge(src_node: &str, src_port: &str, tgt_node: &str, tgt_port: &str) -> EdgeDef {
        EdgeDef {
            source: EdgeEndpoint {
                node: src_node.into(),
                port: src_port.into(),
            },
            target: EdgeEndpoint {
                node: tgt_node.into(),
                port: tgt_port.into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        }
    }

    fn empty_run_state() -> RunState {
        RunState::new("run-1".into(), "test".into())
    }

    fn running_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Running,
            iter,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Running,
                started_at: Some("t0".into()),
                completed_at: None,
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    fn completed_node(id: &str, iter: i64) -> NodeState {
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Completed,
            iter,
            started_at: Some("t0".into()),
            completed_at: Some("t1".into()),
            failure_reason: None,
            skip_reason: None,
            iterations: vec![IterationInfo {
                iter,
                status: NodeStatus::Completed,
                started_at: Some("t0".into()),
                completed_at: Some("t1".into()),
            }],
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    fn pending_node(id: &str) -> NodeState {
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: NodeStatus::Pending,
            iter: 1,
            started_at: None,
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: Vec::new(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    #[test]
    fn start_node_already_started_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), running_node("worker", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::AlreadyDone);
        assert!(result.events.is_empty());
    }

    #[test]
    fn start_node_unknown_node_returns_rejected() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "nonexistent",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &tmp.path().join("artifacts"),
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let result = start_node(&params);
        assert!(matches!(result.outcome, PrimitiveOutcome::Rejected { .. }));
    }

    #[test]
    fn start_node_arms_the_stop_hook_for_an_agent_when_enabled() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &[], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: true,
            harness_registry: None,
        };
        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(
            result.spawn.unwrap().inject_hook,
            "an agent node must arm the Stop hook when the setting is on"
        );
    }

    #[test]
    fn start_node_never_arms_the_stop_hook_for_a_script() {
        // A `script` node must never carry the hook, whatever the setting says.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("builder", NodeType::Script, &[], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        let body_file = crate::pipeline::canonical_prompt_path(&pipeline_path, "builder");
        std::fs::create_dir_all(body_file.parent().unwrap()).unwrap();
        std::fs::write(&body_file, "echo hi > \"$PDO_OUTPUT_OUT\"\n").unwrap();

        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "builder",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &pipeline_path,
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: true,
            harness_registry: None,
        };
        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(
            !result.spawn.unwrap().inject_hook,
            "a script node must never arm the Stop hook"
        );
    }

    #[test]
    fn start_node_resolves_inputs_from_blackboard() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("implementer", NodeType::Agent, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("planner".into(), completed_node("planner", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let plan_dir = artifacts_dir.join("planner").join("iter-1").join("plan");
        std::fs::create_dir_all(&plan_dir).unwrap();
        std::fs::write(plan_dir.join("output.md"), "# Plan\nDo the thing").unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        let plan_path = blackboard::artifact_path(&artifacts_dir, "planner", 1, "plan");
        assert_eq!(
            input_paths.get("plan").unwrap(),
            &plan_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn start_node_with_overrides_uses_override_path() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("implementer", NodeType::Agent, &["plan"], &["summary"]),
            ],
            edges: vec![make_edge("planner", "plan", "implementer", "plan")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let override_path = tmp.path().join("custom_plan.md");
        std::fs::write(&override_path, "# Custom plan").unwrap();

        let mut overrides = HashMap::new();
        overrides.insert("plan".to_string(), override_path.clone());

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: Some(overrides),
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        assert_eq!(
            input_paths.get("plan").unwrap(),
            &override_path.to_string_lossy().to_string()
        );
    }

    #[test]
    fn override_applies_to_an_emergent_input_port() {
        // #486 / #600: an `agent` node declares NO input ports (its
        // inputs are emergent from edges), so an override keyed on the edge's target
        // port must still attach — otherwise the operator's dummy input is silently
        // dropped on exactly those nodes.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("up", NodeType::Agent, &["task"], &["code"]),
                // `impl` declares no inputs — the `code` input is emergent.
                make_node("impl", NodeType::Agent, &[], &["out"]),
            ],
            edges: vec![make_edge("up", "code", "impl", "code")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let override_path = tmp.path().join("dummy.md");
        std::fs::write(&override_path, "# dummy code").unwrap();
        let mut overrides = HashMap::new();
        overrides.insert("code".to_string(), override_path.clone());

        let node = pipeline.nodes.iter().find(|n| n.id == "impl").unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "impl",
            iter: 1,
            overrides: Some(overrides),
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };
        let input_paths = resolve_inputs(&params, node);
        assert_eq!(
            input_paths.get("code").unwrap(),
            &override_path.to_string_lossy().to_string(),
            "override attaches to the emergent `code` port"
        );
    }

    #[test]
    fn write_skip_outputs_deposits_empty_declared_and_edge_ports() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("skipme", NodeType::Agent, &["task"], &["decl_out"]),
                make_node("down", NodeType::Agent, &["edge_out"], &["z"]),
            ],
            edges: vec![make_edge("skipme", "edge_out", "down", "edge_out")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let ports = write_skip_outputs(&pipeline, "skipme", 1, &HashMap::new(), &artifacts_dir)
            .expect("write ok");
        assert!(ports.contains(&"decl_out".to_string()));
        assert!(ports.contains(&"edge_out".to_string()));
        for port in ["decl_out", "edge_out"] {
            let p = blackboard::artifact_path(&artifacts_dir, "skipme", 1, port);
            assert!(p.exists(), "empty artifact written for port {port}");
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        }
    }

    #[test]
    fn write_skip_outputs_uses_override_content_and_a_default_port() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("lonely", NodeType::Agent, &["task"], &[])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let mut overrides = HashMap::new();
        overrides.insert("output".to_string(), "provided".to_string());

        let ports =
            write_skip_outputs(&pipeline, "lonely", 1, &overrides, &artifacts_dir).expect("ok");
        assert_eq!(ports, vec!["output".to_string()]);
        let p = blackboard::artifact_path(&artifacts_dir, "lonely", 1, "output");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "provided");
    }

    #[test]
    fn start_node_resolves_task_port_from_input_artifact() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("entry", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let run_state = empty_run_state();
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline.nodes.iter().find(|n| n.id == "entry").unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "entry",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        let expected = blackboard::input_path(&artifacts_dir);
        assert_eq!(
            input_paths.get("task").unwrap(),
            &expected.to_string_lossy().to_string()
        );
    }

    #[test]
    fn start_node_resolves_fan_in_inputs() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("planner", NodeType::Agent, &["task"], &["plan"]),
                make_node("researcher", NodeType::Agent, &["task"], &["research"]),
                make_node(
                    "implementer",
                    NodeType::Agent,
                    &["plan", "research"],
                    &["summary"],
                ),
            ],
            edges: vec![
                make_edge("planner", "plan", "implementer", "plan"),
                make_edge("researcher", "research", "implementer", "research"),
            ],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("planner".into(), completed_node("planner", 1));
        run_state
            .nodes
            .insert("researcher".into(), completed_node("researcher", 1));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        assert!(input_paths.contains_key("plan"));
        assert!(input_paths.contains_key("research"));
    }

    fn completed_node_iters(id: &str, iters: &[(i64, NodeStatus)]) -> NodeState {
        let (head_iter, head_status) = iters.last().cloned().unwrap_or((1, NodeStatus::Pending));
        NodeState {
            isolated_worktree: None,
            harness: None,
            cost: None,
            node_id: id.into(),
            status: head_status,
            iter: head_iter,
            started_at: Some("t0".into()),
            completed_at: None,
            failure_reason: None,
            skip_reason: None,
            iterations: iters
                .iter()
                .map(|(iter, status)| IterationInfo {
                    iter: *iter,
                    status: status.clone(),
                    started_at: Some("t0".into()),
                    completed_at: None,
                })
                .collect(),
            frontmatter_retries: 0,
            frontmatter_violations: Vec::new(),
            missing_outputs: Vec::new(),
            delivery: None,
        }
    }

    #[test]
    fn start_node_resolves_repeated_port_via_projection() {
        // A `repeated` edge resolves to concrete completed-iteration paths, NOT a
        // raw `iter-*` glob; `repeated` is read off the EDGE, and a failed iter is
        // quarantined even when its files are on disk.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node_with_repeated_input("implementer", "reviews"),
            ],
            edges: vec![EdgeDef {
                repeated: true,
                ..make_edge("reviewer", "review", "implementer", "reviews")
            }],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state.nodes.insert(
            "reviewer".into(),
            completed_node_iters(
                "reviewer",
                &[
                    (1, NodeStatus::Completed),
                    (2, NodeStatus::Failed),
                    (3, NodeStatus::Completed),
                ],
            ),
        );
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        let reviews_path = input_paths.get("reviews").unwrap();
        assert!(
            !reviews_path.contains("iter-*"),
            "no raw glob: {reviews_path}"
        );
        assert!(reviews_path.contains("iter-1/review/output.md"));
        assert!(reviews_path.contains("iter-3/review/output.md"));
        assert!(
            !reviews_path.contains("iter-2"),
            "failed iter-2 is quarantined: {reviews_path}"
        );
        assert_eq!(reviews_path.lines().count(), 2);
    }

    #[test]
    fn start_node_uses_latest_iter_for_upstream() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node("implementer", NodeType::Agent, &["review"], &["summary"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("reviewer".into(), completed_node("reviewer", 3));

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        let review_path = input_paths.get("review").unwrap();
        assert!(
            review_path.contains("iter-3"),
            "should resolve to iter-3 (latest), got: {review_path}"
        );
    }

    #[test]
    fn resolve_inputs_reads_latest_completed_not_failed_iter() {
        // A non-repeated cross-iteration edge whose source failed at iter-1 then
        // completed at iter-2 must record iter-2 — never the failed iter-1, and
        // never the consumer's positional iter.
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![
                make_node("reviewer", NodeType::Agent, &["task"], &["review"]),
                make_node("implementer", NodeType::Agent, &["review"], &["summary"]),
            ],
            edges: vec![make_edge("reviewer", "review", "implementer", "review")],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state.nodes.insert(
            "reviewer".into(),
            completed_node_iters(
                "reviewer",
                &[(1, NodeStatus::Failed), (2, NodeStatus::Completed)],
            ),
        );

        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let node = pipeline
            .nodes
            .iter()
            .find(|n| n.id == "implementer")
            .unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "implementer",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &artifacts_dir,
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let input_paths = resolve_inputs(&params, node);
        let review_path = input_paths.get("review").unwrap();
        assert!(
            review_path.contains("iter-2/review/output.md"),
            "resolves to the latest-completed iter-2, got: {review_path}"
        );
        assert!(
            !review_path.contains("iter-1"),
            "the failed iter-1 is quarantined, got: {review_path}"
        );
    }

    #[test]
    fn stop_node_emits_node_stopped_event() {
        let params = StopNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            tmux_socket: "pdo-test",
        };

        let result = stop_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 1);

        let event = &result.events[0];
        assert_eq!(event.kind, EventKind::NodeStopped);
        assert_eq!(event.node_id.as_deref(), Some("worker"));
        assert_eq!(event.iter, Some(1));

        let payload = event.payload.as_ref().unwrap();
        assert_eq!(payload["reason"], "stopped_by_user");
    }

    #[test]
    fn stop_node_does_not_trigger_scheduler() {
        let params = StopNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            tmux_socket: "pdo-test",
        };

        let result = stop_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        for event in &result.events {
            assert_ne!(event.kind, EventKind::RunCompleted);
            assert_ne!(event.kind, EventKind::RunFailed);
        }
    }

    #[test]
    fn invalidate_nodes_resets_to_pending_and_deletes_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let node_a_dir = artifacts_dir.join("node-a").join("iter-1").join("out");
        std::fs::create_dir_all(&node_a_dir).unwrap();
        std::fs::write(node_a_dir.join("output.md"), "# Output A").unwrap();

        let node_b_dir = artifacts_dir.join("node-b").join("iter-1").join("out");
        std::fs::create_dir_all(&node_b_dir).unwrap();
        std::fs::write(node_b_dir.join("output.md"), "# Output B").unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &["node-a".to_string(), "node-b".to_string()],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 2);

        for event in &result.events {
            assert_eq!(event.kind, EventKind::NodeInvalidated);
        }

        assert!(!artifacts_dir.join("node-a").exists());
        assert!(!artifacts_dir.join("node-b").exists());
        assert_eq!(result.deleted_dirs.len(), 2);
    }

    #[test]
    fn invalidate_nodes_empty_list_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &[],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(result.events.is_empty());
        assert!(result.deleted_dirs.is_empty());
    }

    #[test]
    fn invalidate_already_pending_node_still_emits_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let params = InvalidateNodesParams {
            run_id: "run-1",
            node_ids: &["clean-node".to_string()],
            artifacts_dir: &artifacts_dir,
        };

        let result = invalidate_nodes(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].kind, EventKind::NodeInvalidated);
        assert!(result.deleted_dirs.is_empty());
    }

    #[test]
    fn inject_outputs_writes_files_to_port_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let mut artifacts = HashMap::new();
        artifacts.insert(
            "review".to_string(),
            "---\nverdict: PASS\n---\n\nLooks good.".to_string(),
        );
        artifacts.insert("summary".to_string(), "# Summary\nAll done.".to_string());

        let params = InjectOutputsParams {
            node_id: "reviewer",
            iter: 2,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert_eq!(result.written_paths.len(), 2);

        let review_path = blackboard::artifact_path(&artifacts_dir, "reviewer", 2, "review");
        assert!(review_path.exists());
        let content = std::fs::read_to_string(&review_path).unwrap();
        assert!(content.contains("verdict: PASS"));

        let summary_path = blackboard::artifact_path(&artifacts_dir, "reviewer", 2, "summary");
        assert!(summary_path.exists());
        let content = std::fs::read_to_string(&summary_path).unwrap();
        assert!(content.contains("All done"));
    }

    #[test]
    fn inject_outputs_empty_artifacts_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let artifacts = HashMap::new();

        let params = InjectOutputsParams {
            node_id: "reviewer",
            iter: 1,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);
        assert!(result.written_paths.is_empty());
    }

    #[test]
    fn inject_outputs_overwrites_existing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts_dir = tmp.path().join("artifacts");

        let port_dir = artifacts_dir.join("worker").join("iter-1").join("out");
        std::fs::create_dir_all(&port_dir).unwrap();
        std::fs::write(port_dir.join("output.md"), "old content").unwrap();

        let mut artifacts = HashMap::new();
        artifacts.insert("out".to_string(), "new content".to_string());

        let params = InjectOutputsParams {
            node_id: "worker",
            iter: 1,
            artifacts: &artifacts,
            artifacts_dir: &artifacts_dir,
        };

        let result = inject_outputs(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::Executed);

        let content = std::fs::read_to_string(blackboard::artifact_path(
            &artifacts_dir,
            "worker",
            1,
            "out",
        ))
        .unwrap();
        assert_eq!(content, "new content");
    }

    #[test]
    fn double_start_returns_already_done() {
        let pipeline = PipelineDef {
            name: "test".into(),
            version: None,
            variables: HashMap::new(),
            nodes: vec![make_node("worker", NodeType::Agent, &["task"], &["out"])],
            edges: vec![],
            loops: Vec::new(),
            notes: Vec::new(),
            prompt_required: true,
        };

        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), completed_node("worker", 1));

        let tmp = tempfile::tempdir().unwrap();
        let params = StartNodeParams {
            run_id: "run-1",
            node_id: "worker",
            iter: 1,
            overrides: None,
            pipeline: &pipeline,
            run_state: &run_state,
            artifacts_dir: &tmp.path().join("artifacts"),
            worktree_dir: tmp.path(),
            repo_root: tmp.path(),
            pipeline_path: &tmp.path().join("pipeline.yaml"),
            resolved_vars: &HashMap::new(),
            daemon_port: 5172,
            tmux_cmd_override: Some("exec true"),
            docker_cmd_override: None,
            default_model: None,
            default_harness: None,
            default_harness_models: Default::default(),
            project_harness: None,
            inject_hook: false,
            harness_registry: None,
        };

        let result = start_node(&params);
        assert_eq!(result.outcome, PrimitiveOutcome::AlreadyDone);
    }

    #[test]
    fn start_node_pending_status_allows_start() {
        let mut run_state = empty_run_state();
        run_state
            .nodes
            .insert("worker".into(), pending_node("worker"));

        let result = has_node_started_event(&run_state, "worker", 1);
        assert!(!result, "pending node should be startable");
    }
}
