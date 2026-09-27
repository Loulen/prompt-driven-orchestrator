//! Node spawn primitive: the single, injectable sequence that turns a ready
//! NodeRun into a live tmux session under the global admission cap.
//!
//! Carved out of the lib.rs god-file (#356) next to its callers, mirroring
//! worktree_ops (#276) and run_advance. `spawn_node` takes a narrow
//! `SpawnDeps` (db, event sink, admission lock, panic flag, port, tmux
//! override) instead of the full `Runs`, so its ordering invariants —
//! transition guard (#212), atomic cap check-and-reserve (#213), panic
//! isolation + orphan reaping, and "fail loud as RunFailed" (#279) — are
//! unit-testable without a live daemon. It is a leaf primitive (ADR-0009,
//! Couche 2): it appends events and touches tmux/worktree, and never
//! re-enters the scheduler.

use std::collections::HashMap;

use pdo_pipeline::domain::pipeline;

use crate::Runs;

mod guards;
mod harness;
mod interrupts;
mod launch;
mod reserve;
mod resolve;
mod warnings;
mod worktree;

pub struct SpawnContext<'a> {
    pub pipeline: &'a pipeline::PipelineDef,
    pub run_id: &'a str,
    pub pipeline_path: &'a std::path::Path,
    pub worktree_dir: &'a std::path::Path,
    pub artifacts_dir: &'a std::path::Path,
    pub resolved_vars: &'a HashMap<String, serde_yaml::Value>,
    pub repo_root: &'a std::path::Path,
}

/// Narrow, hand-buildable bundle of the exactly-six side-effects `spawn_node`
/// touches. A struct-of-borrows (NOT a trait): `admission_lock` is a bare
/// `tokio::sync::Mutex`, so the guard it hands out borrows `Runs`'s
/// lifetime — a trait object would fight that borrow. Every field is a `Copy`
/// reference / `u16` / `Option<&str>`, so the whole thing is `Copy` and can be
/// threaded on to `fail_spawn_before_start` without reborrow gymnastics.
///
/// `from_state` is the only production constructor; a unit test builds the same
/// struct out of a `test_state_with_dir` `Runs`, which is the whole point of
/// the seam (#356) — spawn is drivable in-process with fakes.
#[derive(Clone, Copy)]
pub struct SpawnDeps<'a> {
    pub runs: &'a Runs,
    pub admission_lock: &'a tokio::sync::Mutex<()>,
    pub panic_on_spawn: &'a std::sync::atomic::AtomicBool,
    pub port: u16,
    pub tmux: &'a dyn pdo_session::application::ports::Tmux,
    pub tmux_cmd_override: Option<&'a str>,
    /// Per-daemon `docker` binary override for the sandbox wiring (#407), `Copy`
    /// like the rest of the bundle. `None` in production (real `docker`).
    pub docker_cmd_override: Option<&'a str>,
    /// The host home root override (#407 seam), borrowed so the bundle stays
    /// `Copy`. `None` in production ⇒ the real `$HOME`. #553 uses it as the root
    /// under which the **disk descriptor tier** (`~/.pdo/harnesses/`) is read, so a
    /// user-declared harness resolves at spawn — and so a layer-3 test that sets
    /// the override reads descriptors from its own tempdir, never the real home.
    pub home_override: Option<&'a std::path::Path>,
    /// #672: the skill context — the Banque de skills and the delivery of a
    /// node's skills into the tree it works in.
    pub skills: &'a dyn crate::application::ports::NodeSkills,
}

impl<'a> SpawnDeps<'a> {
    /// Project the six spawn side-effects out of the full daemon state.
    pub fn from_state(state: &'a Runs) -> Self {
        Self {
            runs: state,
            admission_lock: &state.admission_lock,
            panic_on_spawn: &state.panic_on_spawn,
            port: state.port,
            tmux: state.tmux(),
            tmux_cmd_override: state.tmux_cmd_override.as_deref(),
            docker_cmd_override: state.docker_cmd_override.as_deref(),
            home_override: state.sandbox_home_override.as_deref(),
            skills: state.skills.as_ref(),
        }
    }
}

/// What actually happened in a `spawn_node` call (ADR-0025 / #327). Every exit
/// path is distinguishable so callers that must tell the truth about a
/// re-scheduling (`re_evaluate_after_command`) can report the real effect
/// instead of assuming success. Callers on fire-and-forget paths simply drop it
/// (intentionally not `#[must_use]`).
#[derive(Debug, Clone)]
pub enum SpawnOutcome {
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
    /// (already live / already completed iteration) — or the spawn turned out to
    /// be the **benign loser** of a duplicate: the same `(node, iter)` already
    /// owns a live tmux session (#785 / ADR-0050 §2). Either way the work is
    /// live or done and there is nothing to retry.
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

/// Everything a spawn resolved and prepared before its reservation.
struct Plan {
    role_prompt: String,
    foreach_context: Option<crate::application::node::prompt::ForEachContext>,
    run_sandboxed: bool,
    wt: worktree::Worktree,
    resolved_harness: Option<pdo_harness::agent_choice::ResolvedCombo>,
    resolved_skills: Option<pdo_skill::domain::selection::ResolvedSkills>,
    skipped_skills: Vec<pdo_skill::domain::delivery::SkippedSkill>,
    resolved_model: Option<String>,
    resolved_effort: Option<String>,
    session_id: Option<String>,
}

/// Spawn one NodeRun (ADR-0009, layer 2): the guards before any side effect,
/// the admission, what the node resolves (harness, skills), where it works,
/// its reservation (`NodeStarted`) and then its session. A leaf: it never
/// re-enters the scheduler.
pub async fn spawn_node(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
) -> SpawnOutcome {
    let run_id = spawn_ctx.run_id;
    let loaded = match guards::guard(deps, spawn_ctx, node, iter).await {
        Ok(loaded) => loaded,
        Err(outcome) => return outcome,
    };
    // #407: whether the Run is sandboxed (immutable, projected from RunStarted). When
    // it is, the tail below is wrapped to run inside `pdo-sbx-<run_id>`. Read from the
    // guard projection — `sandbox` never changes over a Run's life. A `bool` and not
    // the mode (#432): the profile name is owned, and only the off-ness matters here.
    let run_sandboxed = loaded.as_ref().is_some_and(|(_, s)| !s.sandbox.is_off());

    let admission_guard = match guards::admit(deps, run_id, node, iter).await {
        Ok(guard) => guard,
        Err(outcome) => return outcome,
    };

    let canonical_path = pipeline::canonical_prompt_path(spawn_ctx.pipeline_path, &node.id);
    let role_prompt = std::fs::read_to_string(&canonical_path).unwrap_or_default();

    let foreach_context = warnings::find_collection_context(spawn_ctx, &node.id, iter);

    let resolved_harness = resolve::harness(deps, spawn_ctx, node, &loaded).await;
    let resolved_skills = resolve::skills(deps, spawn_ctx, node, &loaded).await;
    let (harness_descriptor, set_env) = match harness::prepare(
        deps,
        spawn_ctx,
        node,
        iter,
        resolved_harness.as_ref(),
        run_sandboxed,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    let wt = match worktree::prepare(deps, spawn_ctx, node, iter, &loaded).await {
        Ok(wt) => wt,
        Err(outcome) => return outcome,
    };
    let skipped_skills = match worktree::deliver_skills(
        deps,
        spawn_ctx,
        node,
        iter,
        &wt.working_dir,
        resolved_skills.as_ref(),
        wt.orphan_to_reap.as_ref(),
    )
    .await
    {
        Ok(skipped) => skipped,
        Err(outcome) => return outcome,
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

    let plan = Plan {
        role_prompt,
        foreach_context,
        run_sandboxed,
        wt,
        resolved_harness,
        resolved_skills,
        skipped_skills,
        resolved_model,
        resolved_effort,
        session_id,
    };
    let (full_prompt, script_env) =
        match reserve::reserve(deps, spawn_ctx, node, iter, &plan, admission_guard).await {
            Ok(reserved) => reserved,
            Err(outcome) => return outcome,
        };
    if let Err(outcome) = launch::launch(
        deps,
        spawn_ctx,
        node,
        iter,
        &plan,
        harness_descriptor.as_ref(),
        &set_env,
        full_prompt,
        script_env,
    )
    .await
    {
        return outcome;
    }

    // #588 / ADR-0069: NO `NodeAwaitingUser` at spawn, interactive or not. A
    // fresh node has the colour of a working node; the wait is declared by its
    // agent (`pdo wait-user`) or by a refused unreleased completion.

    SpawnOutcome::Spawned {
        reused_sub_worktree: plan.wt.reused_sub_worktree,
        base_sha: plan.wt.spawn_base_sha,
        interrupted_git_ops: plan.wt.interrupted_git_ops,
    }
}
