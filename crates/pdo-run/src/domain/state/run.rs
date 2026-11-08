//! A Run, as the projection folds its event log.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::*;
use crate::domain::event::{EdgeInfo, NodeDefInfo};

/// The TypeScript shape of `GET /runs/{id}`: the projected [`RunState`] plus what
/// `get_run` adds to the JSON on the way out (`has_manager`, and the entity
/// attributes the extensions stamped on the Run when it carries some).
#[derive(TS)]
#[ts(export, rename = "RunState")]
#[allow(dead_code)]
struct RunStateView {
    #[ts(flatten)]
    state: RunState,
    #[ts(optional)]
    has_manager: Option<bool>,
    #[ts(optional)]
    attributes: Option<std::collections::BTreeMap<String, String>>,
}

/// The TypeScript shape of one node of `GET /runs/{id}`: the projected
/// [`NodeState`] plus the provisioning `get_run` copies from the iteration's
/// `NodeStarted` event.
#[derive(TS)]
#[ts(export, rename = "NodeState")]
#[allow(dead_code)]
struct NodeStateView {
    #[ts(flatten)]
    state: NodeState,
    #[ts(optional)]
    provisioning: Option<pdo_workspace::domain::provisioning::ProvisioningRules>,
    #[ts(optional)]
    provisioning_plan: Option<pdo_workspace::domain::provisioning::ProvisioningPlan>,
    #[ts(optional)]
    provisioning_frozen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, rename = "ProjectedRunState")]
pub struct RunState {
    pub run_id: String,
    pub status: RunStatus,
    pub pipeline_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub input: Option<String>,
    /// The values the Run was launched with for the pipeline's variables, as
    /// its `run_started` froze them: only the ones the launch set, a variable
    /// left out runs on its default. Empty for a Run launched without any.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[ts(type = "{ [key in string]: unknown }", optional)]
    pub variables: HashMap<String, serde_json::Value>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Why the Run reached a non-green terminal state — the `reason` of its
    /// `RunFailed` / `RunSkipped` / `RunHalted` (#503).
    ///
    /// Set on the terminal event and cleared by `RunResumed`, mirroring
    /// `NodeState::failure_reason`: a Run being driven again must not still show
    /// last time's cause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub failure_reason: Option<String>,
    /// Why the Run is parked **`AwaitingUser` on an incident** (ADR-0049),
    /// distinct from the interactive `AwaitingUser` wait of a node that asked
    /// its user a question. Set by a `RunInterrupted` event (run-level give-up)
    /// or derived in [`finalize`] from an `Interrupted` node's reason; cleared
    /// by `RunResumed` and by a `reopen_run`/`resume_run` command — a run being
    /// driven again must not still show last time's incident. `None` for an
    /// interactive wait (that node's own reason lives on the node), so the two
    /// awaiting causes stay distinguishable from the run state alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub awaiting_reason: Option<String>,
    /// The **machine** slug companion of [`awaiting_reason`] (#601): a stable
    /// snake_case code (`session_died`, `run_stalled`, `unrouted`,
    /// `region_exhausted`, `spawn_aborted`, `boot_recovery`, a completion-refusal
    /// slug, …) the manager and UI branch on, next to the human sentence — the
    /// same slug+prose contract as a refusal body (ADR-0035). Projected from a
    /// `RunInterrupted` event's `reason_code` payload key, or derived in
    /// [`finalize`] from an `Interrupted` node's `<code>: <prose>` reason prefix
    /// (so historical logs without the explicit key still carry a code). Cleared
    /// with [`awaiting_reason`] on resume / reopen. Every non-advancement thus
    /// carries a reason that is machine-branchable, not only prose to grep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub awaiting_reason_code: Option<String>,
    /// The « Attente des nœuds lancés » (ADR-0086) as the Run's snapshot froze
    /// it: `true` ⇒ the Run waits at `End` for the nodes it launched.
    /// `skip_serializing_if` keeps the wire byte-identical when unset.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub wait_for_started_nodes: bool,
    /// How many started nodes a `Running` Run that waits for them (the
    /// option above) still waits for once past `End` (ADR-0086). Derived in
    /// [`finalize`]; `None` while the Run has not reached `End`, once it no
    /// longer waits, and for a Run without the option — a Run re-opened by a
    /// ▶ simply runs until its node is done. Read by the banner (« waiting for
    /// N started nodes ») and carried by the Résumé de Run (ADR-0084).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waiting_for_started_nodes: Option<u32>,
    #[ts(as = "HashMap<String, NodeStateView>")]
    pub nodes: HashMap<String, NodeState>,
    #[serde(default)]
    pub edges: Vec<EdgeInfo>,
    #[serde(default)]
    pub node_defs: Vec<NodeDefInfo>,
    /// Instance + Project + Run provisioning recipe frozen at Run creation
    /// (ADR-0061). Node rules are appended only when its isolated worktree is cut.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provisioning_rules: Vec<pdo_workspace::domain::provisioning::ScopedRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_node: Option<StartNodeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub end_node: Option<EndNodeInfo>,
    #[serde(default)]
    pub loop_states: HashMap<String, LoopState>,
    #[serde(default)]
    pub foreach_states: HashMap<String, ForEachState>,
    #[serde(default)]
    pub collection_states: HashMap<String, CollectionState>,
    #[serde(default)]
    pub switch_states: HashMap<String, SwitchState>,
    /// Live loop-region cap overrides folded from `set_region_max_iter` commands
    /// (ADR-0011 / #600), keyed by region id. An **absolute** cap (last-write-wins),
    /// consulted by the scheduler in place of the region's declared `max_iter` —
    /// **uniformly for a literal and a `$var` cap** (FP #1), so an operator can grant
    /// a stuck bounded region more laps in flight without editing the YAML or
    /// restarting. Folded from the append-only log, so the raised cap survives a
    /// `reopen_run` re-projection. Empty for every run that never set one (serialized
    /// byte-identically), which is every historical run.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub region_max_iter_overrides: HashMap<String, i64>,
    /// Forced routes folded from `force_route` commands (ADR-0011 / #600), keyed by
    /// **source** — a node id OR a region id — mapping to the target node id. The
    /// scheduler spawns the target (or completes, if it is `End`) when the source
    /// completes, **short-circuiting the source's `when:` edges** — the lever for a
    /// run wedged `unrouted` because a non-`PASS` verdict reaches no live branch
    /// (FP #3). Folded from the log, so the forced exit is **not re-decided** by
    /// `when:` on the next lap or after a reopen (FP #8). Empty (and byte-identical)
    /// for every run that never forced one.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub forced_routes: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_repo: Option<String>,
    /// Secondary repositories associated with this Run in **read-only** (#465,
    /// ADR-0042). Empty for a mono-repo Run (the overwhelming majority, incl. every
    /// historical run) — `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
    /// keeps a mono-repo Run's serialized state byte-identical to the pre-#465 shape
    /// (mirror of `target_repo`'s `Option::is_none` skip); a Run with secondaries
    /// serializes them.
    ///
    /// Each entry is a [`RepoPin`] frozen at Run start; the primary repo is NOT in
    /// here (it stays in `target_repo`). Projected from the `RunStarted` payload key
    /// `target_repos`; an unreadable value degrades to empty with a `warn!`, never a
    /// panic — this applier runs before the transition guard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_repos: Vec<RepoPin>,
    /// Isolation mode for this Run (#403 / #407 / #432) — `off`, or the name of the
    /// staging profile it was launched with. Immutable: set once from the
    /// `RunStarted` payload, never mutated. Absent payload field → `Off` (the
    /// legacy host path), so historical runs and bare-API creates stay off.
    #[serde(default)]
    pub sandbox: SandboxMode,
    /// The staging profile's **resolved entry list, frozen at creation** (#432,
    /// ADR-0031 §6). Written to `RunStarted` as the sibling key `sandbox_entries`,
    /// always together with `sandbox` or not at all, so editing (or deleting) a
    /// profile can never retroactively rewrite what a Run in flight already staged.
    ///
    /// The `Option` is **load-bearing**: `Some(vec![])` is a legitimate resolution
    /// (that IS `minimal`), so a bare `Vec` would confuse "legacy payload, re-resolve"
    /// with "empty profile, stage the floor only".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sandbox_entries: Option<Vec<String>>,
    /// The raw `sandbox_entries` payload value when the key was **present but
    /// unreadable** (#432). Internal to the projection — `skip`ped on the wire — and
    /// exists only so the sandbox prep can fail LOUD with the offending value in its
    /// reason. Silently re-resolving would change what the nodes that already
    /// launched saw, which is exactly what the freeze protects against.
    #[serde(skip)]
    pub sandbox_entries_raw_error: Option<String>,
    /// The staging profile's **env, frozen at creation** (#468, ADR-0031 §8). Written to
    /// `RunStarted` as the sibling key `sandbox_env` — but, unlike `sandbox_entries`, only
    /// when it is **non-empty**. That asymmetry is deliberate: for the entries, absence
    /// ("pre-#432 daemon") and emptiness (`minimal`) had to stay distinguishable; for the
    /// env they describe the same container, so `None` and `Some(empty)` both mean "no
    /// profile env" and historical run JSON stays byte-identical.
    ///
    /// The values are on the wire, like they are in SQLite and in `docker inspect`: the
    /// sandbox is not a security boundary and this is not a secret store (ADR-0031 §8). What
    /// is forbidden is the daemon **log** — see the sandbox context's `env_names`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sandbox_env: Option<std::collections::BTreeMap<String, String>>,
    /// The raw `sandbox_env` payload value when the key was **present but unreadable**
    /// (#468). Same role, and same `skip`, as [`RunState::sandbox_entries_raw_error`]: the
    /// sandbox prep fails LOUD with the offending value rather than silently posing a
    /// different environment than the nodes that already launched saw.
    #[serde(skip)]
    pub sandbox_env_raw_error: Option<String>,
    /// The staging profile's **image source, frozen at creation** (#467, ADR-0031 §9). Third
    /// sibling key of `sandbox_entries` / `sandbox_env`, written by the same `resolve` and — like
    /// the env, unlike the entries — **only when the profile poses one**.
    ///
    /// `None` means "this Run's profile posed no image source", indistinguishable from "a
    /// pre-#467 daemon created this Run": in both cases the **profile default** decides
    /// (`DEFAULT_PROFILE_IMAGE` of the sandbox context), overridable by two env vars read
    /// fresh at each prep. That is the one place the freeze is deliberately *not* total, and
    /// it is safe: a daemon's environment does not change under a running Run.
    ///
    /// What IS frozen is the profile's choice: a Run cannot have its image swapped under it
    /// because someone edited the profile, so two nodes of the same Run can never land in two
    /// different images (ADR-0031 §6).
    ///
    /// Unlike [`SandboxMode`] the value is structured (`{kind, path|ref}`), affordable here
    /// only because the key is new — no historical payload carries it as a bare string, so no
    /// reader has to accept `String | Object` for ever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sandbox_image: Option<pdo_sandbox::domain::image::ProfileImage>,
    /// The raw `sandbox_image` payload value when the key was **present but unreadable** (#467).
    /// Same role, and same `skip`, as its two twins: the sandbox prep fails LOUD with the
    /// offending value rather than silently starting a container in a *different image* than the
    /// nodes that already launched ran in.
    #[serde(skip)]
    pub sandbox_image_raw_error: Option<String>,
    /// One-time image-prep visibility for a sandboxed Run (#410). Additive: `None`
    /// for `off`/historical runs; `pending` while the image is pulled/built at first
    /// use; `ready` once the container is about to run. `status` is never touched —
    /// this drives a banner, not admission/overlap/liveness logic. Survives a daemon
    /// restart by event replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sandbox_prep: Option<SandboxPrepState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_branch: Option<String>,
    /// Why the **pre-cut fetch** of the Trigger fire that created this Run failed
    /// (#804, ADR-0070 §1), frozen from the `RunStarted` payload.
    ///
    /// Set ONLY on a fired Run whose fetch did not happen. Absent on every manual
    /// Run (its form fetched before it opened), on every CLI-created child (it cuts
    /// without fetching, by design — ADR-0070 §3) and on every fire that refreshed
    /// fine. Its presence is **not** a failure signal: the Run started, from the
    /// last state known locally, and the detail says so rather than leaving someone
    /// to wonder why the fork point looks old. Nothing branches on it — it drives a
    /// sentence, never `status`, admission or liveness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_fetch_error: Option<SourceFetchError>,
    /// The commit `pdo/run-<id>` was cut from at Run start — the run's fork point,
    /// FROZEN here from the `RunStarted` payload (#417), same immutability posture as
    /// `source_branch`/`harness`/`RepoPin.sha`. The stable 3-dot base for the LOC stat
    /// and the Run diff, so a shared checkout whose HEAD later wanders can no longer
    /// displace the merge-base and inflate the count. `None` ⇒ a pre-#417 Run (payload
    /// omits the key) → fall back to `source_branch`, then `HEAD`. NB: distinct from the
    /// per-node `NodeStarted.base_sha` (sub-worktree ← pipeline branch, ADR-0036).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fork_sha: Option<String>,
    /// The harness chosen at Run creation (#551, ADR-0046), **frozen** here from the
    /// `RunStarted` payload — the middle tier of the precedence chain
    /// `nœud → Run → Projet → instance → plancher (claude)`. Immutable, exactly like
    /// [`RunState::sandbox`]: set once from the create event, never mutated, so a
    /// pipeline edit or a changed instance default cannot re-decide a Run in flight.
    ///
    /// `None` ⇒ the Run named no harness, so each free node resolves through the
    /// instance default and the floor as before (every historical Run, and any Run
    /// created without an explicit choice — the payload omits the key, keeping it
    /// byte-identical to the pre-#551 shape). A blank/empty stored value collapses to
    /// `None` at the freeze (`Some("")` never persists), so it can never win a tier
    /// (#347). Read at every spawn seam as the `run` tier of [`pdo_harness::resolver`]
    /// and by the infra sessions (Pipeline Manager).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub harness: Option<String>,
    /// The Run tier of the agentic-profile union (#563, ADR-0057), **frozen**
    /// here from the `RunStarted` payload — same immutability as
    /// [`RunState::harness`]: set once at create, never mutated (frozen for
    /// resume, ADR-0057 ¶4). `None` ⇒ the Run named no choice (every historical
    /// Run, and any Run created without one), so [`RunState::harness`] (the
    /// legacy signal) still decides the Run tier. `Some(Profile | Custom)` wins
    /// outright at this tier over `harness` — never merges — and also supplies
    /// model/effort to infra sessions (#563 AC18, amending ADR-0046's
    /// Run-only-harness rule for infra).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Run tier of the **skills** selection (#669, ADR-0062), **frozen** here
    /// from the `RunStarted` payload — set once at create (a fired Run copies its
    /// Trigger's list), never mutated. Read at every node spawn as the `run` slot
    /// of `skill_selection::resolve`. Empty ⇒ the Run adds none (every
    /// historical Run: the payload omits the key).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<pdo_skill::domain::selection::SkillRef>,
    /// The Run's `auto_fail` preference (ADR-0049), **frozen** here from the
    /// `RunStarted` payload — the **run** tier of
    /// [`crate::domain::auto_fail::resolve_auto_fail`] (`node → Run → Projet →
    /// instance`). `None` ⇒ the Run stated no preference (every historical Run,
    /// and any Run created without an explicit choice — the payload omits the
    /// key), so a node `pdo fail` resolves through the project/instance tiers.
    /// Immutable, like [`RunState::harness`]: set once at create, never mutated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto_fail: Option<bool>,
    /// Provenance: the id of the Trigger that created this Run, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub triggered_by: Option<String>,
    /// The Actor of the Run's `run` × `create` (API 0.8): who launched it, or
    /// the Trigger of a fire. Frozen into `RunStarted`; `None` on a Run created
    /// before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_by: Option<pdo_kernel::Actor>,
    /// For a fired Run, the Trigger's last editor when it fired. Frozen with
    /// [`RunState::created_by`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub trigger_editor: Option<pdo_kernel::Actor>,
    /// Provenance: the Run whose node session created this one (ADR-0064).
    /// **Mechanical**: posed by the daemon at create time from the caller's
    /// verified live node session — never declared by the caller, whose body may
    /// not even carry a parent field. `None` on a **root run** — every historical
    /// Run, and any Run created outside a node session. Immutable, like
    /// [`RunState::triggered_by`]: frozen into `RunStarted`, never mutated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_run_id: Option<String>,
    /// The parent Run's node whose session created this Run (ADR-0064) — the key
    /// `GET /runs/{id}/children` groups by. Frozen with
    /// [`RunState::parent_run_id`], same posture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_node_id: Option<String>,
    /// The Projet the Run was launched in (ADR-0087): chosen at launch, or
    /// inherited from the parent Run, frozen into `RunStarted` and never
    /// recomputed. `None` for a Run launched in no Projet. The Projet of
    /// every Operation on the Run, its nodes and sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_id: Option<String>,
    /// The library pipeline id this Run was created from (#377). Written to the
    /// `RunStarted` payload going forward; consumers (aggregated "by pipeline"
    /// stats) fall back to `pipeline_name` when it is absent — historical runs,
    /// bare-API/multipart creates, retries — so grouping survives a rename
    /// (#230). Additive and backward-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pipeline_id: Option<String>,
    /// The version of the pipeline this Run launched (ADR-0088), frozen into
    /// `RunStarted`. `None` on a Run older than the versions, and on one not
    /// launched from the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pipeline_version: Option<pdo_pipeline::domain::version::PipelineVersionRef>,
    /// Cumulative count of `NodeStarted` events for this Run — i.e. how many
    /// Claude Code NodeRun sessions it spawned (issue #100). A **raw** count,
    /// not deduplicated by `(node, iter)`: a legal re-spawn at the same
    /// `(node, iter)` (restart/recovery) counts again, so this is always ≥ the
    /// number of distinct iterations shown. The Pipeline Manager emits no
    /// `NodeStarted`, so it is excluded by construction.
    /// Sent review comments (#750, ADR-0067 §2), in send order, with their
    /// replies and resolution state. Folded from `review_comment_*` events; drafts
    /// never reach here (browser-only). Empty — and byte-identically absent — for
    /// every Run nobody reviewed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_comments: Vec<crate::domain::review_comment::ReviewComment>,
    #[serde(default)]
    pub sessions_spawned: u64,
    /// Lines changed for the Run (issue #100). `None` (not `Some(0)`) when the
    /// run branch is gone (archived/cleaned) — the UI renders "—" vs "0".
    /// Derived on read, never persisted; see [`LocStat`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub loc: Option<LocStat>,
    /// Estimated USD cost for the Run (issue #272). `None` when no Claude Code
    /// transcript dir is found (UI renders "—"). Derived on read, never
    /// persisted; see [`CostStat`]. More durable than `loc`: archival leaves
    /// `~/.claude/projects/` intact, so an archived Run keeps its cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost: Option<CostStat>,
    /// The Coût de l'arbre, read by the detail; absent without descendant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tree_cost: Option<TreeCost>,
}
