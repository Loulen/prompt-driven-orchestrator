//! A Trigger, the Run template a cron schedule fires (CONTEXT.md → *Trigger*),
//! and what its history records.
//!
//! A Trigger is config + mutable scheduling state, so it lives in storage
//! beside the event log rather than as a YAML artifact. This does not violate
//! event-sourcing: the event log remains the source of truth for *Run* state; a
//! Trigger merely *produces* Runs.

use pdo_harness::agent_choice::AgentChoice;
use pdo_kernel::attributes::Attributes;
use pdo_skill::domain::selection::SkillRef;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// serde default for [`Trigger::auto_name`]: a Trigger deserialised from a
/// payload predating the field keeps auto-naming, matching the column default.
fn default_true() -> bool {
    true
}

/// A persisted Trigger row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Trigger {
    pub id: String,
    pub name: String,
    /// Library pipeline id the Trigger fires.
    pub pipeline_id: String,
    /// Pipeline display name (denormalised for list rendering).
    #[serde(default)]
    pub pipeline_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_repo: Option<String>,
    /// Read-only secondary repos to associate with fired Runs (ADR-0042), stored
    /// as raw JSON TEXT (`[{repo, base_branch?}]`) so the Trigger stays
    /// decoupled from the Run's request types. `None`/blank → mono-repo fire. The
    /// create chokepoint re-freezes each secondary's SHA at fire time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_repos: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub input_template: String,
    /// Variable overrides as a JSON object (stored as a JSON string).
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub variables: serde_json::Value,
    /// 5-field cron expression.
    pub cron: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard_command: Option<String>,
    /// `"skip"` (default) or `"allow"`.
    pub overlap_policy: String,
    /// Bounded-`allow` ceiling: max simultaneous live Runs of this Trigger.
    /// `None` = unbounded (also the effective value under the `skip` policy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_concurrent: Option<i64>,
    /// Per-Trigger sandbox mode (`"off"` | `"full"` | `"minimal"`), or `None` to
    /// inherit the instance default. Read at fire time and folded into the create
    /// request's explicit tier; the create chokepoint resolves the precedence
    /// (run → trigger → instance default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sandbox: Option<String>,
    /// The agentic harness fired Runs launch on (ADR-0046), or `None` to inherit
    /// the instance default. There is NO separate Trigger tier in the precedence
    /// chain: a Trigger *is* a Run template, so this is read at fire time and
    /// folded into the create request's `run` choice
    /// (`nœud → Run → Projet → instance`). A cron tick and a "Run now" both route
    /// through the same fire, so they freeze the SAME harness. Free text, no
    /// validation (ADR-0045): an unknown name fails fast at the first node spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub harness: Option<String>,
    /// The Run tier of the agentic-profile union (ADR-0057). Read at fire time
    /// and folded into the create request's `run` choice, exactly like
    /// [`Self::harness`]. `None` ⇒ `harness` (and the tiers below it) decide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_choice: Option<AgentChoice>,
    /// The skills a fired Run carries at its **Run** tier (#669, ADR-0062). Read
    /// at fire time and folded into the create request's `skills`, exactly like
    /// [`Self::harness`] — no separate Trigger tier. Empty ⇒ nothing added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<SkillRef>,
    /// Whether Runs fired from this Trigger are auto-named by the manager. A flat
    /// bool (mirror of [`Self::enabled`]), NOT a nullable inherit: the choice is
    /// frozen at creation from the instance default and read at fire time.
    #[serde(default = "default_true")]
    pub auto_name: bool,
    pub enabled: bool,
    /// The next scheduled fire, as **canonical UTC RFC3339-millis** (`…Z`).
    /// Every writer (create and edit, the scheduler's `set_next_fire`)
    /// stores UTC, so a lexicographic string compare equals a chronological one.
    /// The due query (`TriggerRepository::due`) is nonetheless tz-normalised so a legacy
    /// or stray-offset row can never silently go dormant (#222).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_fire_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_fired_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_outcome: Option<String>,
    /// The entity attributes the extensions stamped on it (CONTEXT.md §
    /// *Extensions*), when the read asked for them; absent when none.
    #[serde(default, skip_serializing_if = "Attributes::is_empty")]
    #[ts(as = "Option<Attributes>", optional)]
    pub attributes: Attributes,
}

/// The fields supplied at creation time; scheduling state is derived/initial.
#[derive(Debug, Clone)]
pub struct NewTrigger {
    pub name: String,
    pub pipeline_id: String,
    pub pipeline_name: String,
    pub target_repo: Option<String>,
    /// Raw JSON TEXT; `None` → mono-repo.
    pub target_repos: Option<String>,
    pub source_branch: Option<String>,
    pub input_template: String,
    pub variables: serde_json::Value,
    pub cron: String,
    pub guard_command: Option<String>,
    pub overlap_policy: String,
    /// `None` = unbounded.
    pub max_concurrent: Option<i64>,
    /// `None` inherits the instance default.
    pub sandbox: Option<String>,
    /// `None` inherits the instance default at fire time.
    pub harness: Option<String>,
    /// `None` inherits the legacy `harness` tier (and below) at fire time.
    pub agent_choice: Option<AgentChoice>,
    /// The Run-tier skills every fired Run carries (#669). Empty ⇒ none.
    pub skills: Vec<SkillRef>,
    /// Seeded from the instance default in the create modal and frozen here.
    pub auto_name: bool,
    /// First scheduled fire, computed by the caller from the cron expression.
    pub next_fire_at: Option<String>,
}

/// One audit row in `trigger_fires`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TriggerFire {
    pub id: i64,
    pub trigger_id: String,
    pub ts: String,
    /// `fired` / `skipped-overlap` / `guard-exit-nonzero` / `guard-error` / `error`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub run_id: Option<String>,
    /// Guard diagnostics on a `guard-exit-nonzero` row. NULL on every other
    /// outcome and on legacy rows; tail-capped to 16 KB each (`domain::guard`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard_stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard_stderr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard_exit_code: Option<i32>,
    /// `"manual"` for a Run-now click, `"cron"` for a scheduler tick. NULL on
    /// legacy rows ≈ cron.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
}

/// What happened on a tick, persisted to the audit table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireRecord {
    pub outcome: String,
    pub reason: Option<String>,
    pub run_id: Option<String>,
    /// Set only on a `guard-exit-nonzero` record.
    pub guard_stdout: Option<String>,
    pub guard_stderr: Option<String>,
    pub guard_exit_code: Option<i32>,
    /// `"manual"` / `"cron"`; `None` writes NULL (≈ cron).
    pub source: Option<String>,
}

/// A partial config edit. `None` leaves the stored value untouched. A nullable
/// field is double-wrapped so the caller can distinguish "leave alone" (`None`)
/// from "set to NULL" (`Some(None)`) — load-bearing: a plain `serde(default)`
/// would make present-`null` indistinguishable from omitted, so the UI could
/// never reset a field back to inheriting.
#[derive(Debug, Clone, Default)]
pub struct UpdateTrigger {
    pub name: Option<String>,
    /// The route validates the target exists. Both `pipeline_id` and the
    /// denormalised `pipeline_name` are updated together, so list rendering
    /// cannot show a stale name.
    pub pipeline_id: Option<String>,
    pub pipeline_name: Option<String>,
    pub target_repo: Option<Option<String>>,
    /// `Some(None)` clears to NULL (mono-repo).
    pub target_repos: Option<Option<String>>,
    pub source_branch: Option<Option<String>>,
    pub input_template: Option<String>,
    pub variables: Option<serde_json::Value>,
    pub cron: Option<String>,
    pub guard_command: Option<Option<String>>,
    pub overlap_policy: Option<String>,
    pub max_concurrent: Option<Option<i64>>,
    /// `Some(None)` clears to NULL = "inherit the instance default".
    pub sandbox: Option<Option<String>>,
    /// `Some(None)` clears to NULL = "inherit the instance default".
    pub harness: Option<Option<String>>,
    /// `Some(None)` clears to NULL = the legacy `harness` tier decides again.
    pub agent_choice: Option<Option<AgentChoice>>,
    /// Replace the Run-tier skills wholesale (#669). A flat `Option<Vec>`: an
    /// empty list IS the clear (stored as NULL), there is no third state.
    pub skills: Option<Vec<SkillRef>>,
    /// A FLAT `Option<bool>`, NOT double-wrapped: there is no "inherit" state to
    /// clear back to; the choice is a plain on/off.
    pub auto_name: Option<bool>,
    pub next_fire_at: Option<Option<String>>,
    /// Folded in here so the enable bit and a forward `next_fire_at` land in ONE
    /// atomic UPDATE (it was a separate `set_enabled` write).
    pub enabled: Option<bool>,
}

impl UpdateTrigger {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.pipeline_id.is_none()
            && self.pipeline_name.is_none()
            && self.target_repo.is_none()
            && self.target_repos.is_none()
            && self.source_branch.is_none()
            && self.input_template.is_none()
            && self.variables.is_none()
            && self.cron.is_none()
            && self.guard_command.is_none()
            && self.overlap_policy.is_none()
            && self.max_concurrent.is_none()
            && self.sandbox.is_none()
            && self.harness.is_none()
            && self.agent_choice.is_none()
            && self.skills.is_none()
            && self.auto_name.is_none()
            && self.next_fire_at.is_none()
            && self.enabled.is_none()
    }
}
