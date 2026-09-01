//! Node spawn primitive: the single, injectable sequence that turns a ready
//! NodeRun into a live tmux session under the global admission cap.
//!
//! Carved out of the lib.rs god-file (#356) next to its callers, mirroring
//! worktree_ops (#276) and run_advance. `spawn_node` takes a narrow
//! `SpawnDeps` (db, event sink, admission lock, panic flag, port, tmux
//! override) instead of the full `AppState`, so its ordering invariants —
//! transition guard (#212), atomic cap check-and-reserve (#213), panic
//! isolation + orphan reaping, and "fail loud as RunFailed" (#279) — are
//! unit-testable without a live daemon. It is a leaf primitive (ADR-0009,
//! Couche 2): it appends events and touches tmux/worktree, and never
//! re-enters the scheduler.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use tracing::{error, info, warn};

use crate::worktree_ops::{
    ensure_sub_worktree, reap_orphan_sub_worktree, sub_worktree_branch, sub_worktree_path,
};
use crate::{
    admission, agent_choice, agent_profile, append_event_with,
    count_global_live_sessions_excluding, event_log, harness_registry, input_resolution,
    merge_action, panic_payload_message, pipeline, prompt_augmenter, reload_run_state_with,
    stored_autocomplete_turn_end, stored_default_harness, stored_default_harness_models,
    stored_session_cap, tmux_session_manager, transition_guard, AppState,
};

pub(crate) struct SpawnContext<'a> {
    pub(crate) pipeline: &'a pipeline::PipelineDef,
    pub(crate) run_id: &'a str,
    pub(crate) pipeline_path: &'a std::path::Path,
    pub(crate) worktree_dir: &'a std::path::Path,
    pub(crate) artifacts_dir: &'a std::path::Path,
    pub(crate) resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    pub(crate) repo_root: &'a std::path::Path,
}

/// Narrow, hand-buildable bundle of the exactly-six side-effects `spawn_node`
/// touches. A struct-of-borrows (NOT a trait): `admission_lock` is a bare
/// `tokio::sync::Mutex`, so the guard it hands out borrows `AppState`'s
/// lifetime — a trait object would fight that borrow. Every field is a `Copy`
/// reference / `u16` / `Option<&str>`, so the whole thing is `Copy` and can be
/// threaded on to `fail_spawn_before_start` without reborrow gymnastics.
///
/// `from_state` is the only production constructor; a unit test builds the same
/// struct out of a `test_state_with_dir` `AppState`, which is the whole point of
/// the seam (#356) — spawn is drivable in-process with fakes.
#[derive(Clone, Copy)]
pub(crate) struct SpawnDeps<'a> {
    pub(crate) db: &'a sqlx::SqlitePool,
    pub(crate) event_tx: &'a tokio::sync::broadcast::Sender<event_log::Event>,
    pub(crate) admission_lock: &'a tokio::sync::Mutex<()>,
    pub(crate) panic_on_spawn: &'a std::sync::atomic::AtomicBool,
    pub(crate) port: u16,
    pub(crate) tmux_cmd_override: Option<&'a str>,
    /// Per-daemon `docker` binary override for the sandbox wiring (#407), `Copy`
    /// like the rest of the bundle. `None` in production (real `docker`).
    pub(crate) docker_cmd_override: Option<&'a str>,
    /// The host home root override (#407 seam), borrowed so the bundle stays
    /// `Copy`. `None` in production ⇒ the real `$HOME`. #553 uses it as the root
    /// under which the **disk descriptor tier** (`~/.pdo/harnesses/`) is read, so a
    /// user-declared harness resolves at spawn — and so a layer-3 test that sets
    /// the override reads descriptors from its own tempdir, never the real home.
    pub(crate) home_override: Option<&'a std::path::Path>,
}

impl<'a> SpawnDeps<'a> {
    /// Project the six spawn side-effects out of the full daemon state.
    pub(crate) fn from_state(state: &'a AppState) -> Self {
        Self {
            db: &state.db,
            event_tx: &state.event_tx,
            admission_lock: &state.admission_lock,
            panic_on_spawn: &state.panic_on_spawn,
            port: state.port,
            tmux_cmd_override: state.tmux_cmd_override.as_deref(),
            docker_cmd_override: state.docker_cmd_override.as_deref(),
            home_override: state.sandbox_home_override.as_deref(),
        }
    }
}

/// The host home root the disk descriptor tier (#553) is read under: the per-daemon
/// override when set (the layer-3 seam), else the real `$HOME`. Mirrors
/// `sandbox_run::sandbox_home_roots` exactly, but reachable from the narrow
/// [`SpawnDeps`] bundle (which carries no `AppState`). An unresolved `$HOME`
/// degrades to an empty root, i.e. the embedded floor — never a spawn failure.
fn spawn_home_root(deps: &SpawnDeps<'_>) -> PathBuf {
    deps.home_override
        .map(PathBuf::from)
        .or_else(|| crate::sandbox_staging::default_roots_from_env().map(|(home, _)| home))
        .unwrap_or_default()
}

/// #553 / ADR-0031: say ONCE per `(run, harness)` that a sandboxed Run's node runs
/// on a harness with no staging floor. The message is the pure
/// [`crate::harness_probes::staging_floor_absence_note`] (so it is unit-tested
/// there, not against a terminal); the process-static dedup keeps a busy scheduler
/// from repeating it on every retry or collection lap. A no-op for `claude`, which
/// has the floor.
fn warn_missing_staging_floor_once(run_id: &str, harness: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let Some(note) = crate::harness_probes::staging_floor_absence_note(harness) else {
        return;
    };
    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), harness.to_string())) {
        warn!("run {run_id}: {note}");
    }
}

/// #613 / ADR-0051 (AC #7): say ONCE per `(run, harness)` that turn-end
/// auto-completion is enabled but the node's harness has no end-of-turn substrate,
/// so the setting cannot be honoured for it. The message is the pure
/// [`crate::harness_probes::turn_end_absence_note`] (unit-tested there); the
/// process-static dedup keeps a busy scheduler from repeating it. A no-op for
/// `claude` (which has the substrate) and whenever the setting is off. Without this
/// the setting was a **silent** no-op on a substrate-less harness — the very thing
/// this ticket removes.
fn warn_turn_end_unsupported_once(run_id: &str, harness: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let Some(note) = crate::harness_probes::turn_end_absence_note(harness) else {
        return;
    };
    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), harness.to_string())) {
        warn!("run {run_id}: {note}");
    }
}

/// #563 (AC13/AC14): say ONCE per `(run, tier, profile_id)` that a tier named a
/// `Profile` reference absent from the atomic snapshot — the walk warned and
/// behaved as `Inherit` for that tier rather than failing the spawn. Deduped the
/// same way as [`warn_missing_staging_floor_once`] so a busy scheduler replaying
/// the same stale reference doesn't spam the log every retry.
fn warn_missing_agent_profile_once(
    run_id: &str,
    warning: &crate::agent_choice::MissingProfileWarning,
) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String, String)>>> = Mutex::new(None);

    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    let key = (
        run_id.to_string(),
        format!("{:?}", warning.tier),
        warning.profile_id.clone(),
    );
    if said.insert(key) {
        warn!(
            "run {run_id}: agent profile '{}' referenced at tier {:?} no longer exists — \
             falling through as if that tier stated no choice",
            warning.profile_id, warning.tier
        );
    }
}

/// A collection-region member (ADR-0011 / #269) reads its OWN deposited item:
/// the fan-out deposits `_item.md` under the entry's artifact dir, one per
/// lap — there is no separate driver node like the retired ForEach.
fn find_collection_context(
    spawn_ctx: &SpawnContext<'_>,
    node_id: &str,
    iter: i64,
) -> Option<prompt_augmenter::ForEachContext> {
    crate::loop_region::collection_region_for_member(spawn_ctx.pipeline, node_id)?;
    let item_path = spawn_ctx
        .artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join("_item.md");
    let item_content = std::fs::read_to_string(&item_path).ok()?;
    let total = std::fs::read_dir(spawn_ctx.artifacts_dir.join(node_id))
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
    Some(prompt_augmenter::ForEachContext {
        current_item,
        current_iter: iter,
        total,
    })
}

/// What actually happened in a `spawn_node` call (ADR-0025 / #327). Every exit
/// path is distinguishable so callers that must tell the truth about a
/// re-scheduling (`re_evaluate_after_command`) can report the real effect
/// instead of assuming success. Callers on fire-and-forget paths simply drop it
/// (intentionally not `#[must_use]`).
#[derive(Debug, Clone)]
pub(crate) enum SpawnOutcome {
    /// A tmux session was launched and `NodeStarted` recorded.
    ///
    /// Carries what the caller has to be able to say out loud (#489-A): a
    /// `restart_node` that reused an existing sub-worktree handed the fresh agent
    /// the dead session's uncommitted work, and that changes what the operator (or
    /// the manager) should tell it to do.
    Spawned {
        /// The sub-worktree already existed on the right branch and was reused **in
        /// place** — nothing was re-cut, nothing was destroyed. Always `false` for
        /// a node that owns no sub-worktree.
        reused_sub_worktree: bool,
        /// The commit the sub-worktree is cut from (#503 / ADR-0036), carried over
        /// unchanged on a reuse. `None` for a node with no sub-worktree.
        base_sha: Option<String>,
        /// Every interrupted git operation found in the reused worktree's private
        /// gitdir (`index.lock`, `MERGE_HEAD`, `rebase-*`), in scan order (#516).
        /// Reported, not removed — see `worktree_ops::ensure_sub_worktree`. Empty
        /// for a fresh cut or a node with no sub-worktree.
        interrupted_git_ops: Vec<String>,
    },
    /// Admission cap reached: the node entered `waiting` (`NodeWaiting`
    /// appended); `retry_waiting_nodes` re-drives it later.
    Throttled,
    /// The transition guard refused the spawn before any side effect
    /// (already live / already completed iteration).
    Refused { reason: String },
    /// The Run is sandboxed and its container is not up yet (#445): the spawn was
    /// declined before any side effect and **no event was appended**, so the node
    /// keeps no state and the scheduler still reports it ready. It is replayed when
    /// `SandboxPrepReady` drives the next `advance_run`.
    ///
    /// Distinct from [`SpawnOutcome::Refused`] (nothing to retry — the work is
    /// already live or done) and from [`SpawnOutcome::Throttled`] (a `NodeWaiting`
    /// reservation *was* appended and the cross-run admission sweep owns the retry).
    Deferred { reason: String },
    /// The spawn aborted (empty script body, worktree creation failure,
    /// panic/error in the isolated span) — a failure was recorded.
    Failed { reason: String },
}

pub(crate) async fn spawn_node(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) -> SpawnOutcome {
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
    // Events kept alongside the projection: #503's `base_sha` lives on the previous
    // `NodeStarted` of this same iteration, and a reuse has to carry it forward
    // (#489-B / ADR-0037 §6).
    let loaded = reload_run_state_with(deps.db, run_id).await;
    let projected = loaded.as_ref().map(|(_, s)| s);
    match transition_guard::validate_transition(projected, &started_probe) {
        transition_guard::Verdict::Allow => {}
        transition_guard::Verdict::NoOp { reason } => {
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return SpawnOutcome::Refused { reason };
        }
        transition_guard::Verdict::Reject { reason } => {
            // #515: the cause is typed now; forward its historical prose (the
            // `Refused` outcome carries a `String`, unchanged).
            let reason = reason.to_string();
            warn!("spawn_node refused for {} iter {iter}: {reason}", node.id);
            return SpawnOutcome::Refused { reason };
        }
    }

    // #407: whether the Run is sandboxed (immutable, projected from RunStarted). When
    // it is, the tail below is wrapped to run inside `pdo-sbx-<run_id>`. Read from the
    // guard projection — `sandbox` never changes over a Run's life. A `bool` and not
    // the mode (#432): the profile name is owned, and only the off-ness matters here.
    let run_sandboxed = projected.is_some_and(|s| !s.sandbox.is_off());

    // #445: SANDBOX PRECONDITION — "a sandboxed Run whose prep is not `ready` is not
    // schedulable". Carried by the spawn itself, deliberately NOT by its callers: the
    // create path gated correctly (`lib.rs`, the detached prep task) while the pipeline
    // watcher and `retry_waiting_nodes` reached the spawn with no idea a container was
    // still being built, so the tail below `docker exec`ed into a name that did not
    // exist yet → exit 1 in ~30 ms → the tmux window's command ended → `session_died`.
    // Enforced HERE (after the transition guard, before admission and before any
    // sub-worktree is created) so the invariant holds for every present and future
    // caller, exactly as the transition guard does for illegal starts.
    //
    // Appends NOTHING and reserves nothing. That is load-bearing for the replay: a
    // node with no state stays in `compute_ready_to_spawn`, so the `advance_run` that
    // follows `SandboxPrepReady` starts it. A `NodeWaiting` reservation would flip it
    // to `Waiting`, which `compute_ready_to_spawn` skips — the deferred spawn would
    // then depend on the *cross-run* admission sweep and a Run whose only trigger was
    // the watcher could wedge for ever. `run_stall_reason` knows about this window and
    // will not read it as a silent spawn-abort (it fails loud only past its own
    // sandbox-prep grace).
    if let Some(reason) = projected.and_then(|s| s.sandbox_spawn_block()) {
        info!(
            "spawn_node deferred for {} iter {iter}: {reason} (replayed on sandbox_prep_ready)",
            node.id
        );
        return SpawnOutcome::Deferred { reason };
    }

    // #248 / ADR-0017: refuse to spawn a `script` node with an empty body — it
    // would `bash <empty>` → exit 0 → a silent no-op masquerading as success.
    // `create_run` guards this at launch, but the scheduler and `restart_node`
    // reach `spawn_node` directly, and a mid-run edit could have emptied a
    // pending script's body since launch. Fail loud (before admission / any side
    // effect) rather than silently no-op.
    if node.node_type == pipeline::NodeType::Script {
        let body_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
        let body_empty = std::fs::read_to_string(&body_path)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if body_empty {
            let reason = format!("script node {} has an empty body", node.id);
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                None,
                &reason,
            )
            .await;
            return SpawnOutcome::Failed { reason };
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
    //
    // #489-C: the count EXCLUDES the slot this very spawn is taking back. A
    // `restart_node` kills the node's session and then re-spawns the SAME
    // iteration, but appends no lifecycle event in between — so the node still
    // projects `Running` and, at `live == cap`, the restart is throttled against
    // *itself*, deterministically. Nothing then rescues it: `retry_waiting_nodes`
    // has no timer, `resume_run` treats a throttled node as owned by the sweep,
    // boot recovery only looks at `Running`/`AwaitingUser`, and the Stop button
    // 409s because `node_stop` requires `Running`. The Run froze for good.
    //
    // The exclusion key is `(run_id, node_id, iter)` — never `(node_id, iter)`:
    // the count is global across Runs while node ids are local to a pipeline, so
    // two concurrent Runs of the same pipeline both have an `implementer` at
    // `iter 1`, and a Run-blind key would discount the *other* Run's live session
    // and overshoot the cap. It is unconditional because it can only ever subtract
    // when that exact triple currently holds a session, and `validate_start` allows
    // re-spawning a live iteration for exactly one reason: this spawn replaces it
    // (`transition_guard.rs`, "Same iter: legal restart/promotion").
    //
    // Computed INSIDE the lock, from the same all-Runs projection as the count —
    // reusing the pre-lock projection above would reopen the check-and-reserve race
    // the lock exists to close.
    let admission_guard = deps.admission_lock.lock().await;
    let cap = admission::configured_cap_with(stored_session_cap(deps.db).await);
    let live = count_global_live_sessions_excluding(
        deps.db,
        Some(admission::SlotExclusion {
            run_id,
            node_id: &node.id,
            iter,
        }),
    )
    .await;
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
        if let Err(e) = append_event_with(deps.db, deps.event_tx, &waiting).await {
            error!("failed to append node_waiting for {}: {e}", node.id);
        }
        info!(
            "node {} throttled into waiting ({live}/{cap} sessions live)",
            node.id
        );
        return SpawnOutcome::Throttled;
    }

    let canonical_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let foreach_context = find_collection_context(spawn_ctx, &node.id, iter);

    // #550/ADR-0046: resolve the harness ONCE, here — before any side effect — so
    // its result freezes into `NodeStarted` and is re-posed at resume (ADR-0007).
    // A `script` node launches no agent (ADR-0017), so it resolves no harness. The
    // binary fail-fast runs BEFORE the sub-worktree is created and BEFORE the
    // reservation span, so a missing harness leaves no orphan and writes no spawn
    // event (AC #10 / ADR-0037: never a 2xx for a spawn that did not happen).
    // Skipped under the tmux command override — the test seam launches a stand-in,
    // not the real binary.
    let resolved_harness = if node.node_type == pipeline::NodeType::Script {
        None
    } else {
        let default_harness = stored_default_harness(deps.db).await;
        let default_models = stored_default_harness_models(deps.db).await;
        // #552/ADR-0046: the Projet tier — the harness carried by the Projet that
        // owns this Run's **primary** repo. The primary is `target_repo` (else the
        // daemon repo root — the same `effective_repo` key the lists group and
        // attach by). A secondary repo (ADR-0042) lives in `target_repos` and is
        // never consulted, so adding or removing one changes neither the Projet nor
        // the resolved harness. A DB error degrades the tier to transparent — a
        // Projet lookup never fails a spawn.
        let primary_repo = projected
            .and_then(|s| s.target_repo.clone())
            .unwrap_or_else(|| spawn_ctx.repo_root.to_string_lossy().into_owned());
        let project_harness =
            match crate::project_store::harness_for_path(deps.db, &primary_repo).await {
                Ok(h) => h,
                Err(e) => {
                    warn!("project harness lookup failed for {primary_repo}: {e}");
                    None
                }
            };
        // #563/ADR-0057: the Projet's `AgentChoice`, read alongside its legacy
        // `harness` above — same primary-repo key, same "a DB error degrades the
        // tier to transparent" posture.
        let project_choice =
            match crate::project_store::agent_choice_for_path(deps.db, &primary_repo).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("project agent_choice lookup failed for {primary_repo}: {e}");
                    None
                }
            };
        // #563: a second fresh `instance_config::get` rather than plumbing the whole
        // config through `stored_default_*`. A DB error degrades the tier to
        // transparent, same posture as every other tier here.
        let instance_choice = crate::instance_config::get(deps.db)
            .await
            .ok()
            .and_then(|c| c.agent_choice);
        // #563: the atomic profile snapshot — taken ONCE per spawn (ADR-0057 ¶4),
        // so every `Profile` reference this resolution touches (node/run/project/
        // instance) resolves against the exact same point-in-time read. A DB error
        // degrades to an empty snapshot: `agent_choice::resolve` still falls through
        // to its defensive `claude` floor rather than failing the spawn.
        let profiles = agent_profile::snapshot(deps.db).await.unwrap_or_default();
        let tiers = agent_choice::Tiers {
            node_choice: node.agent_choice.as_ref(),
            node_pin: node.pin_harness.as_deref(),
            node_harnesses: Some(&node.harnesses),
            // #551: the Run tier — the harness frozen in this Run's `RunStarted`
            // (`projected` is the fresh projection loaded at the head of this spawn).
            // A pinned node still ignores it; a free node follows it (ADR-0046).
            run_choice: projected.and_then(|s| s.agent_choice.as_ref()),
            run_harness: projected.and_then(|s| s.harness.as_deref()),
            // #552: the Projet tier — resolved just above from this Run's primary
            // repo. Sits below the Run and above the instance default (ADR-0046).
            project_choice: project_choice.as_ref(),
            project_harness: project_harness.as_deref(),
            instance_choice: instance_choice.as_ref(),
            instance_default_harness: default_harness.as_deref(),
            instance_default_models: Some(&default_models),
        };
        let resolved = agent_choice::resolve(&tiers, &profiles, agent_profile::DEFAULT_PROFILE_ID);
        for warning in &resolved.warnings {
            warn_missing_agent_profile_once(run_id, warning);
        }
        Some(resolved.combo)
    };
    let harness_descriptor = match &resolved_harness {
        None => None,
        Some(r) => {
            // #553: resolve against the embedded floor MERGED with the user's disk
            // descriptor tier (`~/.pdo/harnesses/`), so a harness declared in data
            // launches without a rebuild. The registry reads a root we hand it — it
            // never touches `$HOME` itself.
            let registry = harness_registry::HarnessRegistry::load(&spawn_home_root(&deps));
            match registry.resolve(&r.harness) {
                Some(d) => {
                    // #553 / ADR-0031: a sandboxed Run whose node runs on a harness
                    // with no staging floor holds only by the profile's image and
                    // its `$HOME` exceptions — say it once, visibly (the plancher is
                    // claude-specific and built per-Run regardless of the harness).
                    if run_sandboxed {
                        warn_missing_staging_floor_once(run_id, &r.harness);
                    }
                    Some(d)
                }
                None => {
                    return SpawnOutcome::Failed {
                        reason: format!("node {}: unknown harness '{}'", node.id, r.harness),
                    };
                }
            }
        }
    };
    if let Some(d) = &harness_descriptor {
        if deps.tmux_cmd_override.is_none() && !tmux_session_manager::binary_available(&d.binary) {
            return SpawnOutcome::Failed {
                reason: format!(
                    "node {}: harness '{}' binary '{}' not found in PATH {} \
                     (ADR-0055: the user's interactive PATH, not the service's)",
                    node.id,
                    d.name,
                    d.binary,
                    tmux_session_manager::harness_probe_path()
                ),
            };
        }
    }

    // #653 / ADR-0060: where this NodeRun works. The FROZEN value wins over the
    // document — a re-spawn of the same iteration (restart / invalidate) must
    // land back in the directory the interrupted one left, even if the graph was
    // edited in between (ADR-0007). Only a first spawn reads the node's own
    // `isolated_worktree`, and that reading is what gets frozen below.
    let has_sub_worktree = loaded
        .as_ref()
        .and_then(|(events, _)| merge_action::frozen_isolation(events, &node.id, iter))
        .unwrap_or_else(|| node.is_isolated());

    // Track the sub-worktree + branch this spawn creates so an abort in the
    // panic-isolated span below can reap them (#279). `None` for nodes that own
    // no worktree (a non-isolated agent/script, control nodes).
    let mut orphan_to_reap: Option<(PathBuf, String)> = None;
    // #503 / ADR-0036: the commit the sub-worktree is cut from, recorded on
    // `NodeStarted`. It is the sole basis on which a later merge-back conflict may
    // be resolved in the node's favour — no base recorded, no resolution.
    let mut spawn_base_sha: Option<String> = None;
    let mut reused_sub_worktree = false;
    // #516: every interrupted git op left in a reused sub-worktree, in scan order.
    // Routed to both the re-spawned node's preamble and the wire response.
    let mut interrupted_git_ops: Vec<String> = Vec::new();
    let node_provisioning = if has_sub_worktree {
        let frozen = loaded.as_ref().and_then(|(events, _)| {
            crate::provisioning::frozen_node_rules(events, &node.id, iter)
        });
        match frozen {
            Some(rules) => rules,
            None => match crate::provisioning::node_rules_from_pipeline(
                spawn_ctx.pipeline_path,
                &node.id,
            ) {
                Ok(rules) => rules,
                Err(e) => {
                    return SpawnOutcome::Failed {
                        reason: format!(
                            "provisioning failed for {}: {e:#}; no node was spawned",
                            node.id
                        ),
                    };
                }
            },
        }
    } else {
        crate::provisioning::ProvisioningRules::default()
    };
    let working_dir = if has_sub_worktree {
        let sub_wt_dir = sub_worktree_path(spawn_ctx.repo_root, run_id, &node.id, iter);
        let sub_branch = sub_worktree_branch(run_id, &node.id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");

        // #503 / ADR-0036 under reuse (ADR-0037 §6): a reuse does not cut anything,
        // so it carries the ORIGINAL base forward rather than deriving a new one.
        let previous_base_sha = loaded
            .as_ref()
            .and_then(|(events, _)| merge_action::spawn_base_sha(events, &node.id, iter));

        // #489-B: `ensure_sub_worktree`, not `create_sub_worktree`. The bare create
        // replayed `git worktree add -b <branch>` on a branch that already existed
        // and failed with exit 255 on EVERY re-spawn of the same iteration — i.e. on
        // every `restart_node` of an isolated node, 100% of the time.
        match ensure_sub_worktree(
            spawn_ctx.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
            previous_base_sha.as_deref(),
        ) {
            Ok(ensured) => {
                spawn_base_sha = ensured.base_sha;
                reused_sub_worktree = !ensured.created;
                interrupted_git_ops = ensured.entry_state.interrupted_git_ops().to_vec();
                if ensured.created {
                    let inherited = projected
                        .map(|state| state.provisioning_rules.as_slice())
                        .unwrap_or_default();
                    let provisioned = crate::provisioning::provision_node_worktree(
                        spawn_ctx.repo_root,
                        &sub_wt_dir,
                        inherited,
                        &node_provisioning,
                        &pipeline_branch,
                    );
                    if let Err(e) = provisioned {
                        let reason = format!(
                            "provisioning failed for {} in copy/link phase: {e:#}; no node was spawned",
                            node.id
                        );
                        let orphan = (sub_wt_dir.clone(), sub_branch.clone());
                        interrupt_spawn_before_start(
                            deps,
                            spawn_ctx.repo_root,
                            run_id,
                            &node.id,
                            iter,
                            Some(&orphan),
                            &reason,
                        )
                        .await;
                        return SpawnOutcome::Failed { reason };
                    }
                }
                // #489-B: `Some(...)` ONLY when this spawn created the worktree.
                // On a reuse, any later abort in the panic-isolated span would send
                // `interrupt_spawn_before_start` into `reap_orphan_sub_worktree`, and
                // `worktree remove --force` succeeds on a dirty tree — it would
                // destroy exactly the work the restart exists to save. Gated, the
                // abort path appends `NodeInterrupted` and destroys nothing: the Run
                // parks `AwaitingUser` with the work intact (ADR-0049), a reopen/retry
                // re-drives it and the next classification answers `Reusable`. #279's
                // invariant ("an aborted spawn leaves no orphan") is untouched — it
                // only ever covered what the spawn itself created.
                if ensured.created {
                    orphan_to_reap = Some((sub_wt_dir.clone(), sub_branch));
                }
            }
            Err(e) => {
                // #498 / ADR-0050 §1: a surviving `pdo/sub-*` branch (or another
                // git collision) made `worktree add -b` fail. Before résilience
                // this only `error!`-logged and returned `Failed` — no event, so
                // the run stayed frozen `running` and only journalctl knew why.
                // Now it names the node in a `NodeInterrupted`, parking the run
                // `AwaitingUser`; on the next reopen/retry `ensure_sub_worktree`
                // reaps the survivor (Recyclable) and the spawn succeeds (FP #3).
                let reason = format!("failed to ensure sub-worktree for {}: {e:#}", node.id);
                interrupt_spawn_before_start(
                    deps,
                    spawn_ctx.repo_root,
                    run_id,
                    &node.id,
                    iter,
                    None,
                    &reason,
                )
                .await;
                return SpawnOutcome::Failed { reason };
            }
        }
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

    // #550/#347/#424: the model and effort come from the harness resolved above
    // (post node → instance precedence, post empty-string collapse), read from the
    // winning harness's entry — NOT the raw NodeDef. The `NodeStarted` payload
    // records these resolved values (what the flags really carried, which the
    // resume path reads back). `__manager__` / `__merge_resolver__` are infra
    // sessions with no NodeDef and stay at the account default — they don't route
    // through `spawn_node`.
    let resolved_model = resolved_harness.as_ref().and_then(|r| r.model.clone());
    let resolved_effort = resolved_harness.as_ref().and_then(|r| r.effort.clone());
    // #473/#550: pin a session id only for a harness that can honour it
    // (`claude`); a harness that cannot (`opencode`: a fresh id errors) gets
    // `None` and is attributed by working dir. Recorded on `NodeStarted` so the
    // resume path and the sweep read it back; a fresh id per spawn means a
    // `restart_node` of the same iteration gets its own transcript.
    let session_id: Option<String> = harness_descriptor
        .as_ref()
        .filter(|d| d.pins_session_id())
        .map(|_| uuid::Uuid::new_v4().to_string());

    // Panic/cancellation-isolated spawn window (#279). Everything from here to
    // the `NodeStarted` append can panic (`build_full_prompt`, image discovery,
    // input resolution) or — when this runs in-request inside `node_done` — be
    // dropped if the completing client disconnects (hyper drops the in-flight
    // future at an `.await`). Before #279 either left the freshly-created
    // sub-worktree orphaned with NO `NodeStarted`, wedging the run `running`
    // forever: no live node, no error, nothing logged. It slips past every
    // recovery path — `advance_run` is event-triggered, the stale detector only
    // inspects live tmux sessions, and `reconcile_run_level_stall` saw the node
    // as "ready, about to be driven". Run the window under `catch_unwind` so a
    // panic becomes a LOUD failure (reap the orphan, fail the run) instead of a
    // silent stall (ADR-0004 « jamais de stall silencieux »). A dropped
    // (cancelled) future can't be caught here; the periodic detector in
    // `run_stall_reason` (#279 Layer 2) is the backstop for that path — and
    // since #304 (ADR-0023) the `node_done` tail runs DETACHED from the request
    // future, so the completing client's disconnect can no longer cancel this
    // window in the first place.
    // `tokio::sync::Mutex` doesn't poison, so the DB / admission state stay
    // usable after a caught panic (the property `run_isolated` relies on too).
    let span = std::panic::AssertUnwindSafe(async {
        // Debug-only one-shot fault injection (#279): exercises the catch + reap
        // + RunFailed path. Armed via `PDO_DEBUG_PANIC_SPAWN` or
        // `DaemonHandle::arm_spawn_panic`. Checked at the span head so the
        // orphaned worktree already exists and the reap has something to remove.
        if deps
            .panic_on_spawn
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            panic!("PDO_DEBUG_PANIC_SPAWN fault injection (#279)");
        }

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
        // #353: alongside the single-input source iters, resolve the `repeated`
        // pools from the SAME fresh projection — one artifact per COMPLETED
        // source iteration, so a failed iter's artifact is never pooled and no
        // raw `iter-*` glob reaches the agent/script.
        // #465: the secondary repos come from the SAME fresh projection, resolved to
        // absolute snapshot paths for injection (the sub-worktree does not inherit
        // the snapshot files).
        let (source_iters, repeated_iters, secondary_repos) =
            match reload_run_state_with(deps.db, run_id).await {
                Some((_, fresh_state)) => (
                    input_resolution::resolved_source_iters(
                        spawn_ctx.pipeline,
                        &fresh_state,
                        &node.id,
                        iter,
                    ),
                    input_resolution::resolved_repeated_iters(
                        spawn_ctx.pipeline,
                        &fresh_state,
                        &node.id,
                    ),
                    prompt_augmenter::secondary_repo_contexts(
                        spawn_ctx.repo_root,
                        run_id,
                        &fresh_state.target_repos,
                    ),
                ),
                None => (HashMap::new(), HashMap::new(), Vec::new()),
            };

        // Precompute whether the Start prompt carries content so `build_preamble`
        // stays pure (#274). Gate on `!prompt_required` (the only branch that
        // consults it), NOT on the edge-based `is_entry_node` — that would regress
        // the `task`-port fallback (a node with no incoming edge still reads from
        // `_input`). On a genuine I/O error, fail toward "prompt present" and log:
        // a false negative would silently discard the run's actual brief.
        let start_prompt_present = if spawn_ctx.pipeline.prompt_required {
            false // value is never consulted for prompt-required pipelines — skip the read
        } else {
            match prompt_augmenter::read_start_prompt_present(spawn_ctx.artifacts_dir) {
                Ok(present) => present,
                Err(e) => {
                    warn!(
                        "entry-node input read failed (run {run_id} node {} iter {iter}): {e}; \
                         assuming a prompt is present",
                        node.id
                    );
                    true // fail toward "prompt present" — never tell the agent "no prompt" on an I/O error
                }
            }
        };

        // #599 AC1 (ADR-0049): the partial output an interrupted attempt at THIS
        // node left on disk for this iteration. On a same-iter re-spawn
        // (restart-with-artifacts) it is never wiped, so it is surfaced to the
        // fresh agent as input to build on. Empty on a first spawn.
        let partial_outputs =
            prompt_augmenter::surviving_partial_outputs(node, spawn_ctx.artifacts_dir, iter);

        let aug_ctx = prompt_augmenter::AugmentContext {
            pipeline: spawn_ctx.pipeline,
            node,
            run_id,
            iter,
            artifacts_dir: spawn_ctx.artifacts_dir,
            variables: spawn_ctx.resolved_vars,
            // #447: same single resolver as the manager preamble and
            // `PDO_DAEMON_URL` — `run_sandboxed` is already projected above for the
            // spawn precondition. Defensive here: no node preamble consumes
            // `daemon_url` yet (see the note in `node_primitives::start_node`).
            daemon_url: &crate::sandbox_container::daemon_url(deps.port, run_sandboxed),
            foreach_context,
            source_worktree_dir: has_sub_worktree.then_some(working_dir.as_path()),
            // #654: the same section, from the other isolation. This site only
            // ever builds a preamble for a node that spawns a session, so the
            // two are exhaustive and mutually exclusive.
            shared_worktree_dir: (!has_sub_worktree).then_some(working_dir.as_path()),
            input_images,
            start_prompt_present,
            source_iters,
            repeated_iters,
            secondary_repos,
            // #516: both computed above (outside this span), borrowed read-only
            // here so `build_preamble` can route the interrupted-git-op notice. The
            // `Vec` is moved into `SpawnOutcome::Spawned` only after this span is
            // awaited, so the borrow is already released — see G7 in the plan.
            reused_sub_worktree,
            interrupted_git_ops: &interrupted_git_ops,
            partial_outputs: &partial_outputs,
        };

        let full_prompt = prompt_augmenter::build_full_prompt(&aug_ctx, &role_prompt);

        // A `script` node (#248 / ADR-0017) runs the author's bash instead of
        // Claude. Compute its I/O env catalogue and pre-create its output dirs
        // here (inside the panic-isolated span, next to `aug_ctx`) and hand the
        // env back to the spawn below — a script can't read the prose preamble.
        let script_env = if node.node_type == pipeline::NodeType::Script {
            prompt_augmenter::precreate_output_dirs(&aug_ctx);
            prompt_augmenter::build_script_env(&aug_ctx)
        } else {
            Vec::new()
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
                "node_type": node.node_type.as_str(),
                // #653/ADR-0060: FREEZE where this NodeRun works. Every later
                // reader — the re-spawn above, the restart probe, the completion
                // path's merge-back decision — asks this event, never the
                // document, so an isolation edit lands on the next launch and
                // never under a live node's feet.
                "isolated_worktree": has_sub_worktree,
                "provisioning": node_provisioning,
                // #424: the launch-time model and effort, **resolved** (post
                // node → instance precedence, post empty-string collapse) — not
                // the raw `NodeDef` values. This is what the resume path reads
                // back to re-pose `--effort`, so it has to be what the flag
                // actually carried. `None` serializes as JSON `null`; a script
                // node records both as `null` since it launches no agent.
                // Recording `model` alongside is deliberate even though nothing
                // reads it yet: the meaning of an effort level depends on the
                // model (supported levels and the default both vary per model),
                // so storing the effort alone would store half a fact.
                "model": resolved_model.as_deref(),
                "effort": resolved_effort.as_deref(),
                // #550/ADR-0046: the harness resolved at spawn, FROZEN here so the
                // resume path re-poses what was launched, never what the YAML or a
                // tier says now (ADR-0007). `null` for a `script` node (no agent).
                "harness": resolved_harness.as_ref().map(|r| r.harness.as_str()),
                // #473: the pinned Claude Code session id the agent launches with
                // (`claude --session-id <uuid>`). The sweep resolves this node's
                // transcript by it (`<uuid>.jsonl`), and the resume path re-enters
                // it (`--resume <uuid>`). `null` for a `script` node (no claude) and
                // for every pre-#473 row (legacy newest-mtime resolution / bare
                // `--continue`), so no migration is needed.
                "session_id": session_id.as_deref(),
                // #503 / ADR-0036: the sub-worktree's base commit — what the
                // merge-back compares the pipeline tip against to decide whether a
                // conflict is the run's own history rewritten by this node. `null`
                // for a node with no sub-worktree, which never merges back.
                "base_sha": spawn_base_sha,
            })),
        };
        // A failed `NodeStarted` append means the reservation was NOT recorded:
        // treat it as a spawn abort (reap + RunFailed) rather than launching a
        // tmux session the run's event log has no record of.
        //
        // Since #485 (ADR-0038) this ordering is also another subsystem's
        // correctness precondition, not just local hygiene: the orphan sweep's
        // "absent from the log ⇒ orphan ⇒ kill" verdict is only sound because no
        // session can exist before its reservation is durably appended. This was
        // the one spawn path that already got it right, and it is why the sweep
        // never killed a scheduler-spawned node once its snapshot was correctly
        // ordered. Do not reorder this for readability.
        append_event_with(deps.db, deps.event_tx, &node_started)
            .await
            .context("failed to append node_started")?;
        Ok::<(String, Vec<(String, String)>), anyhow::Error>((full_prompt, script_env))
    });

    let span_outcome = futures_util::future::FutureExt::catch_unwind(span).await;

    // The reservation (`NodeStarted`) is recorded iff the span returned
    // `Ok(Ok(_))`; either way the admission lock can be released now — on failure
    // nothing was reserved, on success the projected state already counts the
    // session.
    drop(admission_guard);

    let (full_prompt, script_env) = match span_outcome {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            let reason = format!("spawn of node {} aborted before start: {e}", node.id);
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return SpawnOutcome::Failed { reason };
        }
        Err(panic) => {
            let reason = format!(
                "spawn of node {} panicked before start: {}",
                node.id,
                panic_payload_message(panic.as_ref())
            );
            interrupt_spawn_before_start(
                deps,
                spawn_ctx.repo_root,
                run_id,
                &node.id,
                iter,
                orphan_to_reap.as_ref(),
                &reason,
            )
            .await;
            return SpawnOutcome::Failed { reason };
        }
    };

    let session_name = tmux_session_manager::node_session_name(run_id, &node.id, iter);
    let is_script = node.node_type == pipeline::NodeType::Script;
    let tail = if is_script {
        tmux_session_manager::SessionTail::Script {
            timeout_secs: tmux_session_manager::SCRIPT_TIMEOUT_SECS,
            env: &script_env,
        }
    } else {
        // Resolved above the panic span so the `NodeStarted` payload could record
        // the same values the flags carry (#347/#424/#473/#550). A non-`script`
        // node always has a resolved descriptor (the fail-fast returned otherwise).
        let descriptor = harness_descriptor
            .as_ref()
            .expect("a non-script node resolved a harness descriptor");
        tmux_session_manager::SessionTail::Agent {
            harness: descriptor,
            model: resolved_model.as_deref(),
            effort: resolved_effort.as_deref(),
            session_id: session_id.as_deref(),
        }
    };
    // A script node executes the RAW bash body (`role_prompt`), never the
    // augmented prompt — the preamble is prose an agent reads, not runnable bash.
    let spawn_prompt: &str = if is_script {
        &role_prompt
    } else {
        &full_prompt
    };
    // #407: wrap the tail in `docker exec … pdo-sbx-<run>` when sandboxed. The
    // marker MUST equal the session name (the kill path scans `/proc` for it), and
    // the workdir is the node's own working dir.
    let sandbox_wrap = run_sandboxed.then(|| tmux_session_manager::SandboxWrap {
        docker_bin: deps.docker_cmd_override.unwrap_or("docker"),
        uid: crate::sandbox_container::host_uid(),
        gid: crate::sandbox_container::host_gid(),
        marker: &session_name,
        workdir: &working_dir,
    });
    // #433 / ADR-0043: arm the turn-end `Stop` hook only when the operator has
    // opted into turn-end auto-completion (the SAME setting as the daemon sweep,
    // read FRESH — parity with model/effort) and never for a `script` node (bash
    // tail, no `claude`). Resolved here, at the spawn edge, so a `PUT /settings`
    // takes effect on the next node with no daemon restart.
    let inject_hook = !is_script && stored_autocomplete_turn_end(deps.db).await;
    // #613/ADR-0051 (AC #7): the setting is on but this harness has no end-of-turn
    // substrate ⇒ it will not auto-complete. Say the absence once rather than let
    // the setting be a silent no-op. `claude` (and a `script` node, which never
    // arms the hook) say nothing.
    if inject_hook {
        if let Some(r) = &resolved_harness {
            warn_turn_end_unsupported_once(run_id, &r.harness);
        }
    }
    if let Err(e) = tmux_session_manager::spawn(
        &session_name,
        spawn_prompt,
        &working_dir,
        run_id,
        &node.id,
        iter,
        deps.port,
        deps.tmux_cmd_override,
        tail,
        sandbox_wrap.as_ref(),
        inject_hook,
    ) {
        // #508: the tmux spawn itself failed *after* `NodeStarted` is durable. It
        // used to be swallowed (`error!` + fall through), leaving the node
        // projected `Running` with NO session — the liveness sweep then rewrote it
        // `Failed` ~30s later with a false `session_died` cause, and `restart_node`
        // answered a lying `200 {spawned}`. Fail loud right here instead: append
        // `NodeFailed` (legal now that the iteration is `Running`) → reap only what
        // this spawn created → `RunFailed`, then return `Failed`. This lands BEFORE
        // the `NodeAwaitingUser` block below, so a session that never launched can
        // never collect a phantom `NodeAwaitingUser`. (ADR-0037 §1/§3.)
        let reason = format!(
            "failed to spawn tmux session {session_name} for node {}: {e}",
            node.id
        );
        interrupt_spawn_after_start(
            deps,
            spawn_ctx.repo_root,
            run_id,
            &node.id,
            iter,
            orphan_to_reap.as_ref(),
            &reason,
        )
        .await;
        return SpawnOutcome::Failed { reason };
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
        if let Err(e) = append_event_with(deps.db, deps.event_tx, &awaiting).await {
            error!("failed to append node_awaiting_user: {e}");
        }
    }

    SpawnOutcome::Spawned {
        reused_sub_worktree,
        base_sha: spawn_base_sha,
        interrupted_git_ops,
    }
}

/// Interrupt a run when a node spawn aborts *before* `NodeStarted` is appended
/// (#279 / #498, ADR-0050 §1). Reaps any orphaned sub-worktree + branch the
/// spawn created, then appends a visible cause naming the node.
///
/// Since résilience (ADR-0049) the cause is `NodeInterrupted`, **not**
/// `RunFailed`: a spawn abort is an infra incident, not a business failure, so
/// the runtime never terminalises the run. The guard's `validate_interrupt`
/// admits a `NodeInterrupted` even for an iteration that never opened a
/// `NodeStarted` row, so the projection materialises the node `Interrupted` and
/// [`finalize`](crate::event_log) parks the run `AwaitingUser` with this reason
/// — visible, recoverable, never frozen `running` (the #498 trap). This is why
/// the pre-résilience code had to reach for the un-guarded `RunFailed`: a
/// `NodeFailed` on a never-started node was a guard no-op.
async fn interrupt_spawn_before_start(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    orphan: Option<&(PathBuf, String)>,
    reason: &str,
) {
    error!("Run {run_id}: node {node_id} spawn aborted before NodeStarted — {reason}");
    if let Some((sub_worktree_dir, sub_branch)) = orphan {
        reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
    }
    let interrupted = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeInterrupted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        // #601: prefix the machine slug so the node reason is `<code>: <prose>`
        // and [`finalize`](crate::event_log) derives `awaiting_reason_code`
        // (`spawn_aborted`). `source: "spawn"` is retained for existing readers.
        payload: Some(serde_json::json!({
            "reason": format!("spawn_aborted: {reason}"),
            "reason_code": "spawn_aborted",
            "source": "spawn",
        })),
    };
    if let Err(e) = append_event_with(deps.db, deps.event_tx, &interrupted).await {
        error!("Run {run_id}: failed to append NodeInterrupted after spawn abort: {e}");
    }
}

/// Interrupt a run when a node spawn aborts *after* `NodeStarted` is appended
/// (#508). The iteration is already `Running`, so a `NodeInterrupted` is a legal
/// transition and IS appended: it moves the *node* to `Interrupted` (non
/// terminal, ADR-0049), closing the window where the liveness sweep would
/// rewrite it `Failed` with a false `session_died` cause and where `GET …/pane`
/// / boot_recovery could re-drive a `Running` node that has no session. The run
/// then parks `AwaitingUser` (derived in [`finalize`](crate::event_log)), never
/// `RunFailed`. The reap is gated on `orphan` (Some iff THIS spawn created the
/// sub-worktree — a reuse must destroy nothing; ADR-0037 §6). The order
/// (node-interrupt → reap) mirrors the #488 "terminal event first, reap second"
/// convention.
async fn interrupt_spawn_after_start(
    deps: SpawnDeps<'_>,
    repo_root: &std::path::Path,
    run_id: &str,
    node_id: &str,
    iter: i64,
    orphan: Option<&(PathBuf, String)>,
    reason: &str,
) {
    error!("Run {run_id}: node {node_id} spawn failed after NodeStarted — {reason}");

    let interrupted = event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind: event_log::EventKind::NodeInterrupted,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        // #601: machine slug prefix, as in `interrupt_spawn_before_start`.
        payload: Some(serde_json::json!({
            "reason": format!("spawn_aborted: {reason}"),
            "reason_code": "spawn_aborted",
            "source": "spawn",
        })),
    };
    if let Err(e) = append_event_with(deps.db, deps.event_tx, &interrupted).await {
        error!("Run {run_id}: failed to append NodeInterrupted after spawn failure: {e}");
    }

    // Reap ONLY what this spawn created (gated; ADR-0037 §6 — a reuse loses
    // nothing).
    if let Some((sub_worktree_dir, sub_branch)) = orphan {
        reap_orphan_sub_worktree(repo_root, sub_worktree_dir, sub_branch);
    }
}
