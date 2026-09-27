//! One transcript line's token usage, its list-price cost, and the fold of a
//! transcript into a Run-level aggregate and per-model subtotals. Pure: the
//! lines come in as text, the prices as a table.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::domain::prices::{strip_date_suffix, PriceTable};
use crate::domain::state::CostStat;

/// The four cache buckets are disjoint from `input`/`output` — CC's
/// `input_tokens` already excludes cache tokens, so they sum without subtraction.
#[derive(Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create_5m: u64,
    pub cache_create_1h: u64,
}

pub struct Line {
    pub message_id: Option<String>,
    pub request_id: Option<String>,
    pub model: String,
    pub usage: Usage,
}

/// The 5-term ccusage formula. `in_p`/`out_p` are per-MTok list prices; the
/// three cache rates are derived from `in_p`.
pub fn line_cost(u: &Usage, in_p: f64, out_p: f64) -> f64 {
    (u.input as f64 * in_p
        + u.output as f64 * out_p
        + u.cache_create_5m as f64 * in_p * 1.25
        + u.cache_create_1h as f64 * in_p * 2.0
        + u.cache_read as f64 * in_p * 0.1)
        / 1_000_000.0
}

/// Tolerant: a torn/invalid JSON line is skipped, never `?`-propagated. Only
/// `assistant` lines with a real (non-`<synthetic>`, non-error, non-zero) usage
/// carry cost.
pub fn parse_line(raw: &str) -> Option<Line> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    if v.get("isApiErrorMessage").and_then(|b| b.as_bool()) == Some(true) {
        return None;
    }
    let msg = v.get("message")?;
    let model = msg.get("model").and_then(|m| m.as_str())?.to_string();
    if model == "<synthetic>" {
        return None;
    }
    let u = msg.get("usage")?;
    let field = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let input = field("input_tokens");
    let output = field("output_tokens");
    let cache_read = field("cache_read_input_tokens");
    // Prefer the nested 5m/1h split; else drop the flat total into the 5m bucket
    // (ccusage's fallback for transcripts without the split).
    let (cache_create_5m, cache_create_1h) = match u.get("cache_creation") {
        Some(cc) => (
            cc.get("ephemeral_5m_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            cc.get("ephemeral_1h_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
        ),
        None => (field("cache_creation_input_tokens"), 0),
    };
    let usage = Usage {
        input,
        output,
        cache_read,
        cache_create_5m,
        cache_create_1h,
    };
    // All-zero usage carries no cost and would needlessly occupy a dedup slot.
    if input == 0 && output == 0 && cache_read == 0 && cache_create_5m == 0 && cache_create_1h == 0
    {
        return None;
    }
    Some(Line {
        message_id: msg.get("id").and_then(|x| x.as_str()).map(String::from),
        request_id: v
            .get("requestId")
            .and_then(|x| x.as_str())
            .map(String::from),
        model,
        usage,
    })
}

/// Dedup by `(message.id, requestId)`, keeping the first. Lines without a
/// `message.id` are always counted (no key to dedup on).
///
/// Unknown model ids are **de-dated** before collection, so a dated id and its
/// family name one offender, not two — the same key a human would add to
/// `models.yaml`. `partial` is **derived** from that set, so no caller can flip
/// one without the other. `<synthetic>` is priced $0 by the table (never
/// `None`), so it never lands here.
/// it `†`, with the family remembered per id just like the run-level set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelUsage {
    pub usd: f64,
    pub unpriced: BTreeSet<String>,
}

/// [`aggregate_with_seen`] plus the per-model ventilation the « By model » axis
/// folds (ADR-0065). The run-level [`CostStat`] is byte-identical to before —
/// the axis is additive, the totals never move.
pub fn aggregate_by_model(
    lines: impl Iterator<Item = Line>,
    prices: &PriceTable,
    seen: &mut HashSet<(String, Option<String>)>,
) -> (CostStat, BTreeMap<String, ModelUsage>) {
    let mut usd = 0.0;
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();
    for l in lines {
        if let Some(id) = &l.message_id {
            if !seen.insert((id.clone(), l.request_id.clone())) {
                continue; // duplicate: replay on resume/compaction
            }
        }
        let entry = by_model.entry(l.model.clone()).or_default();
        match prices.price_for(&l.model) {
            Some(p) => {
                let line_usd = line_cost(&l.usage, p.input, p.output);
                usd += line_usd;
                entry.usd += line_usd;
            }
            // Unknown real model → $0 + named in the lower-bound signal (#425).
            None => {
                unpriced.insert(strip_date_suffix(&l.model).to_string());
                entry
                    .unpriced
                    .insert(strip_date_suffix(&l.model).to_string());
            }
        }
    }
    let unpriced_models: Vec<String> = unpriced.into_iter().collect();
    let stat = CostStat {
        usd,
        partial: !unpriced_models.is_empty(),
        unpriced_models,
        // The aggregate is transcript-derived — i.e. it already ran because the
        // Run's harness HAS a cost source. Absent-cost-source harnesses are handled
        // by the attributed fold, which returns a "—"-with-reason `CostStat`.
        uncosted_harnesses: Vec::new(),
        // The claude-derived aggregate carries no ventilation of its own; the
        // by-harness breakdown is assembled by the attributed fold, which pairs
        // this derived slice with `copilot`'s reported one (#615).
        by_harness: Vec::new(),
    };
    (stat, by_model)
}
