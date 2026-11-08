//! What the rest of PDO asks of a projected Run.

use std::collections::HashMap;

use super::*;

impl RunState {
    /// #588 / ADR-0069: is this Run parked on a **declared wait** — a node's agent
    /// asked its user something (or a refused unreleased completion declared it
    /// for the agent) — rather than on an incident? The prose reason is set in
    /// both cases; only the incident carries a machine slug, and only the
    /// declared wait has an awaiting node with a message. The reopen embedding
    /// asks this so a targeted command never « reopens » a run that is simply
    /// waiting for an answer.
    pub fn is_declared_wait(&self) -> bool {
        self.status == RunStatus::AwaitingUser
            && self.awaiting_reason_code.is_none()
            && self.nodes.values().any(|n| {
                n.status == NodeStatus::AwaitingUser
                    && n.awaiting.as_ref().is_some_and(|a| a.message.is_some())
            })
    }
    pub fn new(run_id: String, pipeline_name: String) -> Self {
        Self {
            run_id,
            status: RunStatus::Running,
            pipeline_name,
            name: None,
            input: None,
            variables: HashMap::new(),
            started_at: None,
            completed_at: None,
            failure_reason: None,
            awaiting_reason: None,
            awaiting_reason_code: None,
            awaiting_region_id: None,
            wait_for_started_nodes: false,
            waiting_for_started_nodes: None,
            nodes: HashMap::new(),
            edges: Vec::new(),
            node_defs: Vec::new(),
            provisioning_rules: Vec::new(),
            start_node: None,
            end_node: None,
            loop_states: HashMap::new(),
            foreach_states: HashMap::new(),
            collection_states: HashMap::new(),
            switch_states: HashMap::new(),
            region_max_iter_overrides: HashMap::new(),
            forced_routes: HashMap::new(),
            target_repo: None,
            target_repos: Vec::new(),
            sandbox: SandboxMode::Off,
            sandbox_entries: None,
            sandbox_entries_raw_error: None,
            sandbox_env: None,
            sandbox_env_raw_error: None,
            sandbox_image: None,
            sandbox_image_raw_error: None,
            sandbox_prep: None,
            source_branch: None,
            source_fetch_error: None,
            fork_sha: None,
            harness: None,
            agent_choice: None,
            skills: Vec::new(),
            auto_fail: None,
            triggered_by: None,
            created_by: None,
            trigger_editor: None,
            parent_run_id: None,
            parent_node_id: None,
            project_id: None,
            pipeline_id: None,
            pipeline_version: None,
            review_comments: Vec::new(),
            sessions_spawned: 0,
            loc: None,
            cost: None,
            loop_costs: HashMap::new(),
        }
    }

    /// Status of `node_id` in this run's projection, if the node exists.
    ///
    /// Borrows (`NodeStatus` is `Clone`-not-`Copy`); use for status-only reads
    /// where the whole [`NodeState`] is not needed.
    pub fn node_status(&self, node_id: &str) -> Option<&NodeStatus> {
        self.nodes.get(node_id).map(|n| &n.status)
    }

    /// The latest **settled-complete** iteration of `node_id`, if any (#210).
    ///
    /// History-max over `Completed`/`Skipped` iterations (failed/stopped iters are
    /// quarantined — their artifacts stay on disk but are never resolvable as
    /// inputs), falling back to the head `iter` when the head status is settled
    /// but no per-iteration history exists (legacy states). This is the single
    /// home for the rule formerly duplicated as a free fn in `input_resolution`.
    ///
    /// `Skipped` counts (#620): the auto-skip writes an empty output precisely so a
    /// downstream resolver finds a concrete artifact, so a pruned producer's lap is
    /// resolvable exactly as it was when a skip projected `Completed`.
    pub fn latest_completed_iter(&self, node_id: &str) -> Option<i64> {
        let node = self.nodes.get(node_id)?;
        let from_history = node
            .iterations
            .iter()
            .filter(|it| it.status.is_settled_complete())
            .map(|it| it.iter)
            .max();
        from_history.or_else(|| node.status.is_settled_complete().then_some(node.iter))
    }

    /// All `Completed` iterations of `node_id`, ascending (#353).
    ///
    /// The set-valued twin of [`latest_completed_iter`], for `repeated`/pooled
    /// inputs that accumulate one artifact per completed lap. Failed/Stopped/
    /// Stale iters are quarantined (their artifacts stay on disk but are never
    /// resolvable as inputs). Falls back to `vec![head_iter]` when the head
    /// status is `Completed` but no per-iteration history exists (legacy
    /// states). Empty when the node has no completed iteration or does not
    /// exist.
    ///
    /// `NodeState.iterations` is already sorted by `iter` and deduplicated by
    /// `(node, iter)` in [`project`], so the history filter yields an ascending,
    /// duplicate-free `Vec` — no `BTreeSet` needed.
    ///
    /// Invariant: `completed_iters(n).last().copied() == latest_completed_iter(n)`.
    /// `latest_completed_iter` is NOT reimplemented on top of this: the spawn
    /// path is hot and calls it per resolution, so it avoids the `Vec` alloc.
    pub fn completed_iters(&self, node_id: &str) -> Vec<i64> {
        let Some(node) = self.nodes.get(node_id) else {
            return Vec::new();
        };
        let from_history: Vec<i64> = node
            .iterations
            .iter()
            .filter(|it| it.status.is_settled_complete())
            .map(|it| it.iter)
            .collect();
        if !from_history.is_empty() {
            return from_history;
        }
        if node.status.is_settled_complete() {
            vec![node.iter]
        } else {
            Vec::new()
        }
    }

    /// True iff `node_ids` is non-empty AND every id resolves to a node that is a
    /// **settled completion** — `Completed` or `Skipped`
    /// ([`NodeStatus::is_settled_complete`]).
    ///
    /// `Skipped` counts (#620): an auto-skipped node discharged its obligation with
    /// an empty output, so a run whose only "unfinished" node is a pruned one MUST
    /// still be able to reach `RunCompleted` — leaving it out would hang the run
    /// forever. The error-ish terminals (`Failed`/`Stopped`/`Stale`/`Interrupted`)
    /// do NOT count — those are owned by the stall / fail-fast / incident paths. A
    /// never-spawned id (no `NodeState`) counts as not-done. An empty set yields
    /// `false`, NOT vacuous-true: a run with no expected nodes is not "all done"
    /// (preserving the original `!is_empty()` guard).
    ///
    /// The authoritative node set is the caller's (`pipeline.nodes` at the
    /// completion/stall sites, the runtime `expected_node_ids` at the
    /// node-done sites) — `RunState` owns neither, so it receives the ids.
    pub fn all_nodes_completed(&self, node_ids: &[String]) -> bool {
        !node_ids.is_empty()
            && node_ids.iter().all(|id| {
                self.node_status(id)
                    .is_some_and(|s| s.is_settled_complete())
            })
    }

    /// Why this Run is **not schedulable yet** because its sandbox is still being
    /// prepared (#445) — `None` when a session may be launched.
    ///
    /// A sandboxed node's tmux window runs `docker exec … pdo-sbx-<run_id> …`. On a
    /// container that does not exist yet, `docker exec` exits 1 in ~30 ms, the window's
    /// command ends, the tmux session disappears, and ~25 s later the stale detector
    /// renders `session_died` — a failure that names tmux while the real fault is the
    /// ordering. The precondition therefore belongs to the *spawn*, not to the callers
    /// that reach it.
    ///
    /// | `sandbox` | `sandbox_prep` | verdict |
    /// |---|---|---|
    /// | `off` | (any) | `None` — the host path never grew a precondition |
    /// | profile | `Ready` | `None` — image + container + staging are guaranteed |
    /// | profile | `Pending` | blocked — the prep task is between its two events |
    /// | profile | `None` | blocked — the prep task has not reached its head event |
    ///
    /// The `None` arm blocks deliberately: `RunStarted` and `SandboxPrepStarted` are
    /// ~100 ms apart, and a read of `<run>/pipeline.yaml` inside that window wakes the
    /// watcher (inotify reports the *first read* of a fresh run dir as a modification).
    /// Blocking is also the fail-safe direction — the cost of a wrong `Some` is one
    /// deferred spawn replayed on `SandboxPrepReady`, the cost of a wrong `None` is a
    /// dead node.
    ///
    /// Pure: projected state in, decision out. The reason is the operator-facing
    /// sentence, so it names the profile (the *why this Run and not that one*).
    pub fn sandbox_spawn_block(&self) -> Option<String> {
        let profile = self.sandbox.profile()?;
        match self.sandbox_prep {
            Some(SandboxPrepState::Ready) => None,
            Some(SandboxPrepState::Pending) => Some(format!(
                "sandbox prep for run {} (profile `{profile}`) is still in progress: \
                 its container is not up, so no session can be launched yet",
                self.run_id
            )),
            None => Some(format!(
                "sandbox prep for run {} (profile `{profile}`) has not started yet: \
                 its container does not exist, so no session can be launched yet",
                self.run_id
            )),
        }
    }

    /// The ids of every **open** (not-`done`) region on this Run, across ALL
    /// region-state kinds (#601). Exhaustive by construction via
    /// [`RegionStateKind`]: an open region of any kind is the Pipeline Manager's
    /// domain, never a fail-fast run-level stall — so `run_stall_reason` can defer
    /// to a *total* predicate here instead of a hand-written per-map disjunction
    /// that a new region kind could silently escape (the #453 class, where each
    /// new region type reopened a frozen run reported `stalled = false`).
    pub fn open_region_ids(&self) -> Vec<String> {
        RegionStateKind::ALL
            .iter()
            .flat_map(|k| k.open_ids(self))
            .collect()
    }

    /// Whether any region of any kind is currently open (#601). See
    /// [`open_region_ids`](RunState::open_region_ids).
    pub fn has_open_region(&self) -> bool {
        !self.open_region_ids().is_empty()
    }
}
