//! The Cost section (#377, ADR-0029): the Runs of the window, their
//! harness-specific contributions folded into period, Pipeline → Node,
//! Project → Pipeline → Node and Model → Effort → Pipeline → Node
//! hierarchies. Aggregated cost is a **sum of lower bounds**: partial runs
//! (an unpriced model) and null-cost runs (no transcript) are counted
//! separately so a bucket is never silently undercounted (ADR-0001 honesty).

pub mod fold;
pub mod metric;
pub mod pairs;
pub mod wire;

use serde::Serialize;
use ts_rs::TS;

use crate::domain::absorption::tallies::AbsorbedMember;
use crate::domain::provenance::StatsProvenance;
use pdo_run::domain::prices::PriceTier;

/// One resolved price row (#528): a family key, the tier that decides it, and the
/// `$/MTok` actually in force. `tier` serializes as `"manual" | "fetched" |
/// "embedded"` (the `PriceTier` `rename_all = "lowercase"`).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, rename = "PriceRow")]
pub struct ResolvedPriceRow {
    pub key: String,
    pub tier: PriceTier,
    /// $/MTok in — the price ACTUALLY applied (the winning tier).
    pub input: f64,
    /// $/MTok out — the price ACTUALLY applied (the winning tier).
    pub output: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsHarnessCost {
    pub harness: String,
    pub usd: Option<f64>,
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub readable: i64,
    pub unknown: i64,
    pub average_usd: Option<f64>,
    /// The **median** cost per readable execution (#811) — the value the Cost
    /// tab shows, `average_usd` staying on the wire beside it. See
    /// [`CostMetricAcc::median_usd`].
    pub median_usd: Option<f64>,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
    /// Where THIS harness's model value in the row was read from (ADR-0065 §1)
    /// — only meaningful on the « By model » rows and the Node leaves' pairs
    /// (#736: the tooltip says, harness by harness, where the value came from);
    /// omitted on the rows that carry no slices (totals, other axes).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provenance: Option<StatsProvenance>,
    /// Same, for the effort half of the row; absent on model-only rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effort_provenance: Option<StatsProvenance>,
    /// The provider the harness's source named for this model (`pi` via
    /// openrouter) — tooltip only, never part of the identity (ADR-0065 §2).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsCostAggregate {
    pub usd: Option<f64>,
    pub average_usd: Option<f64>,
    /// The **median** cost per readable execution (#811) — additive beside
    /// `average_usd`, which stays for the tooltip and for any client that has
    /// not caught up. See [`CostMetricAcc::median_usd`].
    pub median_usd: Option<f64>,
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
    pub readable: i64,
    pub unknown: i64,
    pub unpriced_models: Vec<String>,
    pub missing_reasons: Vec<String>,
    pub harnesses: Vec<StatsHarnessCost>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsCostPeriod {
    pub bucket: String,
    #[serde(flatten)]
    pub aggregate: StatsCostAggregate,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsCostEntity {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub aggregate: StatsCostAggregate,
    pub by_period: Vec<StatsCostPeriod>,
    pub nodes: Vec<StatsCostEntity>,
    /// The Node's cost split into model × effort pairs (ADR-0065) — Node leaves
    /// only, so the drill-down ends there; omitted (never an empty array) on the
    /// other levels.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub models: Vec<StatsModelEffortPair>,
    /// Pipeline, Node and « By model » model rows (#890, #892): the Runs
    /// counted, the most recent one's start, and — on an absorbent — its
    /// absorbed members. Omitted on the other levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub absorbed: Vec<AbsorbedMember>,
}

/// One model × effort pair of a Node leaf (ADR-0065): the same aggregate shape
/// as any cost row, plus the pair identity and where each half was read from.
/// `effort = None` is the "not set" bucket — never merged with a real effort.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsModelEffortPair {
    #[serde(flatten)]
    pub aggregate: StatsCostAggregate,
    pub model: String,
    pub model_provenance: StatsProvenance,
    pub effort: Option<String>,
    pub effort_provenance: Option<StatsProvenance>,
    /// The couple's key (`model|effort`, #906) — what a couple absorption
    /// names; the UI shows `model · effort`, never the key.
    pub key: String,
    pub runs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<String>,
    /// The couples this one absorbs locally, in this Node row (#906).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub absorbed: Vec<AbsorbedMember>,
    /// The raw couples a global absorption (models, efforts) counts here —
    /// the grey, read-only « Global absorption » mark (#906).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(optional = nullable)]
    pub global_absorbed: Vec<AbsorbedMember>,
}

/// One effort level under a model, in the « By model » tree: the entity's `id`
/// is the effort string ("" for not set) and its `name` the effort or
/// "not set".
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsEffortCostEntity {
    #[serde(flatten)]
    pub entity: StatsCostEntity,
    pub effort: Option<String>,
    pub provenance: Option<StatsProvenance>,
    pub pipelines: Vec<StatsCostEntity>,
}

/// One model (verbatim id) of the « By model » axis, with its effort tree:
/// Model → Effort → Pipeline → Node.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsModelCostEntity {
    #[serde(flatten)]
    pub entity: StatsCostEntity,
    pub provenance: StatsProvenance,
    pub efforts: Vec<StatsEffortCostEntity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsProjectCostEntity {
    #[serde(flatten)]
    pub entity: StatsCostEntity,
    pub pipelines: Vec<StatsCostEntity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct StatsCost {
    pub harnesses: Vec<String>,
    pub total: StatsCostAggregate,
    pub by_period: Vec<StatsCostPeriod>,
    pub by_pipeline: Vec<StatsCostEntity>,
    pub by_project: Vec<StatsProjectCostEntity>,
    pub by_model: Vec<StatsModelCostEntity>,
    pub resolved: Vec<ResolvedPriceRow>,
}
