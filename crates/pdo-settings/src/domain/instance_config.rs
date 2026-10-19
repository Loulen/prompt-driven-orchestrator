//! The instance settings as they are stored: one singleton row, one column per
//! instance default of every context (ADR-0015). Every column is NULLABLE and
//! the seed row is all-NULL: a NULL stored value means "unset", and the
//! context that owns the knob falls through to its env var, then its built-in
//! default — the `stored → env → default` precedence. Keeping the columns
//! NULLABLE (rather than `NOT NULL DEFAULT 20`) is deliberate: a non-null
//! default would make the stored tier always win, so the env would never be
//! consulted.
//!
//! The settings context keeps this stored tier and nothing else: the
//! precedence itself lives with each owner (`pdo_run::adapters::env`,
//! `pdo_session::adapters::env`, `pdo_trigger::adapters::env`, …).

use pdo_skill::domain::selection::SkillRef;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The persisted singleton config row. Each field is `None` when unset (the
/// resolver then falls through to env → default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// Stored global session cap, or `None` when unset.
    pub session_cap: Option<i64>,
    /// Stored tmux reaper TTL in seconds, or `None` when unset.
    pub reaper_ttl_secs: Option<i64>,
    /// Stored idle TTL of the shared library assistant in seconds, or `None` when
    /// unset (#594, ADR-0051). Its own knob, not a reuse of
    /// [`Self::reaper_ttl_secs`]: that one bounds a *completed node's* session
    /// (default 1 h, an operator may want to read it later), this one bounds an
    /// assistant nobody is attached to and nobody is editing against (default
    /// 120 s). The resolver
    /// ([`pdo_session::adapters::env::libassist_idle_ttl_with`]) owns the
    /// precedence.
    pub libassist_idle_ttl_secs: Option<i64>,
    /// Stored Trigger guard timeout in **seconds**, or `None` when unset. Note
    /// the env seam ([`pdo_trigger::adapters::env::GUARD_TIMEOUT_MS_OVERRIDE_ENV`]) is
    /// in milliseconds; the stored value is seconds (ADR-0015).
    pub guard_timeout_secs: Option<i64>,
    /// Stored instance-wide default Claude model, or `None` when unset (#347).
    /// A work node with no `model:` override launches with this model; `None`
    /// falls through to the env seam ([`pdo_kernel::env::DEFAULT_MODEL_ENV`])
    /// then the account default (no `--model`). Free-text pass-through, no enum
    /// (ADR-0001): an invalid id fails loud in `claude`.
    pub default_model: Option<String>,
    /// Stored instance-wide default sandbox mode (`"off"` | `"full"` | `"minimal"`), or `None`
    /// when unset (#410), which falls through to the env seam (`PDO_DEFAULT_SANDBOX`) then the
    /// built-in default (`off`, the byte-identical host path). `Some("")` is the clear sentinel
    /// — [`update`] normalises it to SQL `NULL` so it never wins precedence. The resolver
    /// ([`pdo_sandbox::adapters::env::default_sandbox_with`]) owns the precedence; the
    /// create-run chokepoint then feeds it through [`pdo_sandbox::domain::mode::effective_sandbox`].
    pub default_sandbox: Option<String>,
    /// Stored turn-end auto-completion flag as `0`/`1`, or `None` when unset (#469).
    /// `None` falls through to the env seam
    /// ([`pdo_run::application::stale::detector::AUTOCOMPLETE_TURN_END_ENV`]) then the built-in
    /// default (`false`). The resolver
    /// ([`pdo_run::application::stale::detector::autocomplete_turn_end_with`]) owns the precedence.
    ///
    /// `Option<i64>` and not `Option<bool>`: every other column on this table is a
    /// nullable scalar and `NULL` is what makes the `stored → env → default`
    /// fall-through work. A `bool` column would have to choose between `NOT NULL
    /// DEFAULT 0` (the stored tier then always wins, shadowing the env) and being
    /// nullable anyway.
    pub autocomplete_turn_end: Option<i64>,
    /// Stored instance-wide default for auto-naming a Run as `0`/`1`, or `None` when
    /// unset (#338). `None` falls through to the env seam
    /// ([`pdo_run::application::node::prompt::DEFAULT_AUTO_NAME_ENV`]) then the built-in default
    /// (`true`, the pre-#338 behaviour). The resolver
    /// ([`pdo_run::application::node::prompt::default_auto_name_with`]) owns the precedence; the
    /// create-run chokepoint reads it FRESH at the edge (never cached at boot) and only
    /// when the request neither supplies an explicit `auto_name` flag nor a `name`.
    ///
    /// `Option<i64>` and not `Option<bool>` for the same reason as
    /// [`Self::autocomplete_turn_end`]: `NULL` is what makes the `stored → env → default`
    /// fall-through work; a stored `0` is a decision that beats the env.
    pub default_auto_name: Option<i64>,
    /// Stored instance-wide default **harness** (`claude`, `opencode`), or `None`
    /// when unset (#550, ADR-0046 amending ADR-0015). `None` falls through to the
    /// env seam ([`pdo_kernel::env::DEFAULT_HARNESS_ENV`]) then the
    /// `claude` floor. Free-text pass-through, no enum (ADR-0001). `Some("")` is
    /// the clear sentinel — [`update`] normalises it to SQL `NULL`.
    pub default_harness: Option<String>,
    /// Stored instance-wide default **model per harness** (#550, ADR-0046): a map
    /// `harness → model`, e.g. `{"claude":"opus"}`. The single `default_model`
    /// became per-harness because a model slug means nothing outside its harness.
    /// Empty map ⇒ no per-harness default (for `claude` the legacy
    /// [`pdo_kernel::env::DEFAULT_MODEL_ENV`] still contributes at the
    /// spawn seam). Persisted as a JSON object in the `default_harness_model`
    /// column (the Triggers' `variables` idiom).
    #[serde(default)]
    pub default_harness_model: std::collections::BTreeMap<String, String>,
    /// Stored instance-wide `auto_fail` default as `0`/`1`, or `None` when unset
    /// (ADR-0049). The **global** (coarsest) tier of the `node → Run → Projet →
    /// instance` precedence resolved by `pdo_run::domain::auto_fail`.
    /// Checked, an agent's `pdo fail` terminalises the run directly to `Failed`;
    /// unset/`0`, it parks the run `AwaitingUser` for a human to confirm. `None`
    /// falls through to the env seam ([`pdo_run::adapters::env::AUTO_FAIL_ENV`]) then the built-in default
    /// (`false`).
    ///
    /// `Option<i64>` and not `Option<bool>` for the same reason as
    /// [`Self::autocomplete_turn_end`]: `NULL` makes the `stored → env → default`
    /// fall-through work; a stored `0` is a decision that beats the env.
    pub auto_fail: Option<i64>,
    /// Stored `update_check` flag as `0`/`1`, or `None` when unset (#697): may the
    /// daemon ask the release source for the latest published version? `None`
    /// falls through to the env seam ([`pdo_platform::domain::update_check::UPDATE_CHECK_ENV`])
    /// then the built-in default (`true`). A stored `0` is a decision that beats
    /// the env — same `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`].
    pub update_check: Option<i64>,
    /// Stored `review_agent_can_resolve` flag as `0`/`1`, or `None` when unset
    /// (#751, ADR-0067 §4): may an agent's `pdo review reply --resolved` resolve
    /// the comment directly? `None` falls through to the env seam
    /// (`pdo_review::adapters::env::AGENT_CAN_RESOLVE_ENV`) then the
    /// built-in default (`false`: the reply is a proposal the human decides on).
    /// Same `0`-not-`NULL` discipline as [`Self::default_auto_name`].
    pub review_agent_can_resolve: Option<i64>,
    /// The Instance tier of the agentic-profile union (#563, ADR-0057) — the
    /// **coarsest** tier `pdo_harness::agent_choice::resolve` consults, just above the
    /// reserved Default profile floor. `None` (unset) preserves the pre-#563
    /// legacy precedence exactly: [`Self::default_harness`] /
    /// [`Self::default_harness_model`] still apply. `Some(Profile | Custom)`
    /// wins outright over them at this tier (never merges). Persisted as a
    /// JSON object in a nullable TEXT column, same idiom as
    /// [`Self::default_harness_model`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Instance tier of the **skills** selection (#669, ADR-0062): the
    /// baseline every node of every Run receives, unioned with the Projet, Run
    /// and Node tiers by `skill_selection::resolve`. Stored as a JSON list in a
    /// nullable TEXT column; `NULL` ⇒ empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<SkillRef>,
    /// Stored manager auto-start flag as `0`/`1`, or `None` when unset (manager
    /// on demand). `None` falls through to the env seam
    /// ([`pdo_run::adapters::env::MANAGER_ENABLED_ENV`]) then the built-in default (`false` — a Run
    /// starts managerless and the user starts one from the Manager tab when they
    /// want it). The resolver ([`pdo_run::adapters::env::resolve_manager_enabled`]) owns the precedence;
    /// the create-run chokepoint reads it FRESH at the edge and it gates **only
    /// the automatic spawn** — a manual start from the Manager tab is always
    /// available, which is what keeps the setting honest ("default off", not
    /// "forbidden").
    ///
    /// `Option<i64>` and not `Option<bool>` for the same reason as
    /// [`Self::autocomplete_turn_end`]: `NULL` is what makes the
    /// `stored → env → default` fall-through work; a stored `0` is a decision
    /// that beats the env.
    pub manager_enabled: Option<i64>,
    /// Stored **agent-profile name** the manager is pinned to, or `None` when
    /// unset (« Follow the Run »): the manager mirrors the Run's harness
    /// (`Run → instance → floor` via `pdo_harness::agent_choice::resolve_infra`) and lands on
    /// the default agent profile for model/effort. A stored name **pins** the
    /// manager to that profile's harness · model · effort, overriding the Run
    /// tier — the Run tier always resolves, so a pin that merely replaced the
    /// floor would almost never fire. `Some("")` is the clear sentinel —
    /// [`update`] normalises it to SQL `NULL`. A stored name whose profile was
    /// later deleted (or renamed) falls back to « Follow the Run » at resolve
    /// time, never a silent rewrite of the stored value (the #432 tombstone
    /// treatment; `GET /settings` names the dangle in the UI).
    pub manager_profile: Option<String>,
    /// Stored per-run attachment budget in MB (#779), or `None` when unset
    /// (`stored → env PDO_MAX_ATTACHMENTS_MB → default 50`). The SAME value
    /// bounds the `POST /runs` multipart body and is disclosed to the « New
    /// run » modal through `GET /settings`, so the client-side gate and the
    /// daemon's refusal cannot disagree.
    pub max_attachments_mb: Option<i64>,
    /// Stored OTLP export switch as `0`/`1`, or `None` when unset (ADR-0090).
    /// `None` falls through to the env seam
    /// ([`pdo_telemetry::adapters::env::OTLP_ENV`]) then the built-in default
    /// (`false`: nothing leaves the instance). Same `0`-not-`NULL` discipline
    /// as [`Self::autocomplete_turn_end`].
    pub telemetry_otlp: Option<i64>,
    /// Stored address of the OTLP collector, or `None` when unset (env
    /// [`pdo_telemetry::adapters::env::OTLP_ENDPOINT_ENV`], then none).
    pub telemetry_otlp_endpoint: Option<String>,
    /// Stored headers of every export (an API key): `name → value`, persisted
    /// as a JSON object. The values are secrets: `GET /settings` names the
    /// headers, never their values.
    #[serde(default)]
    pub telemetry_otlp_headers: std::collections::BTreeMap<String, String>,
    /// Stored `/metrics` switch as `0`/`1`, or `None` when unset (ADR-0090):
    /// env [`pdo_telemetry::adapters::env::PROMETHEUS_ENV`], then off.
    pub telemetry_prometheus: Option<i64>,
    /// Stored « include the Actor » as `0`/`1`, or `None` when unset
    /// (ADR-0090 §4): env [`pdo_telemetry::adapters::env::INCLUDE_ACTOR_ENV`],
    /// then off.
    pub telemetry_include_actor: Option<i64>,
    /// RFC3339-millis UTC timestamp of the last write (or the seed).
    pub updated_at: String,
}

/// A partial edit. A `None` field leaves the stored value untouched.
///
/// The three numeric knobs are a set-only MVP: there is no path to clear a
/// stored value back to unset (a richer double-`Option` form was considered and
/// deferred — ADR-0015). `default_model` is the first *clear-capable* column
/// (#347): it stays a plain `Option<String>`, but `Some("")` is the clear
/// sentinel — [`update`] normalises an empty string to SQL `NULL` (a `Some("")`
/// must never persist, or it would win precedence and shadow env/account
/// default). This keeps the double-`Option` still deferred while giving the UI
/// a "Default" (unset) affordance.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export, optional_fields = nullable, rename = "UpdateSettingsRequest")]
pub struct UpdateInstanceConfig {
    pub session_cap: Option<i64>,
    pub reaper_ttl_secs: Option<i64>,
    /// Set the shared library assistant's idle TTL in seconds (#594). Set-only,
    /// like the other numeric knobs.
    pub libassist_idle_ttl_secs: Option<i64>,
    pub guard_timeout_secs: Option<i64>,
    pub default_model: Option<String>,
    /// Set the instance-wide default sandbox mode (#410). `Some("")` clears it back to
    /// unset (same `""`-sentinel as `default_model`); `None` leaves it untouched.
    pub default_sandbox: Option<String>,
    /// Set turn-end auto-completion (#469): `Some(true)` stores `1`, `Some(false)`
    /// stores `0`, `None` leaves it untouched.
    ///
    /// A `bool` on the wire (the UI has a checkbox, not a number) but a set-only
    /// knob like the numeric ones: there is no path back to *unset*, so a user who
    /// wants the env tier to decide again clears the column out of band. Storing
    /// `0` rather than `NULL` for "off" is deliberate — unticking the box must
    /// override a `PDO_AUTOCOMPLETE_TURN_END=1`, which a `NULL` would not.
    pub autocomplete_turn_end: Option<bool>,
    /// Set the instance-wide default for auto-naming Runs (#338): `Some(true)` stores
    /// `1`, `Some(false)` stores `0`, `None` leaves it untouched.
    ///
    /// Same set-only, `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`]:
    /// storing `0` for "off" is what lets unticking the box override a
    /// `PDO_DEFAULT_AUTO_NAME=1`, which a `NULL` fall-through would not.
    pub default_auto_name: Option<bool>,
    /// Set the instance-wide default harness (#550). `Some("")` clears it back to
    /// unset (same `""`-sentinel as `default_model`); `None` leaves it untouched.
    pub default_harness: Option<String>,
    /// Set the instance-wide per-harness default model map (#550). `Some(map)`
    /// replaces the stored map wholesale; an empty map clears it. `None` leaves it
    /// untouched.
    pub default_harness_model: Option<std::collections::BTreeMap<String, String>>,
    /// Set the instance-wide `auto_fail` default (ADR-0049): `Some(true)` stores
    /// `1`, `Some(false)` stores `0`, `None` leaves it untouched. Same set-only,
    /// `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`].
    pub auto_fail: Option<bool>,
    /// Set the `update_check` flag (#697): `Some(true)` stores `1`, `Some(false)`
    /// stores `0`, `None` leaves it untouched.
    pub update_check: Option<bool>,
    /// Set the `review_agent_can_resolve` flag (#751): `Some(true)` stores `1`,
    /// `Some(false)` stores `0`, `None` leaves it untouched. Same set-only,
    /// `0`-not-`NULL` discipline as [`Self::default_auto_name`].
    pub review_agent_can_resolve: Option<bool>,
    /// Set the Instance tier of the agentic-profile union (#563): `Some(None)`
    /// clears it back to unset (legacy `default_harness`/`default_harness_model`
    /// decide again); `Some(Some(choice))` sets it; `None` leaves it untouched.
    /// Double-`Option`, mirroring [`pdo_trigger::domain::trigger::UpdateTrigger::harness`]
    /// — this column, unlike the set-only numeric knobs, needs a genuine "clear"
    /// path back to legacy precedence.
    pub agent_choice: Option<Option<pdo_harness::agent_choice::AgentChoice>>,
    /// Replace the Instance tier's skills selection wholesale (#669): `Some(list)`
    /// stores it (an empty list clears to `NULL`); `None` leaves it untouched.
    /// A flat `Option<Vec>` rather than a double-`Option`: an empty selection IS
    /// the clear, there is no third state.
    pub skills: Option<Vec<SkillRef>>,
    /// Set the manager auto-start flag (manager on demand): `Some(true)` stores
    /// `1`, `Some(false)` stores `0`, `None` leaves it untouched. Same set-only,
    /// `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`]: unticking
    /// must persist a stored `0` that beats a `PDO_MANAGER_ENABLED=1`, which a
    /// `NULL` fall-through would not.
    pub manager_enabled: Option<bool>,
    /// Set the manager profile pin (an agent-profile NAME): `Some("")` clears it
    /// back to unset (« Follow the Run », same `""`-sentinel as
    /// `default_model`); `None` leaves it untouched.
    pub manager_profile: Option<String>,
    /// Set the per-run attachment budget in MB (#779). Set-only like the other
    /// numeric knobs; `PUT /settings` refuses `< 1`.
    pub max_attachments_mb: Option<i64>,
    /// Set the OTLP export switch (ADR-0090): `Some(true)` stores `1`,
    /// `Some(false)` stores `0`, `None` leaves it untouched.
    pub telemetry_otlp: Option<bool>,
    /// Set the collector's address; `Some("")` clears it back to unset.
    pub telemetry_otlp_endpoint: Option<String>,
    /// Replace the export headers wholesale. A header sent without a value
    /// keeps the value stored under its name: the page never has it. `None`
    /// leaves the headers untouched.
    pub telemetry_otlp_headers: Option<Vec<pdo_telemetry::domain::setting::OtlpHeaderEdit>>,
    /// Set the `/metrics` switch; `None` leaves it untouched.
    pub telemetry_prometheus: Option<bool>,
    /// Set « include the Actor »; `None` leaves it untouched.
    pub telemetry_include_actor: Option<bool>,
}

impl UpdateInstanceConfig {
    /// Whether the edit touches the telemetry export, which then reloads.
    pub fn touches_telemetry(&self) -> bool {
        self.telemetry_otlp.is_some()
            || self.telemetry_otlp_endpoint.is_some()
            || self.telemetry_otlp_headers.is_some()
            || self.telemetry_prometheus.is_some()
            || self.telemetry_include_actor.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.session_cap.is_none()
            && self.reaper_ttl_secs.is_none()
            && self.libassist_idle_ttl_secs.is_none()
            && self.guard_timeout_secs.is_none()
            && self.default_model.is_none()
            && self.default_sandbox.is_none()
            && self.autocomplete_turn_end.is_none()
            && self.default_auto_name.is_none()
            && self.default_harness.is_none()
            && self.default_harness_model.is_none()
            && self.auto_fail.is_none()
            && self.update_check.is_none()
            && self.review_agent_can_resolve.is_none()
            && self.agent_choice.is_none()
            && self.skills.is_none()
            && self.manager_enabled.is_none()
            && self.manager_profile.is_none()
            && self.max_attachments_mb.is_none()
            && !self.touches_telemetry()
    }
}
