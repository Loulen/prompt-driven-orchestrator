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

/// One depth level of a Run's tree (ADR draft-cout-de-l-arbre): `depth` 1 is
/// the direct children, 2 the grandchildren, and so on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DepthCost {
    pub depth: u32,
    /// How many Runs sit at this depth.
    pub runs: u32,
    /// Their summed cost; `None` when one of them has no cost ("—").
    pub cost: Option<CostStat>,
}

/// The **Coût de l'arbre** of a Run that has at least one descendant (ADR
/// draft-cout-de-l-arbre): its own cost, the **Coût du Run parent** (the Run's
/// `cost`, untouched), plus the own cost of every descendant, archived ones
/// included, summed on read and never persisted. A forgotten descendant has no
/// log left and is out of the tree. A Run without descendant has none: its
/// surfaces stay as they were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TreeCost {
    /// Parent run + every descendant. `None` ("—") when a Run of the tree has
    /// no cost; a lower bound (`partial`) as soon as one of them is.
    pub total: Option<CostStat>,
    /// The Coût du Run parent: the same figure as the Run's `cost`.
    pub parent: Option<CostStat>,
    /// Every descendant summed, « Child runs (N) ». `None` when one of them has
    /// no cost.
    pub children: Option<CostStat>,
    /// N: how many descendants, every depth included.
    pub descendants: u32,
    /// The descendants per depth, shallowest first.
    pub by_depth: Vec<DepthCost>,
    /// How many Runs of the tree (the parent included) have no cost at all:
    /// what a `None` total owes its "—" to.
    pub unavailable_runs: u32,
}
