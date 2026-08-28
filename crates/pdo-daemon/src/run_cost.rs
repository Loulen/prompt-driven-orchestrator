//! Harness-aware USD cost of a Run, derived on read as common contributions.
//! Claude contributions use per-message token `usage` from each parent session
//! transcript plus its subagents. Copilot contributions preserve the reported
//! cumulative cost from its session journal. Unknown harnesses retain their
//! executions and an absence reason without inventing a zero cost.
//!
//! ## Two forms, ventilated by harness (ADR-0052)
//!
//! `claude`'s transcript path is a **derived** cost; `copilot` counts its own and
//! PDO converts it by a published constant — a **reported** cost, read by session
//! identity, never through the price table (so it can never flag an unpriced
//! model). A harness with no cost source at all (`opencode`) makes only the
//! **total** "—" with a reason: the per-harness slices are still computed and
//! carried alongside the absence, so a mixed Run still says where its known
//! dollars came from (ADR-0052 §3).
//!
//! The price table is **injected** by the caller at the request edge. There is
//! deliberately NO N-1-argument wrapper meaning "the embedded prices": the next
//! call site added would silently ignore both disk tiers, uncatchable by test.
//!
//! The `projects_root` is injected by the caller (the #408 observability seam,
//! [`crate::sandbox_run::transcripts_root`]): `~/.claude/projects/` for an
//! `off`/archived run, the staged home while a sandboxed run is live. This
//! module never reads `$HOME` — one root in, path-math + `std::fs` out.
//!
//! Claude cost is an **estimate, not an invoice**: public list prices, and any
//! model absent from the table contributes $0, flips `partial`, and is named in
//! `unpriced_models`. Derived on read, never persisted (like `LocStat`) — but
//! more durable: archival deletes the run branch (LOC → "—") while leaving
//! `~/.claude/projects/` intact, so an archived run still shows its cost.
//!
//! ## Correctness notes (each verified against real transcripts, ADR-0022)
//! - **Dedup is mandatory.** Claude Code replays assistant messages on
//!   resume/compaction, so the same message is written ~2.3× in a real
//!   transcript. We dedup by `(message.id, requestId)`, keeping the first — the
//!   `usage` is byte-identical within a group, so keep-one is exact (matches
//!   `ccusage`). Without it the number is 2–3× too high.
//! - **Cache tokens don't overlap `input_tokens`.** CC's `input_tokens` excludes
//!   cache tokens, so the four buckets sum without subtraction (matches ccusage).
//! - **Tolerant parsing.** Torn writes (an interleaved-flush `clauclaude-opus-4-8`
//!   was observed) are skipped line-by-line, never `?`-propagated.

use crate::event_log::{CostForm, CostStat, HarnessCost};
use crate::price_table::{strip_date_suffix, PriceTable};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostScope {
    Node,
    Infrastructure,
    Unassigned,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CostContribution {
    pub harness: String,
    pub scope: CostScope,
    pub node_id: Option<String>,
    pub executions: i64,
    pub readable_executions: i64,
    pub usd: Option<f64>,
    pub form: Option<CostForm>,
    pub partial: bool,
    pub unpriced_models: Vec<String>,
    pub unavailable_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RunCostBreakdown {
    pub contributions: Vec<CostContribution>,
    pub cost: Option<CostStat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum FrozenExecutionIdentity {
    Session {
        harness: String,
        session_id: String,
    },
    Legacy {
        node_id: String,
        iter: i64,
        event_id: Option<i64>,
        order: usize,
    },
}

pub(crate) fn frozen_execution_identity(
    harness: &str,
    session_id: Option<&str>,
    node_id: Option<&str>,
    iter: Option<i64>,
    event_id: Option<i64>,
    order: usize,
) -> FrozenExecutionIdentity {
    match session_id.filter(|session_id| !session_id.is_empty()) {
        Some(session_id) => FrozenExecutionIdentity::Session {
            harness: harness.to_string(),
            session_id: session_id.to_string(),
        },
        None => FrozenExecutionIdentity::Legacy {
            node_id: node_id.unwrap_or("(unknown)").to_string(),
            iter: iter.unwrap_or(1),
            event_id,
            order,
        },
    }
}

/// GitHub Copilot bills in AI credits (AIU), one AIU worth one US cent, and its
/// journal reports cumulative spend in nano-AIU. This is Copilot's published
/// billing conversion, not a token-price estimate.
const USD_PER_AIU: f64 = 0.01;
const NANO_PER_AIU: f64 = 1e9;

fn nano_aiu_to_usd(nano_aiu: u64) -> f64 {
    nano_aiu as f64 / NANO_PER_AIU * USD_PER_AIU
}

fn reported_cost_usd(journal: &str) -> Option<f64> {
    let max_nano_aiu = journal
        .lines()
        .filter_map(|raw| serde_json::from_str::<serde_json::Value>(raw.trim()).ok())
        .filter(|value| {
            matches!(
                value.get("type").and_then(|kind| kind.as_str()),
                Some("session.usage_checkpoint") | Some("session.shutdown")
            )
        })
        .filter_map(|value| {
            value
                .get("data")
                .and_then(|data| data.get("totalNanoAiu"))
                .and_then(|total| total.as_u64())
        })
        .max()?;
    Some(nano_aiu_to_usd(max_nano_aiu))
}

fn collect_jsonl_file(path: &Path, out: &mut Vec<Line>) {
    if let Ok(content) = std::fs::read_to_string(path) {
        out.extend(content.lines().filter_map(parse_line));
    }
}

fn claude_execution_cost(
    claude_root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    prices: &PriceTable,
) -> Option<CostStat> {
    let project = claude_root.join(cc_project_dirname(working_dir));
    if !project.is_dir() {
        return None;
    }
    let mut lines = Vec::new();
    match session_id {
        Some(sid) => {
            let main = project.join(format!("{sid}.jsonl"));
            let subagents = project.join(sid).join("subagents");
            if !main.is_file() && !subagents.is_dir() {
                return None;
            }
            collect_jsonl_file(&main, &mut lines);
            collect_jsonl_recursive(&subagents, &mut lines);
        }
        None => collect_jsonl_recursive(&project, &mut lines),
    }
    Some(aggregate(lines.into_iter(), prices))
}

pub(crate) fn compute_run_cost_breakdown(
    events: &[crate::event_log::Event],
    claude_root: &Path,
    copilot_root: &Path,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> RunCostBreakdown {
    let node_types: BTreeMap<String, String> = events
        .iter()
        .find(|event| event.kind == crate::event_log::EventKind::RunStarted)
        .and_then(|event| event.payload.as_ref())
        .and_then(|payload| payload.get("node_defs"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|node| {
            Some((
                node.get("id")?.as_str()?.to_string(),
                node.get("node_type")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let node_type = |event: &crate::event_log::Event| {
        event
            .payload
            .as_ref()
            .and_then(|payload| payload.get("node_type"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| {
                event
                    .node_id
                    .as_ref()
                    .and_then(|id| node_types.get(id))
                    .cloned()
            })
    };
    let mut contributions = Vec::new();
    let mut seen_executions = HashSet::new();
    for (order, event) in events.iter().enumerate() {
        if event.kind != crate::event_log::EventKind::NodeStarted {
            continue;
        }
        let payload = event.payload.as_ref();
        if node_type(event).as_deref() == Some("script") {
            continue;
        }
        let harness = payload
            .and_then(|p| p.get("harness"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::harness_registry::CLAUDE);
        let session_id = payload
            .and_then(|p| p.get("session_id"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let identity = frozen_execution_identity(
            harness,
            session_id,
            event.node_id.as_deref(),
            event.iter,
            event.id,
            order,
        );
        if !seen_executions.insert(identity) {
            continue;
        }
        let (cost, form, reason) = match harness {
            crate::harness_registry::COPILOT => {
                let usd = session_id.and_then(|sid| {
                    let journal =
                        std::fs::read_to_string(copilot_root.join(sid).join("events.jsonl"))
                            .ok()?;
                    reported_cost_usd(&journal)
                });
                (
                    usd.map(|usd| CostStat {
                        usd,
                        partial: false,
                        unpriced_models: Vec::new(),
                        uncosted_harnesses: Vec::new(),
                        by_harness: Vec::new(),
                    }),
                    Some(CostForm::Reported),
                    if session_id.is_none() {
                        Some("missing session identity")
                    } else {
                        Some("no reported cost reading")
                    },
                )
            }
            crate::harness_registry::CLAUDE => {
                let node_type = node_type(event);
                let working_dir = if node_type.as_deref() == Some("code-mutating") {
                    crate::worktree_ops::sub_worktree_path(
                        repo_root,
                        run_id,
                        event.node_id.as_deref().unwrap_or(""),
                        event.iter.unwrap_or(1),
                    )
                } else {
                    crate::worktree_ops::worktree_dir_for_run(repo_root, run_id)
                };
                (
                    if session_id.is_some() || node_type.as_deref() == Some("code-mutating") {
                        claude_execution_cost(claude_root, &working_dir, session_id, prices)
                    } else {
                        None
                    },
                    Some(CostForm::Derived),
                    Some("no attributable Claude transcript"),
                )
            }
            _ => (None, None, Some("harness has no cost source")),
        };
        let usd = cost.as_ref().map(|c| c.usd);
        contributions.push(CostContribution {
            harness: harness.to_string(),
            scope: CostScope::Node,
            node_id: event.node_id.clone(),
            executions: 1,
            readable_executions: i64::from(usd.is_some()),
            usd,
            form: usd.and(form),
            partial: cost.as_ref().is_some_and(|c| c.partial),
            unpriced_models: cost
                .as_ref()
                .map(|c| c.unpriced_models.clone())
                .unwrap_or_default(),
            unavailable_reasons: if cost.is_none() {
                reason.map(str::to_string).into_iter().collect()
            } else {
                Vec::new()
            },
        });
    }

    let run_harness = events
        .iter()
        .find(|event| event.kind == crate::event_log::EventKind::RunStarted)
        .and_then(|event| event.payload.as_ref())
        .and_then(|payload| payload.get("harness"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(crate::harness_registry::CLAUDE);
    let infrastructure_executions = i64::from(
        events
            .iter()
            .any(|event| event.kind == crate::event_log::EventKind::RunStarted),
    ) + events
        .iter()
        .filter(|event| event.kind == crate::event_log::EventKind::MergeResolverStarted)
        .count() as i64;
    if infrastructure_executions > 0 {
        let ambiguous_shared_claude = events.iter().any(|event| {
            if event.kind != crate::event_log::EventKind::NodeStarted {
                return false;
            }
            let payload = event.payload.as_ref();
            let node_type = node_type(event);
            let harness = payload
                .and_then(|p| p.get("harness"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(crate::harness_registry::CLAUDE);
            let has_session = payload
                .and_then(|p| p.get("session_id"))
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.is_empty());
            node_type.as_deref() != Some("script")
                && node_type.as_deref() != Some("code-mutating")
                && harness == crate::harness_registry::CLAUDE
                && !has_session
        });
        let full_claude = (run_harness == crate::harness_registry::CLAUDE)
            .then(|| compute_run_cost(claude_root, repo_root, run_id, prices))
            .flatten();
        let attributed_claude_usd: f64 = contributions
            .iter()
            .filter(|c| c.harness == crate::harness_registry::CLAUDE && c.scope == CostScope::Node)
            .filter_map(|c| c.usd)
            .sum();
        let residual = full_claude.map(|cost| CostStat {
            usd: (cost.usd - attributed_claude_usd).max(0.0),
            partial: cost.partial,
            unpriced_models: cost.unpriced_models,
            uncosted_harnesses: Vec::new(),
            by_harness: Vec::new(),
        });
        let readable = residual
            .as_ref()
            .is_some_and(|cost| cost.usd > 0.0 || cost.partial || !cost.unpriced_models.is_empty());
        contributions.push(CostContribution {
            harness: run_harness.to_string(),
            scope: CostScope::Infrastructure,
            node_id: None,
            executions: infrastructure_executions,
            readable_executions: if readable && !ambiguous_shared_claude {
                infrastructure_executions
            } else {
                0
            },
            usd: (readable && !ambiguous_shared_claude)
                .then(|| residual.as_ref().map_or(0.0, |cost| cost.usd)),
            form: (readable && !ambiguous_shared_claude).then_some(CostForm::Derived),
            partial: !ambiguous_shared_claude && residual.as_ref().is_some_and(|cost| cost.partial),
            unpriced_models: if ambiguous_shared_claude {
                Vec::new()
            } else {
                residual
                    .as_ref()
                    .map(|cost| cost.unpriced_models.clone())
                    .unwrap_or_default()
            },
            unavailable_reasons: if readable && !ambiguous_shared_claude {
                Vec::new()
            } else {
                vec!["no attributable infrastructure cost".to_string()]
            },
        });
        if readable && ambiguous_shared_claude {
            let residual = residual.expect("readable residual exists");
            contributions.push(CostContribution {
                harness: crate::harness_registry::CLAUDE.to_string(),
                scope: CostScope::Unassigned,
                node_id: None,
                executions: 0,
                readable_executions: 0,
                usd: Some(residual.usd),
                form: Some(CostForm::Derived),
                partial: residual.partial,
                unpriced_models: residual.unpriced_models,
                unavailable_reasons: vec![
                    "Claude transcript cannot be matched to a legacy node or infrastructure"
                        .to_string(),
                ],
            });
        }
    }

    let any_readable = contributions.iter().any(|c| c.usd.is_some());
    let unpriced_models: Vec<String> = contributions
        .iter()
        .flat_map(|c| c.unpriced_models.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let uncosted_harnesses: Vec<String> = contributions
        .iter()
        .filter(|contribution| {
            contribution.usd.is_none()
                && contribution
                    .unavailable_reasons
                    .iter()
                    .any(|reason| reason == "harness has no cost source")
        })
        .map(|contribution| contribution.harness.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut harnesses: BTreeMap<String, HarnessCost> = BTreeMap::new();
    for contribution in &contributions {
        let (Some(usd), Some(form)) = (contribution.usd, contribution.form) else {
            continue;
        };
        let entry = harnesses
            .entry(contribution.harness.clone())
            .or_insert_with(|| HarnessCost {
                harness: contribution.harness.clone(),
                usd: 0.0,
                form,
                partial: false,
                unpriced_models: Vec::new(),
            });
        entry.usd += usd;
        entry.partial |= contribution.partial;
        entry
            .unpriced_models
            .extend(contribution.unpriced_models.iter().cloned());
        entry.unpriced_models.sort();
        entry.unpriced_models.dedup();
    }
    let cost = any_readable.then(|| CostStat {
        usd: contributions.iter().filter_map(|c| c.usd).sum(),
        partial: !unpriced_models.is_empty(),
        unpriced_models,
        uncosted_harnesses,
        by_harness: harnesses.into_values().collect(),
    });
    RunCostBreakdown {
        contributions,
        cost,
    }
}

/// The four cache buckets are disjoint from `input`/`output` — CC's
/// `input_tokens` already excludes cache tokens, so they sum without subtraction.
#[derive(Default)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create_5m: u64,
    cache_create_1h: u64,
}

struct Line {
    message_id: Option<String>,
    request_id: Option<String>,
    model: String,
    usage: Usage,
}

/// The 5-term ccusage formula. `in_p`/`out_p` are per-MTok list prices; the
/// three cache rates are derived from `in_p`.
fn line_cost(u: &Usage, in_p: f64, out_p: f64) -> f64 {
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
fn parse_line(raw: &str) -> Option<Line> {
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
fn aggregate(lines: impl Iterator<Item = Line>, prices: &PriceTable) -> CostStat {
    let mut seen = std::collections::HashSet::new();
    let mut usd = 0.0;
    let mut unpriced: BTreeSet<String> = BTreeSet::new();
    for l in lines {
        if let Some(id) = &l.message_id {
            if !seen.insert((id.clone(), l.request_id.clone())) {
                continue; // duplicate: replay on resume/compaction
            }
        }
        match prices.price_for(&l.model) {
            Some(p) => usd += line_cost(&l.usage, p.input, p.output),
            // Unknown real model → $0 + named in the lower-bound signal (#425).
            None => {
                unpriced.insert(strip_date_suffix(&l.model).to_string());
            }
        }
    }
    let unpriced_models: Vec<String> = unpriced.into_iter().collect();
    CostStat {
        usd,
        partial: !unpriced_models.is_empty(),
        unpriced_models,
        // Both are assembled one layer up, in `run_cost_or_absence`: an
        // absent-cost-source harness never reaches this fold, and the by-harness
        // ventilation pairs this derived slice with copilot's reported one.
        uncosted_harnesses: Vec::new(),
        by_harness: Vec::new(),
    }
}

/// The distinct harnesses this Run launched a node on that have **no cost
/// source**. Read off the `NodeStarted` payloads (the frozen-at-spawn harness,
/// ADR-0046), never the current YAML. A `null` harness is the `claude` floor,
/// which HAS a cost source, so it never lands here.
pub(crate) fn uncosted_harnesses(events: &[crate::event_log::Event]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for e in events {
        if e.kind != crate::event_log::EventKind::NodeStarted {
            continue;
        }
        let Some(harness) = e
            .payload
            .as_ref()
            .and_then(|p| p.get("harness"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue; // null harness ⇒ the claude floor ⇒ has a cost source
        };
        if !crate::harness_probes::can_cost(harness) {
            names.insert(harness.to_string());
        }
    }
    names.into_iter().collect()
}

/// The `copilot` session ids this Run launched a node on, latest per
/// `(node_id, iter)` so a same-iter restart resolves to the fresh id. Only a
/// `NodeStarted` with a frozen `copilot` harness AND a non-empty session id
/// contributes, so an infra or other-harness journal is never attributed here.
fn copilot_session_ids(events: &[crate::event_log::Event]) -> Vec<String> {
    // Last `NodeStarted` per (node, iter) wins (a restart re-pins a fresh id).
    let mut latest: BTreeMap<(String, i64), String> = BTreeMap::new();
    for e in events {
        if e.kind != crate::event_log::EventKind::NodeStarted {
            continue;
        }
        let Some(payload) = e.payload.as_ref() else {
            continue;
        };
        let harness = payload.get("harness").and_then(|v| v.as_str());
        if harness != Some(crate::harness_registry::COPILOT) {
            continue;
        }
        let Some(sid) = payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let (Some(node), Some(iter)) = (e.node_id.clone(), e.iter) else {
            continue;
        };
        latest.insert((node, iter), sid.to_string());
    }
    let mut ids: Vec<String> = latest.into_values().collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The Run's **reported** `copilot` cost in USD, or `None` when no copilot node
/// has a usage reading yet.
///
/// Attribution is by the imposed session identity, never by scanning the store:
/// copilot's journal carries no working-directory encoding. A reading exists
/// mid-session (each turn writes a `session.usage_checkpoint`), so a live
/// copilot node has a cost rather than a "—" until its reap.
fn copilot_reported_cost(events: &[crate::event_log::Event], copilot_root: &Path) -> Option<f64> {
    let mut usd = 0.0;
    let mut any = false;
    for sid in copilot_session_ids(events) {
        let journal = copilot_root.join(&sid).join("events.jsonl");
        let Ok(text) = std::fs::read_to_string(&journal) else {
            continue;
        };
        if let Some(c) = crate::copilot_journal::reported_cost_usd(&text) {
            usd += c;
            any = true;
        }
    }
    any.then_some(usd)
}

/// Assemble the Run's cost, **ventilated by harness** (ADR-0052 §3). `None` when
/// neither harness contributed a cost — the surfaces' "—".
fn ventilate(
    claude: Option<CostStat>,
    events: &[crate::event_log::Event],
    copilot_root: &Path,
) -> Option<CostStat> {
    let copilot_usd = copilot_reported_cost(events, copilot_root);
    if claude.is_none() && copilot_usd.is_none() {
        return None;
    }

    let mut by_harness: Vec<HarnessCost> = Vec::new();
    let mut total_usd = 0.0;
    let mut partial = false;
    let mut unpriced_models: Vec<String> = Vec::new();

    if let Some(c) = &claude {
        total_usd += c.usd;
        partial = c.partial;
        unpriced_models = c.unpriced_models.clone();
        by_harness.push(HarnessCost {
            harness: crate::harness_registry::CLAUDE.to_string(),
            usd: c.usd,
            form: CostForm::Derived,
            partial: c.partial,
            unpriced_models: c.unpriced_models.clone(),
        });
    }
    if let Some(usd) = copilot_usd {
        total_usd += usd;
        by_harness.push(HarnessCost {
            harness: crate::harness_registry::COPILOT.to_string(),
            usd,
            form: CostForm::Reported,
            // A reported cost never consults the price table, so it can never
            // be a lower bound nor name an unpriced model (ADR-0052 §2).
            partial: false,
            unpriced_models: Vec::new(),
        });
    }
    by_harness.sort_by(|a, b| a.harness.cmp(&b.harness));

    Some(CostStat {
        usd: total_usd,
        partial,
        unpriced_models,
        uncosted_harnesses: Vec::new(),
        by_harness,
    })
}

/// A Run whose cost is **unavailable** because a node ran on a harness with no
/// cost source — "—" with a reason, never a `$0`.
///
/// `slices` are the per-harness costs PDO could still compute for the Run's
/// *other* nodes; they **survive** the unavailable total (ADR-0052 §3). What is
/// refused is the sum, not the knowledge.
fn cost_unavailable(uncosted: Vec<String>, slices: Vec<HarnessCost>) -> CostStat {
    CostStat {
        // Not a total, and never rendered as one: `uncosted_harnesses` non-empty is
        // what makes every surface print "—". The slices say themselves; this field
        // stays 0.0 rather than half a sum, which would read as a total.
        usd: 0.0,
        // NOT `partial`: `partial` means "priced, but a lower bound" and still
        // shows a dollar figure. This is a categorically different state — the
        // aggregate is unavailable, not merely incomplete — so it stays out of
        // the `partial ⟺ !unpriced_models.is_empty()` invariant (both empty).
        // A slice keeps its own `partial`; the Run-level one is about the total.
        partial: false,
        unpriced_models: Vec::new(),
        uncosted_harnesses: uncosted,
        by_harness: slices,
    }
}

fn slices_or_empty(ventilated: Option<CostStat>) -> Vec<HarnessCost> {
    ventilated.map(|c| c.by_harness).unwrap_or_default()
}

/// The Run's cost, **honest about harnesses without a cost source** and
/// **ventilated by harness** (ADR-0052).
///
/// If any node ran on a harness PDO cannot cost, the **total** is not honestly
/// summable and this returns a "—"-with-reason `CostStat`. The ventilation still
/// runs, so a mixed Run says what came through each harness even while refusing
/// to add them (ADR-0052 §3). Only the sum is withheld.
pub(crate) fn run_cost_or_absence(
    events: &[crate::event_log::Event],
    claude_root: &Path,
    copilot_root: &Path,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> Option<CostStat> {
    let uncosted = uncosted_harnesses(events);
    let claude = compute_run_cost(claude_root, repo_root, run_id, prices);
    let ventilated = ventilate(claude, events, copilot_root);
    if !uncosted.is_empty() {
        return Some(cost_unavailable(uncosted, slices_or_empty(ventilated)));
    }
    ventilated
}

/// Encode an absolute path exactly as Claude Code names its `~/.claude/projects`
/// directory: every non-`[A-Za-z0-9]` char → `-`, case preserved, runs NOT
/// collapsed — so `/home/u/.pdo/runs/X/worktree` → `-home-u--pdo-runs-X-worktree`
/// (verified against real dirs). Delegates to
/// [`crate::stale_detector::encode_working_dir`], the single source of truth.
pub(crate) fn cc_project_dirname(path: &Path) -> String {
    crate::stale_detector::encode_working_dir(path)
}

/// The recursion is what captures subagent transcripts nested at
/// `<project>/<uuid>/subagents/*.jsonl`; dedup by `message.id` makes a
/// double-count with the parent impossible.
fn collect_jsonl_recursive(dir: &Path, out: &mut Vec<Line>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_recursive(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Some(parsed) = parse_line(line) {
                        out.push(parsed);
                    }
                }
            }
        }
    }
}

/// Estimated cost for a run: every CC transcript whose project dir is under
/// `<repo_root>/.pdo/runs/<run_id>/`. `None` when no such dir exists (UI "—");
/// `Some { usd: 0.0, .. }` when dirs exist but carry no priced tokens.
///
/// `repo_root` must be the run's **effective** repo root (honours `target_repo`,
/// as already resolved by `effective_repo_root`): it builds the run-id dir
/// prefix, NOT the read root — that is `projects_root`.
pub(crate) fn compute_run_cost(
    projects_root: &Path,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> Option<CostStat> {
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    // Trailing '-' anchors the run_id: a run whose id is a lexical prefix of
    // another can't leak its sessions in (after run_id comes `-nodes`/`-worktree`).
    let prefix = format!("{}-", cc_project_dirname(&run_dir));
    let mut lines = Vec::new();
    let mut found = false;
    for entry in std::fs::read_dir(projects_root).ok()?.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        found = true;
        collect_jsonl_recursive(&entry.path(), &mut lines);
    }
    if !found {
        return None;
    }
    Some(aggregate(lines.into_iter(), prices))
}

// Read-side memo for `/stats/cost`, which fans over every Run in the cohort. It
// stores the complete harness contribution breakdown, never a superseded scalar.
// Nothing is persisted.
const BREAKDOWN_MEMO_CAP: usize = 4096;

/// Mirrors [`collect_jsonl_recursive`]'s traversal but `stat`s only — no file
/// contents are read.
fn max_mtime_recursive(dir: &Path, max_ms: &mut i64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            max_mtime_recursive(&path, max_ms);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Ok(ms) = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
            {
                if ms > *max_ms {
                    *max_ms = ms;
                }
            }
        }
    }
}

/// Max mtime (epoch millis) across every `*.jsonl` transcript contributing to
/// `run_id`'s cost — the same recursive glob [`compute_run_cost`] aggregates.
/// `0` when nothing exists yet, so a later write bumps the key and invalidates
/// the memo.
pub(crate) fn max_transcript_mtime_millis(
    projects_root: &Path,
    repo_root: &Path,
    run_id: &str,
) -> i64 {
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    let prefix = format!("{}-", cc_project_dirname(&run_dir));
    let Ok(entries) = std::fs::read_dir(projects_root) else {
        return 0;
    };
    let mut max_ms: i64 = 0;
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        max_mtime_recursive(&entry.path(), &mut max_ms);
    }
    max_ms
}

type BreakdownMemoKey = (String, i64, i64, u64, u64, u64);
type BreakdownMemoMap = HashMap<BreakdownMemoKey, RunCostBreakdown>;

static BREAKDOWN_MEMO: OnceLock<Mutex<BreakdownMemoMap>> = OnceLock::new();

fn breakdown_memo() -> &'static Mutex<BreakdownMemoMap> {
    BREAKDOWN_MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The Copilot mirror of [`max_transcript_mtime_millis`]. `pub(crate)` so a
/// whole-cohort cache key (`stats_performance`'s) can fold a Run's Copilot
/// contribution alongside its Claude one without re-deriving this lookup.
pub(crate) fn copilot_mtime_millis(events: &[crate::event_log::Event], copilot_root: &Path) -> i64 {
    events
        .iter()
        .filter_map(|event| {
            event
                .payload
                .as_ref()
                .and_then(|payload| payload.get("session_id"))
                .and_then(|value| value.as_str())
        })
        .filter_map(|session_id| {
            std::fs::metadata(copilot_root.join(session_id).join("events.jsonl"))
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .map(|modified| {
            modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
        })
        .max()
        .unwrap_or(0)
}

/// Order-and-content fingerprint of a Run's own event log. `pub(crate)` so any
/// other cache keyed on "this Run's events haven't changed" reuses this exact
/// hash rather than defining a second, possibly inconsistent one.
pub(crate) fn event_fingerprint(events: &[crate::event_log::Event]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for event in events {
        event.run_id.hash(&mut hasher);
        event.ts.hash(&mut hasher);
        serde_json::to_string(&event.kind)
            .unwrap_or_default()
            .hash(&mut hasher);
        event.node_id.hash(&mut hasher);
        event.iter.hash(&mut hasher);
        event
            .payload
            .as_ref()
            .map(serde_json::Value::to_string)
            .hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn compute_run_cost_breakdown_cached(
    events: &[crate::event_log::Event],
    claude_root: &Path,
    copilot_root: &Path,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> RunCostBreakdown {
    let mut roots = DefaultHasher::new();
    claude_root.hash(&mut roots);
    copilot_root.hash(&mut roots);
    repo_root.hash(&mut roots);
    let key = (
        run_id.to_string(),
        max_transcript_mtime_millis(claude_root, repo_root, run_id),
        copilot_mtime_millis(events, copilot_root),
        prices.fingerprint(),
        event_fingerprint(events),
        roots.finish(),
    );
    {
        let guard = breakdown_memo()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let value =
        compute_run_cost_breakdown(events, claude_root, copilot_root, repo_root, run_id, prices);
    let mut guard = breakdown_memo()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.len() >= BREAKDOWN_MEMO_CAP {
        guard.clear();
    }
    guard.insert(key, value.clone());
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // `strip_date_suffix` and the whole `price_for` family moved to
    // `price_table.rs` with #427, and their tests moved with them. What stays here
    // is everything that is about *transcripts*, not about *prices* — every case
    // below now pins the EMBEDDED tier explicitly via `PriceTable::builtin()`, so a
    // stray `~/.pdo/prices/` on a dev machine can never turn this module red.
    fn builtin() -> PriceTable {
        PriceTable::builtin()
    }

    // --- line_cost ---

    #[test]
    fn line_cost_sums_five_buckets_without_overlap() {
        let u = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_create_5m: 1_000_000,
            cache_create_1h: 1_000_000,
        };
        // opus-4-8: in=5, out=25. 5 + 25 + 5*1.25 + 5*2 + 5*0.1 = 46.75
        assert!((line_cost(&u, 5.0, 25.0) - 46.75).abs() < 1e-9);
    }

    // --- cc_project_dirname ---

    #[test]
    fn encodes_like_claude_code() {
        // Verified against a real ~/.claude/projects dir name.
        assert_eq!(
            cc_project_dirname(Path::new("/home/u/.pdo/runs/X/worktree")),
            "-home-u--pdo-runs-X-worktree"
        );
        // Case is preserved; every non-alphanumeric char maps to '-'.
        assert_eq!(
            cc_project_dirname(Path::new("/home/llenoir/Documents/perso/Maestro")),
            "-home-llenoir-Documents-perso-Maestro"
        );
    }

    // --- parse_line ---

    fn assistant(id: &str, req: &str, model: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"{req}","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn copilot_nano_aiu_converts_by_the_published_constant() {
        assert!((nano_aiu_to_usd(2_823_580_000) - 0.0282358).abs() < 1e-12);
        assert!((nano_aiu_to_usd(100_000_000_000) - 1.0).abs() < 1e-12);
        assert_eq!(nano_aiu_to_usd(0), 0.0);
    }

    #[test]
    fn copilot_reported_cost_reads_the_max_total_and_is_none_when_absent() {
        let journal = concat!(
            "{\"type\":\"session.usage_checkpoint\",\"data\":{\"totalNanoAiu\":2823580000}}\n",
            "torn json\n",
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":2000000000}}\n",
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":3000000000}}\n"
        );
        assert!((reported_cost_usd(journal).unwrap() - 0.03).abs() < 1e-12);
        assert!(reported_cost_usd("{\"type\":\"assistant.turn_start\"}\n").is_none());
        assert!(reported_cost_usd("").is_none());
    }

    #[test]
    fn parses_a_valid_assistant_line() {
        let l = parse_line(&assistant("m1", "r1", "claude-opus-4-8", 100, 50)).unwrap();
        assert_eq!(l.message_id.as_deref(), Some("m1"));
        assert_eq!(l.request_id.as_deref(), Some("r1"));
        assert_eq!(l.model, "claude-opus-4-8");
        assert_eq!(l.usage.input, 100);
        assert_eq!(l.usage.output, 50);
    }

    #[test]
    fn skips_torn_or_invalid_json() {
        assert!(parse_line("clauclaude-opus-4-8 garbage").is_none());
        assert!(parse_line("{not json").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn skips_non_assistant_synthetic_error_and_zero() {
        // user line
        assert!(parse_line(r#"{"type":"user","message":{"role":"user"}}"#).is_none());
        // synthetic sentinel
        assert!(parse_line(&assistant("m", "r", "<synthetic>", 10, 10)).is_none());
        // api error message
        assert!(parse_line(
            r#"{"type":"assistant","isApiErrorMessage":true,"message":{"id":"m","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":10}}}"#
        )
        .is_none());
        // all-zero usage
        assert!(parse_line(&assistant("m", "r", "claude-opus-4-8", 0, 0)).is_none());
    }

    #[test]
    fn uses_nested_cache_creation_split() {
        let raw = r#"{"type":"assistant","requestId":"r","message":{"id":"m","model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":100,"cache_creation":{"ephemeral_5m_input_tokens":30,"ephemeral_1h_input_tokens":70}}}}"#;
        let l = parse_line(raw).unwrap();
        assert_eq!(l.usage.cache_create_5m, 30);
        assert_eq!(l.usage.cache_create_1h, 70);
    }

    #[test]
    fn falls_back_to_flat_cache_creation_into_5m() {
        let raw = r#"{"type":"assistant","requestId":"r","message":{"id":"m","model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":100}}}"#;
        let l = parse_line(raw).unwrap();
        assert_eq!(l.usage.cache_create_5m, 100);
        assert_eq!(l.usage.cache_create_1h, 0);
    }

    // --- aggregate ---

    fn line(id: Option<&str>, req: Option<&str>, model: &str, input: u64) -> Line {
        Line {
            message_id: id.map(String::from),
            request_id: req.map(String::from),
            model: model.into(),
            usage: Usage {
                input,
                ..Default::default()
            },
        }
    }

    #[test]
    fn aggregate_dedups_by_message_id_and_request_id() {
        // Two copies of the same (m1, r1) → counted once; a distinct line counts too.
        let lines = vec![
            line(Some("m1"), Some("r1"), "claude-opus-4-8", 1_000_000),
            line(Some("m1"), Some("r1"), "claude-opus-4-8", 1_000_000), // dup
            line(Some("m2"), Some("r2"), "claude-opus-4-8", 1_000_000),
        ];
        let c = aggregate(lines.into_iter(), &builtin());
        // 2 distinct × (1M input × $5 / 1M) = $5 + $5 = $10 (dup excluded).
        assert!((c.usd - 10.0).abs() < 1e-9, "usd = {}", c.usd);
        assert!(!c.partial);
    }

    #[test]
    fn aggregate_counts_lines_without_message_id_each_time() {
        let lines = vec![
            line(None, None, "claude-opus-4-8", 1_000_000),
            line(None, None, "claude-opus-4-8", 1_000_000),
        ];
        let c = aggregate(lines.into_iter(), &builtin());
        assert!((c.usd - 10.0).abs() < 1e-9, "usd = {}", c.usd);
    }

    #[test]
    fn aggregate_flags_partial_on_unknown_model() {
        let lines = vec![
            line(Some("m1"), Some("r1"), "claude-opus-4-8", 1_000_000),
            line(Some("m2"), Some("r2"), "some-future-model", 1_000_000),
        ];
        let c = aggregate(lines.into_iter(), &builtin());
        // Only the priced line contributes; the unknown one flags partial + $0.
        assert!((c.usd - 5.0).abs() < 1e-9, "usd = {}", c.usd);
        assert!(c.partial);
        // #425 AC#4: the offender is NAMED, not just counted.
        assert_eq!(c.unpriced_models, vec!["some-future-model".to_string()]);
    }

    #[test]
    fn aggregate_names_de_dates_and_dedups_unpriced_models() {
        // #425: several lines, three of them on models no tier prices. The dated
        // and undated forms of the SAME family collapse to one name (the family
        // key a human would add to price it), and the set is sorted + unique.
        // #527 floored gen-5, so the unpriced exemplars are models BEYOND the
        // floor (`claude-opus-6`/`claude-opus-7`) — the case a sync still repairs.
        let lines = vec![
            line(Some("m1"), Some("r1"), "claude-opus-4-8", 1_000_000), // priced
            line(Some("m2"), Some("r2"), "claude-opus-6", 1_000_000),
            line(Some("m3"), Some("r3"), "claude-opus-6-20260501", 1_000_000), // same family
            line(Some("m4"), Some("r4"), "claude-opus-7", 1_000_000),
        ];
        let c = aggregate(lines.into_iter(), &builtin());
        // Only the one priced line contributes.
        assert!((c.usd - 5.0).abs() < 1e-9, "usd = {}", c.usd);
        assert!(c.partial);
        assert_eq!(
            c.unpriced_models,
            vec!["claude-opus-6".to_string(), "claude-opus-7".to_string()],
            "de-dated, de-duplicated, sorted"
        );
        // The invariant the whole design rests on.
        assert_eq!(c.partial, !c.unpriced_models.is_empty());
    }

    #[test]
    fn aggregate_synthetic_does_not_flip_partial() {
        let lines = vec![line(Some("m1"), Some("r1"), "<synthetic>", 1_000_000)];
        let c = aggregate(lines.into_iter(), &builtin());
        assert_eq!(c.usd, 0.0);
        assert!(!c.partial);
        // The negative control: a $0 sentinel is priced, so it never names itself.
        assert!(c.unpriced_models.is_empty());
    }

    // --- compute_run_cost (filesystem) ---
    //
    // Since #408 the `projects/` root is a parameter (the observability seam), so
    // these tests plant transcripts under a tempdir root and pass it directly —
    // no HOME swap, no crate-wide HOME lock, fully hermetic. A `projects` root
    // stands in for either `~/.claude/projects/` or a sandboxed run's staged home.

    #[test]
    fn compute_run_cost_aggregates_and_dedups_across_sessions() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "20260706-abc-node";
        // A session cwd under the run dir (the worktree, where the manager runs).
        let worktree = repo
            .path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree");
        let proj = projects.join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&proj).unwrap();

        let l1 = assistant("msg_1", "req_1", "claude-opus-4-8", 1000, 500);
        let l2 = assistant("msg_2", "req_2", "claude-opus-4-8", 2000, 1000);
        // l1 replayed (same msg_1, req_1) → deduped.
        std::fs::write(proj.join("s.jsonl"), format!("{l1}\n{l1}\n{l2}\n")).unwrap();

        let cost = compute_run_cost(&projects, repo.path(), run_id, &builtin()).unwrap();
        // (1000*5 + 500*25)/1e6 + (2000*5 + 1000*25)/1e6 = 0.0175 + 0.035 = 0.0525
        assert!((cost.usd - 0.0525).abs() < 1e-9, "usd = {}", cost.usd);
        assert!(!cost.partial);
    }

    #[test]
    fn compute_run_cost_recurses_into_subagents() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "20260706-sub";
        let node = repo
            .path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("nodes")
            .join("N")
            .join("iter-1");
        let proj = projects.join(cc_project_dirname(&node));
        let subagents = proj.join("uuid-1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(
            proj.join("main.jsonl"),
            format!(
                "{}\n",
                assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        std::fs::write(
            subagents.join("side.jsonl"),
            format!(
                "{}\n",
                assistant("m2", "r2", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();

        let cost = compute_run_cost(&projects, repo.path(), run_id, &builtin()).unwrap();
        // 1M input × $5/MTok, twice (main + subagent) = $10.
        assert!((cost.usd - 10.0).abs() < 1e-9, "usd = {}", cost.usd);
    }

    #[test]
    fn compute_run_cost_none_when_no_transcript_dir() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let repo = tempfile::tempdir().unwrap();
        assert!(compute_run_cost(&projects, repo.path(), "no-such-run", &builtin()).is_none());
    }

    // --- transcript discovery and production breakdown memo ---

    /// Write a single-line transcript for `run_id`'s worktree under `projects` and
    /// return the `.jsonl` path so the test can manipulate its mtime.
    fn seed_transcript(
        projects: &Path,
        repo: &Path,
        run_id: &str,
        line: &str,
    ) -> std::path::PathBuf {
        let worktree = repo.join(".pdo").join("runs").join(run_id).join("worktree");
        let proj = projects.join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("s.jsonl");
        std::fs::write(&file, format!("{line}\n")).unwrap();
        file
    }

    #[test]
    fn max_transcript_mtime_is_zero_without_a_transcript_and_positive_with_one() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let repo = tempfile::tempdir().unwrap();
        assert_eq!(
            max_transcript_mtime_millis(&projects, repo.path(), "no-such-run"),
            0,
            "no transcript dir → 0 (so a later write bumps the key)"
        );
        let run_id = "mtime-run";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1000, 0),
        );
        assert!(max_transcript_mtime_millis(&projects, repo.path(), run_id) > 0);
    }

    #[test]
    fn compute_run_cost_prefix_does_not_leak_across_runs() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        // Two runs where one id is a lexical prefix of the other.
        let other = repo
            .path()
            .join(".pdo")
            .join("runs")
            .join("run-1x") // "run-1" is a prefix of "run-1x"
            .join("worktree");
        let proj = projects.join(cc_project_dirname(&other));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("s.jsonl"),
            format!("{}\n", assistant("m", "r", "claude-opus-4-8", 1_000_000, 0)),
        )
        .unwrap();

        // Querying "run-1" must NOT pick up "run-1x"'s transcript.
        assert!(compute_run_cost(&projects, repo.path(), "run-1", &builtin()).is_none());
    }

    // --- run_cost_or_absence / uncosted_harnesses (#553) ---
    //
    // The test that guarantees "absence is said": a Run on a harness with no cost
    // source shows "—" and a reason, never `0`.

    use crate::event_log::{Event, EventKind};

    fn node_started(node: &str, harness: Option<&str>) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "harness": harness })),
        }
    }

    #[test]
    fn uncosted_harnesses_names_only_the_harnesses_without_a_cost_source() {
        let events = vec![
            node_started("a", Some("claude")),   // has a cost source
            node_started("b", Some("opencode")), // none
            node_started("c", None),             // script/legacy ⇒ claude floor ⇒ costed
            node_started("d", Some("opencode")), // duplicate ⇒ deduped
        ];
        assert_eq!(uncosted_harnesses(&events), vec!["opencode".to_string()]);
    }

    #[test]
    fn run_cost_or_absence_is_dash_with_reason_not_zero_for_an_uncosted_harness() {
        // A Run with an `opencode` node: no transcript need even be read — the
        // aggregate is not honestly summable, so cost is "—" (usd 0, NOT partial)
        // and names the harness.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        std::fs::create_dir_all(&projects).unwrap();
        let repo = tempfile::tempdir().unwrap();
        let events = vec![node_started("n", Some("opencode"))];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            "run-x",
            &builtin(),
        )
        .expect("an uncosted harness yields Some(—), not a bare None");
        assert_eq!(cost.usd, 0.0);
        assert!(!cost.partial, "not a lower bound — it is unavailable");
        assert!(cost.unpriced_models.is_empty());
        assert!(
            cost.by_harness.is_empty(),
            "no transcript, no journal ⇒ no slice to say — NOT because the total is \
             unavailable (see the trio test below)"
        );
        // The offender is NAMED (the frontend builds the "— because opencode has no
        // cost source" sentence from this, the same way it names `unpriced_models`).
        assert_eq!(cost.uncosted_harnesses, vec!["opencode".to_string()]);
    }

    /// A `node_started` event carrying a harness AND a pinned session id — the shape
    /// a `copilot` node leaves (#615), so its reported cost can be attributed by
    /// identity.
    fn node_started_sid(node: &str, harness: &str, session_id: &str) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "harness": harness, "session_id": session_id })),
        }
    }

    /// Write a copilot event journal for `session_id` under `copilot_root` with a
    /// usage checkpoint of `nano_aiu`.
    fn seed_copilot_journal(copilot_root: &Path, session_id: &str, nano_aiu: u64) {
        let dir = copilot_root.join(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        let checkpoint = format!(
            r#"{{"type":"session.usage_checkpoint","data":{{"totalNanoAiu":{nano_aiu}}}}}"#
        );
        std::fs::write(
            dir.join("events.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                r#"{"type":"assistant.turn_start","data":{"turnId":"0"}}"#,
                r#"{"type":"assistant.turn_end","data":{"turnId":"0"}}"#,
                checkpoint,
            ),
        )
        .unwrap();
    }

    #[test]
    fn run_cost_or_absence_ventilates_a_claude_only_run_as_one_derived_slice() {
        // The negative control: a claude Run (and a script node) costs its real
        // dollars, now ventilated as a single `claude` derived slice.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "claude-run";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let events = vec![node_started("n", Some("claude")), node_started("s", None)];

        let honest = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            run_id,
            &builtin(),
        )
        .unwrap();
        assert!(honest.uncosted_harnesses.is_empty());
        assert!((honest.usd - 5.0).abs() < 1e-9);
        // Ventilated: one derived slice, on `claude`, carrying the whole figure.
        assert_eq!(honest.by_harness.len(), 1);
        assert_eq!(honest.by_harness[0].harness, "claude");
        assert_eq!(honest.by_harness[0].form, CostForm::Derived);
        assert!((honest.by_harness[0].usd - 5.0).abs() < 1e-9);
    }

    #[test]
    fn run_cost_or_absence_ventilates_a_mixed_run_by_harness() {
        // FP: a mixed Run — one claude node, one copilot node — sums in dollars but
        // says itself per harness: X via `copilot` (reported), Y via `claude`
        // (derived). No unpriced model on the copilot part (it never sees the table).
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "mixed-run";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0), // $5 derived
        );
        // 2e11 nano-AIU = 200 AIU = $2.00 reported.
        seed_copilot_journal(&copilot, "sid-cop", 200_000_000_000);
        let events = vec![
            node_started("c", Some("claude")),
            node_started_sid("p", "copilot", "sid-cop"),
        ];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            run_id,
            &builtin(),
        )
        .unwrap();
        // Summable total = $5 (claude) + $2 (copilot).
        assert!((cost.usd - 7.0).abs() < 1e-9, "usd = {}", cost.usd);
        assert!(cost.uncosted_harnesses.is_empty());
        // Two slices, in name order (claude, copilot).
        assert_eq!(cost.by_harness.len(), 2);
        let claude = &cost.by_harness[0];
        assert_eq!(claude.harness, "claude");
        assert_eq!(claude.form, CostForm::Derived);
        assert!((claude.usd - 5.0).abs() < 1e-9);
        let cop = &cost.by_harness[1];
        assert_eq!(cop.harness, "copilot");
        assert_eq!(cop.form, CostForm::Reported);
        assert!((cop.usd - 2.0).abs() < 1e-9);
        assert!(!cop.partial, "a reported slice is never a lower bound");
        assert!(
            cop.unpriced_models.is_empty(),
            "reported ⇒ no unpriced model"
        );
    }

    #[test]
    fn an_unavailable_total_still_says_the_slices_it_can_compute() {
        // #617 FP finding 1: the three-harness Run (claude + opencode + copilot) is
        // the one built to *observe* ventilation, and it was the only one that could
        // not show any — the `opencode` short-circuit returned before either slice
        // was computed. The total is still refused (that is #553), but what came
        // through `claude` and what came through `copilot` is said (ADR-0052 §3).
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "trio-run";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0), // $5 derived
        );
        seed_copilot_journal(&copilot, "sid-cop", 200_000_000_000); // $2 reported
        let events = vec![
            node_started("c", Some("claude")),
            node_started("o", Some("opencode")),
            node_started_sid("p", "copilot", "sid-cop"),
        ];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            run_id,
            &builtin(),
        )
        .unwrap();

        // The total is still withheld, and still names why.
        assert_eq!(cost.usd, 0.0, "never half a sum standing in for a total");
        assert!(!cost.partial);
        assert!(cost.unpriced_models.is_empty());
        assert_eq!(cost.uncosted_harnesses, vec!["opencode".to_string()]);

        // …and the two computable slices ride along with the absence.
        assert_eq!(
            cost.by_harness.len(),
            2,
            "by_harness = {:?}",
            cost.by_harness
        );
        assert_eq!(cost.by_harness[0].harness, "claude");
        assert_eq!(cost.by_harness[0].form, CostForm::Derived);
        assert!((cost.by_harness[0].usd - 5.0).abs() < 1e-9);
        assert_eq!(cost.by_harness[1].harness, "copilot");
        assert_eq!(cost.by_harness[1].form, CostForm::Reported);
        assert!((cost.by_harness[1].usd - 2.0).abs() < 1e-9);
        // `opencode` has no slice at all — an absence is not a $0 slice either.
        assert!(!cost.by_harness.iter().any(|h| h.harness == "opencode"));
    }

    #[test]
    fn the_run_cost_path_says_the_slices_under_an_unavailable_total() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "trio-run-cached";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        seed_copilot_journal(&copilot, "sid-cop-cached", 200_000_000_000);
        let events = vec![
            node_started("c", Some("claude")),
            node_started("o", Some("opencode")),
            node_started_sid("p", "copilot", "sid-cop-cached"),
        ];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            run_id,
            &builtin(),
        )
        .unwrap();
        assert_eq!(cost.uncosted_harnesses, vec!["opencode".to_string()]);
        assert_eq!(
            cost.by_harness
                .iter()
                .map(|h| h.harness.as_str())
                .collect::<Vec<_>>(),
            vec!["claude", "copilot"]
        );
    }

    #[test]
    fn a_copilot_only_run_reports_its_cost_while_it_runs() {
        // AC: a copilot node's cost is readable mid-session (a checkpoint is written
        // each turn), not a "—" until reap. Here: no claude transcript at all.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "sid-live", 100_000_000_000); // $1.00
        let events = vec![node_started_sid("p", "copilot", "sid-live")];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &copilot,
            repo.path(),
            "cop-run",
            &builtin(),
        )
        .expect("a copilot reading yields Some, even with no claude transcript");
        assert!((cost.usd - 1.0).abs() < 1e-9);
        assert_eq!(cost.by_harness.len(), 1);
        assert_eq!(cost.by_harness[0].harness, "copilot");
        assert_eq!(cost.by_harness[0].form, CostForm::Reported);
    }

    #[test]
    fn copilot_cost_attributes_only_this_runs_sessions_not_an_infra_one() {
        // *correctif 6*: the reported cost reads ONLY the run's copilot-node session
        // ids, never by scanning the store — so an unrelated session sitting in the
        // same `.copilot/session-state/` (an infra session, another harness) is not
        // attributed to this Run.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "sid-mine", 100_000_000_000); // $1.00, this run
        seed_copilot_journal(&copilot, "sid-stranger", 900_000_000_000); // $9.00, NOT this run
        let events = vec![node_started_sid("p", "copilot", "sid-mine")];

        let cost =
            run_cost_or_absence(&events, &projects, &copilot, repo.path(), "run", &builtin())
                .unwrap();
        assert!(
            (cost.usd - 1.0).abs() < 1e-9,
            "only sid-mine is attributed, not the stranger session: usd = {}",
            cost.usd
        );
    }

    #[test]
    fn copilot_session_ids_takes_the_latest_per_node_iter_and_ignores_other_harnesses() {
        let events = vec![
            node_started_sid("p", "copilot", "old"),
            node_started_sid("p", "copilot", "fresh"), // same (node,iter) restart ⇒ latest wins
            node_started_sid("q", "copilot", "q-sid"),
            node_started("c", Some("claude")), // not copilot ⇒ no id
            node_started("o", Some("opencode")),
        ];
        assert_eq!(
            copilot_session_ids(&events),
            vec!["fresh".to_string(), "q-sid".to_string()]
        );
    }

    #[test]
    fn copilot_adapter_keeps_each_restart_as_a_separate_reported_execution() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(&claude).unwrap();

        for (sid, nano_aiu) in [
            ("sid-first", 100_000_000_000_u64),
            ("sid-restart", 200_000_000_000_u64),
        ] {
            let dir = copilot.join(sid);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("events.jsonl"),
                format!(
                    "{{\"type\":\"session.usage_checkpoint\",\"data\":{{\"totalNanoAiu\":{nano_aiu}}}}}\n"
                ),
            )
            .unwrap();
        }

        let started = |sid: &str| Event {
            id: None,
            run_id: "r".into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "doc-only",
                "harness": "copilot",
                "session_id": sid
            })),
        };
        let events = vec![started("sid-first"), started("sid-restart")];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), "r", &builtin());
        let node: Vec<&CostContribution> = breakdown
            .contributions
            .iter()
            .filter(|c| c.scope == CostScope::Node)
            .collect();

        assert_eq!(node.len(), 2, "a restart is another execution");
        assert!(node.iter().all(|c| c.harness == "copilot"));
        assert!(node.iter().all(|c| c.form == Some(CostForm::Reported)));
        let usd: Vec<f64> = node.iter().map(|c| c.usd.unwrap()).collect();
        assert!((usd[0] - 1.0).abs() < 1e-9);
        assert!((usd[1] - 2.0).abs() < 1e-9);
        assert!((breakdown.cost.unwrap().usd - 3.0).abs() < 1e-9);
    }

    #[test]
    fn pane_resurrection_does_not_bill_the_same_frozen_session_twice() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "resurrected-pane";

        let node_dir = crate::worktree_ops::sub_worktree_path(repo.path(), run_id, "claude", 1);
        let project = claude.join(cc_project_dirname(&node_dir));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("claude-session.jsonl"),
            format!(
                "{}\n",
                assistant("c", "req-c", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        let journal = copilot.join("copilot-session").join("events.jsonl");
        std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
        std::fs::write(
            journal,
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":100000000000}}\n",
        )
        .unwrap();

        let started = |node: &str, harness: &str, session_id: &str| Event {
            id: None,
            run_id: run_id.into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type":"code-mutating",
                "harness":harness,
                "session_id":session_id
            })),
        };
        let claude_start = started("claude", "claude", "claude-session");
        let copilot_start = started("copilot", "copilot", "copilot-session");
        let events = vec![
            claude_start.clone(),
            claude_start,
            copilot_start.clone(),
            copilot_start,
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), run_id, &builtin());
        let nodes: Vec<_> = breakdown
            .contributions
            .iter()
            .filter(|contribution| contribution.scope == CostScope::Node)
            .collect();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes.iter().map(|node| node.executions).sum::<i64>(), 2);
        assert!((breakdown.cost.unwrap().usd - 6.0).abs() < 1e-9);
    }

    #[test]
    fn legacy_starts_without_session_identity_remain_distinct() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let start = Event {
            id: None,
            run_id: "legacy-restarts".into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "node_type":"doc-only" })),
        };

        let breakdown = compute_run_cost_breakdown(
            &[start.clone(), start],
            &claude,
            &copilot,
            repo.path(),
            "legacy-restarts",
            &builtin(),
        );
        assert_eq!(
            breakdown
                .contributions
                .iter()
                .filter(|contribution| contribution.scope == CostScope::Node)
                .count(),
            2
        );
    }

    #[test]
    fn claude_adapter_attaches_subagent_cost_to_its_parent_execution() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "r";
        let working_dir = repo
            .path()
            .join(".pdo/runs")
            .join(run_id)
            .join("nodes/worker/iter-1");
        let project = claude.join(cc_project_dirname(&working_dir));
        std::fs::create_dir_all(project.join("sid/subagents")).unwrap();
        std::fs::write(
            project.join("sid.jsonl"),
            format!(
                "{}\n",
                assistant("main", "req-main", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        std::fs::write(
            project.join("sid/subagents/side.jsonl"),
            format!(
                "{}\n",
                assistant("sub", "req-sub", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        let events = vec![Event {
            id: None,
            run_id: run_id.into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "code-mutating",
                "harness": "claude",
                "session_id": "sid"
            })),
        }];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), run_id, &builtin());

        assert_eq!(breakdown.contributions.len(), 1);
        let contribution = &breakdown.contributions[0];
        assert_eq!(contribution.scope, CostScope::Node);
        assert_eq!(contribution.node_id.as_deref(), Some("worker"));
        assert_eq!(contribution.executions, 1, "a subagent is not an execution");
        assert_eq!(contribution.readable_executions, 1);
        assert_eq!(contribution.form, Some(CostForm::Derived));
        assert!((contribution.usd.unwrap() - 10.0).abs() < 1e-9);
        assert!(
            (breakdown.cost.unwrap().usd - contribution.usd.unwrap()).abs() < 1e-9,
            "the Run total uses the same contribution source"
        );
    }

    #[test]
    fn claude_cost_outside_node_sessions_is_infrastructure() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "infra-run";
        let worktree = repo.path().join(".pdo/runs").join(run_id).join("worktree");
        let project = claude.join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("manager.jsonl"),
            format!(
                "{}\n",
                assistant("manager", "req-manager", "claude-opus-4-8", 400_000, 0)
            ),
        )
        .unwrap();
        let events = vec![
            Event {
                id: None,
                run_id: run_id.into(),
                ts: crate::event_log::now_iso(),
                kind: EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "harness": "claude" })),
            },
            Event {
                id: None,
                run_id: run_id.into(),
                ts: crate::event_log::now_iso(),
                kind: EventKind::MergeResolverStarted,
                node_id: None,
                iter: None,
                payload: None,
            },
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), run_id, &builtin());

        assert_eq!(breakdown.contributions.len(), 1);
        let infra = &breakdown.contributions[0];
        assert_eq!(infra.scope, CostScope::Infrastructure);
        assert_eq!(infra.harness, "claude");
        assert_eq!(infra.executions, 2);
        assert_eq!(infra.readable_executions, 2);
        assert!((infra.usd.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn non_claude_infrastructure_keeps_run_harness_and_unknown_session_cost() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let event = |kind| Event {
            id: None,
            run_id: "copilot-infra".into(),
            ts: crate::event_log::now_iso(),
            kind,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "harness": "copilot" })),
        };
        let events = vec![
            event(EventKind::RunStarted),
            event(EventKind::MergeResolverStarted),
        ];

        let breakdown = compute_run_cost_breakdown(
            &events,
            &claude,
            &copilot,
            repo.path(),
            "copilot-infra",
            &builtin(),
        );
        let infrastructure = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Infrastructure)
            .unwrap();
        assert_eq!(infrastructure.harness, "copilot");
        assert_eq!(infrastructure.executions, 2);
        assert_eq!(infrastructure.readable_executions, 0);
        assert_eq!(infrastructure.usd, None);
        assert_eq!(
            infrastructure.unavailable_reasons,
            vec!["no attributable infrastructure cost"]
        );
    }

    #[test]
    fn legacy_agent_is_claude_script_is_excluded_and_ambiguous_cost_is_unassigned() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "legacy-run";
        let worktree = repo.path().join(".pdo/runs").join(run_id).join("worktree");
        let project = claude.join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("unknown-session.jsonl"),
            format!(
                "{}\n",
                assistant("legacy", "req-legacy", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        let event = |node: Option<&str>, node_type: Option<&str>| Event {
            id: None,
            run_id: run_id.into(),
            ts: crate::event_log::now_iso(),
            kind: if node.is_some() {
                EventKind::NodeStarted
            } else {
                EventKind::RunStarted
            },
            node_id: node.map(str::to_string),
            iter: node.map(|_| 1),
            payload: node_type.map(|kind| serde_json::json!({ "node_type": kind })),
        };
        let mut started = event(None, None);
        started.payload = Some(serde_json::json!({
            "node_defs": [
                {"id":"legacy-agent","node_type":"doc-only"},
                {"id":"deterministic","node_type":"script"}
            ]
        }));
        let events = vec![
            started,
            event(Some("legacy-agent"), Some("doc-only")),
            event(Some("deterministic"), None),
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), run_id, &builtin());

        let node: Vec<_> = breakdown
            .contributions
            .iter()
            .filter(|c| c.scope == CostScope::Node)
            .collect();
        assert_eq!(node.len(), 1, "the script start is not agentic");
        assert_eq!(node[0].node_id.as_deref(), Some("legacy-agent"));
        assert_eq!(node[0].harness, "claude");
        assert!(node[0].usd.is_none());

        let infra = breakdown
            .contributions
            .iter()
            .find(|c| c.scope == CostScope::Infrastructure)
            .unwrap();
        assert!(
            infra.usd.is_none(),
            "do not guess that the residual is infra"
        );
        let unassigned = breakdown
            .contributions
            .iter()
            .find(|c| c.scope == CostScope::Unassigned)
            .unwrap();
        assert_eq!(
            unassigned.executions, 0,
            "Unassigned invents no denominator"
        );
        assert!((unassigned.usd.unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn run_cost_projects_the_same_mixed_harness_contributions_as_stats() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "mixed";

        let node_dir = repo
            .path()
            .join(".pdo/runs")
            .join(run_id)
            .join("nodes/claude-node/iter-1");
        let claude_project = claude.join(cc_project_dirname(&node_dir));
        std::fs::create_dir_all(&claude_project).unwrap();
        std::fs::write(
            claude_project.join("sid-claude.jsonl"),
            format!(
                "{}\n",
                assistant("c", "req-c", "claude-opus-4-8", 1_000_000, 0)
            ),
        )
        .unwrap();
        let copilot_dir = copilot.join("sid-copilot");
        std::fs::create_dir_all(&copilot_dir).unwrap();
        std::fs::write(
            copilot_dir.join("events.jsonl"),
            "{\"type\":\"session.usage_checkpoint\",\"data\":{\"totalNanoAiu\":200000000000}}\n",
        )
        .unwrap();
        let started = |node: &str, harness: &str, sid: &str, kind: &str| Event {
            id: None,
            run_id: run_id.into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": kind,
                "harness": harness,
                "session_id": sid
            })),
        };
        let events = vec![
            started("claude-node", "claude", "sid-claude", "code-mutating"),
            started("copilot-node", "copilot", "sid-copilot", "doc-only"),
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &copilot, repo.path(), run_id, &builtin());
        let run_cost =
            run_cost_or_absence(&events, &claude, &copilot, repo.path(), run_id, &builtin())
                .unwrap();

        assert_eq!(run_cost, breakdown.cost.unwrap());
        assert_eq!(run_cost.by_harness.len(), 2);
        assert!((run_cost.usd - 7.0).abs() < 1e-9);
    }

    #[test]
    fn contribution_memo_refreshes_when_a_reported_cost_journal_changes() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let repo = tempfile::tempdir().unwrap();
        let journal = copilot.join("memo-session").join("events.jsonl");
        std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
        std::fs::write(
            &journal,
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":100000000000}}\n",
        )
        .unwrap();
        let events = vec![Event {
            id: None,
            run_id: "memo-run".into(),
            ts: crate::event_log::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type":"doc-only",
                "harness":"copilot",
                "session_id":"memo-session"
            })),
        }];

        let first = compute_run_cost_breakdown_cached(
            &events,
            &claude,
            &copilot,
            repo.path(),
            "memo-run",
            &builtin(),
        );
        assert!((first.cost.unwrap().usd - 1.0).abs() < 1e-9);

        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(
            &journal,
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":200000000000}}\n",
        )
        .unwrap();
        let refreshed = compute_run_cost_breakdown_cached(
            &events,
            &claude,
            &copilot,
            repo.path(),
            "memo-run",
            &builtin(),
        );
        assert!((refreshed.cost.unwrap().usd - 2.0).abs() < 1e-9);
    }
}
