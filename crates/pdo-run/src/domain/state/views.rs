//! The TypeScript shapes of the Run read on the wire: the projected state
//! plus what `get_run` adds to the JSON on the way out. Exported only.

use ts_rs::TS;

use super::{NodeState, RunState};

/// The TypeScript shape of `GET /runs/{id}`: the projected [`RunState`] plus what
/// `get_run` adds to the JSON on the way out (`has_manager`, and the entity
/// attributes the extensions stamped on the Run when it carries some).
#[derive(TS)]
#[ts(export, rename = "RunState")]
#[allow(dead_code)]
pub(super) struct RunStateView {
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
pub(super) struct NodeStateView {
    #[ts(flatten)]
    state: NodeState,
    #[ts(optional)]
    provisioning: Option<pdo_workspace::domain::provisioning::ProvisioningRules>,
    #[ts(optional)]
    provisioning_plan: Option<pdo_workspace::domain::provisioning::ProvisioningPlan>,
    #[ts(optional)]
    provisioning_frozen_at: Option<String>,
}
