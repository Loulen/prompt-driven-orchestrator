//! What a `POST /runs` asks for: the request, the one repository line of a
//! multi-repo create, the attachments, and the write boundary's refusal of an
//! unknown field.

use std::collections::HashMap;

use axum::http::StatusCode;
use pdo_workspace::domain::branches::FetchError;
use pdo_workspace::domain::provisioning;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::state::SandboxMode;

#[derive(Deserialize)]
pub struct CreateRunRequest {
    pub pipeline: String,
    pub input: String,
    #[serde(default)]
    pub variables: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub pipeline_id: Option<String>,
    #[serde(default)]
    pub target_repo: Option<String>,
    /// The Projet the Run is launched in (ADR-0087): recorded in `RunStarted`
    /// and never recomputed. Omitted/`null`/blank: the parent Run's for a Run
    /// created from a node session, else none. A named Projet must exist, and
    /// a gesture's primary repository must be one of its members.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Secondary repos to associate with this Run (ADR-0042). An omitted field
    /// means "mono-repo". When present, `target_repos[0]` is the **primary** and
    /// must agree with `target_repo`; `[1..]` are the secondaries. An absent list
    /// with a `target_repo` set is normalised to `[{target_repo}]` at the create
    /// chokepoint, so downstream always sees `[0]` = primary.
    #[serde(default)]
    pub target_repos: Vec<TargetRepoInput>,
    #[serde(default)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Provenance: the id of the Trigger that created this Run, if any. Set by
    /// the trigger scheduler; absent for manual runs.
    #[serde(default)]
    pub triggered_by: Option<String>,
    /// Isolation mode for this Run. Don't collapse to `SandboxMode`: `None` must
    /// stay distinguishable from an explicit `"off"` (omitted defers to the
    /// trigger/instance default; explicit `off` may not be overridden upward) —
    /// see [`pdo_sandbox::domain::mode::effective_sandbox`]. `Runs::start_run` resolves it once
    /// against the fresh instance default.
    #[serde(default)]
    pub sandbox: Option<SandboxMode>,
    /// The agentic harness this Run chooses (ADR-0046) — the `run` tier of
    /// `nœud → Run → Projet → instance → plancher (claude)`. Free text
    /// (ADR-0045: PDO never validates a harness name; an unknown one fails fast
    /// at spawn, naming it). Omitted/`null`/blank means the Run names no harness.
    /// Frozen into `RunStarted` at the create chokepoint; a pinned node ignores
    /// it. A Trigger fire folds its stored harness in here, so "Run now" and a
    /// cron tick produce the same one.
    #[serde(default)]
    pub harness: Option<String>,
    /// The Run tier of the agentic-profile union (ADR-0057) — the counterpart of
    /// [`Self::harness`], as the exclusive union (`Inherit`/`Profile`/`Custom`)
    /// atomically supplying harness, model and effort. `None`/`Inherit` ⇒ the Run
    /// states no choice, so `harness` above still decides. `Some(Profile |
    /// Custom)` wins outright at this tier and never merges with `harness`.
    #[serde(default)]
    pub agent_choice: Option<pdo_harness::agent_choice::AgentChoice>,
    /// The Run tier of the **skills** selection (#669, ADR-0062): ids (+ label)
    /// added to every node of this Run, on top of the instance and Projet tiers.
    /// Frozen into `RunStarted` at the create chokepoint (only when non-empty). A
    /// Trigger fire folds its stored list in here, like `harness`.
    #[serde(default)]
    pub skills: Vec<pdo_skill::domain::selection::SkillRef>,
    /// Whether the manager auto-names this Run. Don't collapse to `bool`: `None`
    /// (omitted) must resolve by the presence of `name`, so a supplied `name` with
    /// no flag keeps the name; `Some(b)` is an explicit choice that wins.
    /// Resolved at the create chokepoint against the FRESH instance default — see
    /// [`crate::application::node::prompt::default_auto_name_with`].
    #[serde(default)]
    pub auto_name: Option<bool>,
    /// The Run's `auto_fail` preference (ADR-0049) — the `run` tier of `node →
    /// Run → Projet → instance`. `None` means the Run states no preference, so a
    /// node `pdo fail` resolves through the project / instance tiers. Frozen into
    /// `RunStarted` at the create chokepoint.
    #[serde(default)]
    pub auto_fail: Option<bool>,
    /// Run-level provisioning rules, composed and frozen with Instance + Project.
    #[serde(default)]
    pub provisioning: provisioning::ProvisioningRules,
    /// The data the Run carries for the extensions (API 0.9): extension id →
    /// a JSON value the core never interprets. An id no installed extension
    /// has is a `400`, like an unknown field; the extension checks its value
    /// before anything is created (`validate_create`) and receives it when it
    /// stamps the Run, before the first Node. Frozen into `RunStarted` as
    /// `extensions`, only when non-empty.
    #[serde(default)]
    pub extensions: pdo_kernel::attributes::ExtensionData,
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
    pub source_fetch_error: Option<FetchError>,
    /// The Actor of the Run's `run` × `create` (API 0.8): the caller of `POST
    /// /runs`, or the Trigger of a fire. `#[serde(skip)]` like
    /// `source_fetch_error`: the daemon decides it, never a body. Frozen into
    /// `RunStarted` as `created_by`.
    #[serde(skip)]
    pub created_by: Option<pdo_kernel::Actor>,
    /// The last editor of the Trigger that fires this Run, frozen into
    /// `RunStarted` as `trigger_editor`. `None` on every other path.
    #[serde(skip)]
    pub trigger_editor: Option<pdo_kernel::Actor>,
    /// Where the Run comes from, when an extension creates it with a
    /// Provenance (`Host::create_run`), already checked. `#[serde(skip)]`
    /// like `created_by`: no `POST /runs` body sets it. Frozen into
    /// `RunStarted` as `provenance`; a retry keeps it.
    #[serde(skip)]
    pub provenance: Option<crate::domain::provenance::RunProvenance>,
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
pub const CREATE_RUN_FIELDS: &[&str] = &[
    "pipeline",
    "input",
    "variables",
    "pipeline_id",
    "target_repo",
    "project_id",
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
    "extensions",
];

/// Reject a `POST /runs` body carrying an unknown top-level key. A non-object
/// body is left for the typed deserialize to reject — its `invalid JSON` message
/// is already actionable.
pub fn reject_unknown_create_run_fields(
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
pub struct TargetRepoInput {
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    #[ts(optional = nullable)]
    pub read_only: bool,
}

pub fn is_false(b: &bool) -> bool {
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
pub struct PatchRunReposRequest {
    #[serde(default)]
    pub add: Vec<TargetRepoInput>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// The answer of a created Run.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct CreateRunResponse {
    pub run_id: String,
}

/// One attachment of a `POST /runs` multipart body (#779): an image (the
/// `images` field, image extension required) or any other file (the `files`
/// field). Both land in the child's `_input/` beside `output.md`; the entry
/// node sees them under `## Input Images` / `## Input Files`.
pub struct InputFile {
    pub filename: String,
    pub data: Vec<u8>,
    /// Classified by EXTENSION, whatever field carried the file — so a `.png`
    /// sent under `files` is still discovered as an image at spawn.
    pub is_image: bool,
}
