//! What a Run cost and changed: its lines-of-code delta and its estimated
//! or reported cost, per harness.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lines-of-code delta for a Run, derived live from `git diff --numstat` of the
/// run branch against its fork point (issue #100). Live-only: it is **not**
/// snapshotted into the event log (J2), so once the run branch is cleaned up it
/// becomes uncomputable and the field is dropped (`None` → UI shows "—").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocStat {
    pub insertions: u64,
    pub deletions: u64,
    pub files_changed: u64,
}

/// Estimated USD cost of a Run (#272), aggregated from the per-message token
/// `usage` in each session's Claude Code transcript × a public price table (see
/// [`crate::run_cost`]). An **estimate**, not an invoice: public list prices,
/// unpriced/new models contribute $0. Derived on read, never persisted — mirrors
/// [`LocStat`]. `None` when no transcript dir is found (UI shows "—");
/// `Some { usd: 0.0, .. }` when dirs exist but carry no priced tokens.
///
/// `PartialEq` (not `Eq`) because `usd` is `f64` — compare with a tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CostStat {
    pub usd: f64,
    /// True when ≥1 session used a model absent from the price table: its tokens
    /// are excluded, so `usd` is a **lower bound**. Drives the UI "(lower bound)"
    /// affordance. Derived: `partial ⟺ !unpriced_models.is_empty()` (#425 AC#4).
    pub partial: bool,
    /// The family keys (de-dated) of every model no tier could price — the
    /// offenders `partial` used to hide (#425). Sorted and de-duplicated. Lets the
    /// UI name *which* model was excluded instead of the anonymous "an unpriced
    /// model": that anonymity is exactly how `claude-fable-5` — the priciest model
    /// — stayed invisible on `/stats/cost` for weeks. Empty ⟺ `partial == false`.
    #[serde(default)]
    pub unpriced_models: Vec<String>,
    /// The harnesses this Run launched a node on that have **no cost source**
    /// capability (#553, ADR-0045). Non-empty ⇒ the Run's cost is not honestly
    /// summable — a harness like `opencode` writes its cost where PDO does not read
    /// (its own SQLite), so adding `claude`'s real dollars to that harness's
    /// invisible $0 would be a silent under-count. So the surface shows **"—" with
    /// a reason naming these harnesses**, never a `$0`, never a mute `partial`
    /// (same "name what is missing" vein as `unpriced_models`). Sorted, deduped.
    /// Empty on every all-`claude` Run, so `usd`/`partial` mean exactly what they
    /// meant before this field existed.
    #[serde(default)]
    pub uncosted_harnesses: Vec<String>,
    /// The Run's cost **ventilated by harness** (#615, ADR-0052 §3). A mixed Run's
    /// total stays summable in dollars, but it *says* itself per harness — "X via
    /// `copilot`, Y via `claude`" — because the two halves have neither the same
    /// nature (reported vs derived) nor the same precision. One entry per harness
    /// that contributed a cost, in name order. Empty on a Run with no costable
    /// session (so `usd` is 0 and there is nothing to ventilate), and — for
    /// backward compatibility — absent on a pre-#615 serialized `CostStat`, which
    /// the surfaces render exactly as they did before (a single derived figure).
    #[serde(default)]
    pub by_harness: Vec<HarnessCost>,
}

/// Which of the two legitimate cost forms a per-harness slice is (ADR-0052 §1) —
/// so a surface can say *only under a derived figure* that it is an estimate from
/// Claude Code transcripts, and never under a reported one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum CostForm {
    /// PDO re-derived it from tokens × the price table (`claude`). An estimate.
    Derived,
    /// The harness counted it and PDO converted by a published constant
    /// (`copilot`). Not re-derived from tokens (ADR-0052 §2).
    Reported,
}

/// One harness's slice of a Run's cost (#615, ADR-0052 §3). Additive with the
/// others in dollars (`CostStat.usd` is their sum), but tagged with its `form` so
/// the surface never mislabels a reported figure as a Claude-Code estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HarnessCost {
    /// The harness name (`claude`, `copilot`).
    pub harness: String,
    /// This harness's cost in USD.
    pub usd: f64,
    /// Derived or reported (drives the honesty label).
    pub form: CostForm,
    /// True when this (derived) slice excluded an unpriced model — a lower bound.
    /// Always `false` for a reported slice (it never consults the price table, so
    /// it can never be a lower bound). Empty `unpriced_models` ⟺ `!partial`.
    #[serde(default)]
    pub partial: bool,
    /// The unpriced model family keys of this (derived) slice; always empty for a
    /// reported one.
    #[serde(default)]
    pub unpriced_models: Vec<String>,
    /// True when this **reported** slice is already in dollars — a conversion
    /// constant of 1.0 (`pi`'s `usage.cost.total`, #707, ADR-0052 §2 amended) — so
    /// the surface shows it **without `~`**. `false` for a derived slice (an
    /// estimate) and for a reported slice converted from another billing unit
    /// (`copilot`'s nano-AIU).
    #[serde(default)]
    pub reported_in_usd: bool,
}

/// The **Coût de boucle** of one loop region (CONTEXT.md): the attributed cost
/// of its member nodes' executions (sub-agents included) since the loop's last
/// entry — the same fold as the Run total and the node costs (ADR-0058), so the
/// three figures never diverge. Unlike [`crate::domain::state::NodeCost`], an
/// unreadable member keeps the known part and sets `partial`; `usd` is `None`
/// only when no member execution has a readable cost (the surfaces' `—`, never
/// `$0`). Derived on read, never persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoopCost {
    pub usd: Option<f64>,
    /// `~`: false only when every readable contribution is exact reported dollars.
    pub estimated: bool,
    /// Lower bound (`†`): an unreadable member execution or an unpriced model.
    pub partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub unpriced_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub unavailable_reasons: Vec<String>,
    pub executions: i64,
    pub readable_executions: i64,
    /// Each member's share over the same window, in the region's `members`
    /// order; a member with no execution in the window yet is omitted.
    #[serde(default)]
    pub members: Vec<LoopMemberCost>,
    /// The region's effective **Plafond de coût** in dollars: its
    /// `max_cost_usd` resolved against the Run's variables (a `$var` is fixed
    /// at launch, which only the daemon knows). `None` = uncapped. Lets the
    /// header say `~$1.20 / $5` and warn at 80 %.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cap_usd: Option<f64>,
}

/// One member's share of a [`LoopCost`], over the same window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoopMemberCost {
    pub node_id: String,
    pub usd: Option<f64>,
    /// `~` for this member's own share (false when exact reported dollars).
    pub estimated: bool,
    pub partial: bool,
    pub executions: i64,
}
