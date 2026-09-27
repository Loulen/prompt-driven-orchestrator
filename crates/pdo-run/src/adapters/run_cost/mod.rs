//! Harness-aware USD cost of a Run, derived on read as common contributions.
//! Claude contributions use per-message token `usage` from each parent session
//! transcript plus its subagents. Copilot contributions preserve the reported
//! cumulative cost from its session journal. Unknown harnesses retain their
//! executions and an absence reason without inventing a zero cost.
//!
//! ## Two forms, ventilated by harness (ADR-0052)
//!
//! That transcript-derived path is `claude`'s — a **derived** cost. A second
//! first-party harness, `copilot`, counts its own cost and PDO converts it by a
//! **published constant** ([`pdo_harness::copilot_journal`]) — a **reported** cost, read
//! by session identity, never through the price table (so it can never flag an
//! unpriced model). One attributed fold pairs the two into one summable dollar
//! total that *says* itself per harness (`CostStat::by_harness`): "X via `copilot`,
//! Y via `claude`". A third first-party harness, `pi`, reports a cost **already in
//! dollars** per message (`usage.cost.total`, [`pdo_harness::pi_session`]) — a reported
//! cost of constant 1.0, flagged `reported_in_usd` so the surfaces drop the `~`;
//! a pi message with tokens but no cost makes its node's total unavailable, never
//! `$0` (#707, ADR-0052 §2 amended). A harness with no cost source at all
//! (`opencode`) still makes
//! the whole aggregate "—" with a reason (never `$0`), as before — but only the
//! **total** goes: the per-harness slices are computed and carried alongside the
//! absence, so a Run mixing `opencode` with instrumented harnesses still says where
//! its known dollars came from (#617 FP, ADR-0052 §3).
//!
//! The price table is **injected** by the caller at the request edge. There is
//! deliberately NO N-1-argument wrapper meaning "the embedded prices": the next
//! call site added would silently ignore both disk tiers, uncatchable by test.
//!
//! The `projects_root` is injected by the caller (the #408 observability seam,
//! [`pdo_sandbox::adapters::staging::stores::transcripts_root`]): `~/.claude/projects/` for an
//! `off`/archived run, the staged home while a sandboxed run is live. This
//! module never reads `$HOME` — one root in, path-math + `std::fs` out. The
//! host-home stores of the reported-cost harnesses travel together as a
//! [`HarnessStores`] (`copilot`'s journal store, `pi`'s sessions store).
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

mod breakdown;
mod executions;
mod memo;
mod reported;
mod slices;

pub use crate::domain::run_cost::{
    frozen_execution_identity, CostContribution, CostScope, FrozenExecutionIdentity,
    ModelEffortSlice, RunCostBreakdown,
};
pub use breakdown::{compute_run_cost_breakdown, cost_unavailable};
pub use memo::{
    compute_run_cost_breakdown_cached, event_fingerprint, max_transcript_mtime_millis,
    reported_stores_mtime_millis,
};

use crate::domain::prices::PriceTable;
use crate::domain::run_cost::usage::{aggregate_by_model, parse_line, Line, ModelUsage};
#[cfg(test)]
use crate::domain::run_cost::usage::{line_cost, Usage};
use crate::domain::state::{CostForm, CostStat, HarnessCost};
use executions::{collect_executions, collect_jsonl_file, collect_jsonl_paths, Execution};
use pdo_harness::claude_transcript::cc_project_dirname;
use pdo_sandbox::adapters::staging::stores::HarnessStores;
use reported::reported_execution;
use slices::{execution_model_slices, leftover_model_slices, DerivedFold};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Dedup by `(message.id, requestId)`, keeping the first. Lines without a
/// `message.id` are always counted (no key to dedup on).
///
/// Unknown model ids are **de-dated** before collection, so a dated id and its
/// family name one offender, not two — the same key a human would add to
/// `models.yaml`. `partial` is **derived** from that set, so no caller can flip
/// one without the other. `<synthetic>` is priced $0 by the table (never
/// `None`), so it never lands here.
#[cfg(test)]
fn aggregate(lines: impl Iterator<Item = Line>, prices: &PriceTable) -> CostStat {
    let mut seen = HashSet::new();
    aggregate_with_seen(lines, prices, &mut seen)
}

/// The run-level aggregate alone — the per-model ventilation is only consumed
/// by the breakdown's execution loop, so the run-cost helpers keep this shape.
#[cfg(test)]
fn aggregate_with_seen(
    lines: impl Iterator<Item = Line>,
    prices: &PriceTable,
    seen: &mut HashSet<(String, Option<String>)>,
) -> CostStat {
    aggregate_by_model(lines, prices, seen).0
}

/// The distinct harnesses this Run launched a node on that have **no cost
/// source**. Read off the `NodeStarted` payloads (the frozen-at-spawn harness,
/// ADR-0046), never the current YAML. A `null` harness is the `claude` floor,
/// which HAS a cost source, so it never lands here.
#[cfg(test)]
pub fn uncosted_harnesses(events: &[crate::domain::event::Event]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for e in events {
        if e.kind != crate::domain::event::EventKind::NodeStarted {
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
        if !pdo_harness::probes::can_cost(harness) {
            names.insert(harness.to_string());
        }
    }
    names.into_iter().collect()
}

/// The `copilot` session ids this Run launched a node on, latest per
/// `(node_id, iter)` so a same-iter restart resolves to the fresh id. Only a
/// `NodeStarted` with a frozen `copilot` harness AND a non-empty session id
/// contributes, so an infra or other-harness journal is never attributed here.
#[cfg(test)]
fn copilot_session_ids(events: &[crate::domain::event::Event]) -> Vec<String> {
    // Last `NodeStarted` per (node, iter) wins (a restart re-pins a fresh id).
    let mut latest: BTreeMap<(String, i64), String> = BTreeMap::new();
    for e in events {
        if e.kind != crate::domain::event::EventKind::NodeStarted {
            continue;
        }
        let Some(payload) = e.payload.as_ref() else {
            continue;
        };
        let harness = payload.get("harness").and_then(|v| v.as_str());
        if harness != Some(pdo_harness::registry::COPILOT) {
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
#[cfg(test)]
fn copilot_reported_cost(
    events: &[crate::domain::event::Event],
    stores: &HarnessStores,
) -> Option<f64> {
    let mut usd = 0.0;
    let mut any = false;
    for sid in copilot_session_ids(events) {
        let journal = stores.copilot.join(&sid).join("events.jsonl");
        let Ok(text) = std::fs::read_to_string(&journal) else {
            continue;
        };
        if let Some(c) = pdo_harness::copilot_journal::reported_cost_usd(&text) {
            usd += c;
            any = true;
        }
    }
    any.then_some(usd)
}

/// Assemble the Run's cost, **ventilated by harness** (ADR-0052 §3). `None` when
/// neither harness contributed a cost — the surfaces' "—".
#[cfg(test)]
fn ventilate(
    claude: Option<CostStat>,
    events: &[crate::domain::event::Event],
    stores: &HarnessStores,
) -> Option<CostStat> {
    let copilot_usd = copilot_reported_cost(events, stores);
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
            harness: pdo_harness::registry::CLAUDE.to_string(),
            usd: c.usd,
            form: CostForm::Derived,
            partial: c.partial,
            unpriced_models: c.unpriced_models.clone(),
            reported_in_usd: false,
        });
    }
    if let Some(usd) = copilot_usd {
        total_usd += usd;
        by_harness.push(HarnessCost {
            harness: pdo_harness::registry::COPILOT.to_string(),
            usd,
            form: CostForm::Reported,
            // A reported cost never consults the price table, so it can never
            // be a lower bound nor name an unpriced model (ADR-0052 §2).
            partial: false,
            unpriced_models: Vec::new(),
            reported_in_usd: false,
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

#[cfg(test)]
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
#[cfg(test)]
pub fn run_cost_or_absence(
    events: &[crate::domain::event::Event],
    claude_root: &Path,
    stores: &HarnessStores,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> Option<CostStat> {
    let uncosted = uncosted_harnesses(events);
    let claude = compute_run_cost(claude_root, repo_root, run_id, prices);
    let ventilated = ventilate(claude, events, stores);
    if !uncosted.is_empty() {
        return Some(cost_unavailable(uncosted, slices_or_empty(ventilated)));
    }
    ventilated
}

/// The recursion is what captures subagent transcripts nested at
/// `<project>/<uuid>/subagents/*.jsonl`; dedup by `message.id` makes a
/// double-count with the parent impossible.
#[cfg(test)]
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
#[cfg(test)]
pub fn compute_run_cost(
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

    // --- parse_line ---

    fn assistant(id: &str, req: &str, model: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"{req}","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn copilot_nano_aiu_converts_by_the_published_constant() {
        assert!(
            (pdo_harness::copilot_journal::nano_aiu_to_usd(2_823_580_000) - 0.0282358).abs()
                < 1e-12
        );
        assert!(
            (pdo_harness::copilot_journal::nano_aiu_to_usd(100_000_000_000) - 1.0).abs() < 1e-12
        );
        assert_eq!(pdo_harness::copilot_journal::nano_aiu_to_usd(0), 0.0);
    }

    #[test]
    fn copilot_reported_cost_reads_the_max_total_and_is_none_when_absent() {
        let journal = concat!(
            "{\"type\":\"session.usage_checkpoint\",\"data\":{\"totalNanoAiu\":2823580000}}\n",
            "torn json\n",
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":2000000000}}\n",
            "{\"type\":\"session.shutdown\",\"data\":{\"totalNanoAiu\":3000000000}}\n"
        );
        assert!(
            (pdo_harness::copilot_journal::reported_cost_usd(journal).unwrap() - 0.03).abs()
                < 1e-12
        );
        assert!(pdo_harness::copilot_journal::reported_cost_usd(
            "{\"type\":\"assistant.turn_start\"}\n"
        )
        .is_none());
        assert!(pdo_harness::copilot_journal::reported_cost_usd("").is_none());
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

    use crate::domain::event::{Event, EventKind};

    fn node_started(node: &str, harness: Option<&str>) -> Event {
        Event {
            id: None,
            run_id: "r".into(),
            ts: pdo_kernel::clock::now_iso(),
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        std::fs::create_dir_all(&projects).unwrap();
        let repo = tempfile::tempdir().unwrap();
        let events = vec![node_started("n", Some("opencode"))];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &stores,
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
            ts: pdo_kernel::clock::now_iso(),
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "claude-run";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let events = vec![node_started("n", Some("claude")), node_started("s", None)];

        let honest =
            run_cost_or_absence(&events, &projects, &stores, repo.path(), run_id, &builtin())
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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

        let cost =
            run_cost_or_absence(&events, &projects, &stores, repo.path(), run_id, &builtin())
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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

        let cost =
            run_cost_or_absence(&events, &projects, &stores, repo.path(), run_id, &builtin())
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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

        let cost =
            run_cost_or_absence(&events, &projects, &stores, repo.path(), run_id, &builtin())
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "sid-live", 100_000_000_000); // $1.00
        let events = vec![node_started_sid("p", "copilot", "sid-live")];

        let cost = run_cost_or_absence(
            &events,
            &projects,
            &stores,
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "sid-mine", 100_000_000_000); // $1.00, this run
        seed_copilot_journal(&copilot, "sid-stranger", 900_000_000_000); // $9.00, NOT this run
        let events = vec![node_started_sid("p", "copilot", "sid-mine")];

        let cost = run_cost_or_absence(&events, &projects, &stores, repo.path(), "run", &builtin())
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "copilot",
                "session_id": sid
            })),
        };
        let events = vec![started("sid-first"), started("sid-restart")];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), "r", &builtin());
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "resurrected-pane";

        let node_dir = pdo_workspace::adapters::git::worktrees::sub_worktree_path(
            repo.path(),
            run_id,
            "claude",
            1,
        );
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
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type":"agent", "isolated_worktree":true,
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
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let start = Event {
            id: None,
            run_id: "legacy-restarts".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({ "node_type":"agent", "isolated_worktree":false })),
        };

        let breakdown = compute_run_cost_breakdown(
            &[start.clone(), start],
            &claude,
            &stores,
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": true,
                "harness": "claude",
                "session_id": "sid"
            })),
        }];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());

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
    fn claude_fold_deduplicates_replayed_parent_and_subagent_messages_run_wide() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "dedup";
        let working_dir = pdo_workspace::adapters::git::worktrees::sub_worktree_path(
            repo.path(),
            run_id,
            "worker",
            1,
        );
        let project = claude.join(cc_project_dirname(&working_dir));
        std::fs::create_dir_all(project.join("first/subagents")).unwrap();
        let replay = format!(
            "{}\n",
            assistant("same", "request", "claude-opus-4-8", 1_000_000, 0)
        );
        std::fs::write(project.join("first.jsonl"), &replay).unwrap();
        std::fs::write(project.join("first/subagents/side.jsonl"), &replay).unwrap();
        std::fs::write(project.join("second.jsonl"), &replay).unwrap();
        let started = |sid: &str| Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": true,
                "harness": "claude",
                "session_id": sid
            })),
        };

        let breakdown = compute_run_cost_breakdown(
            &[started("first"), started("second")],
            &claude,
            &stores,
            repo.path(),
            run_id,
            &builtin(),
        );

        assert!((breakdown.cost.unwrap().usd - 5.0).abs() < 1e-9);
    }

    #[test]
    fn uncosted_harness_withholds_total_but_keeps_costable_slices() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "known", 100_000_000_000);
        let events = vec![
            node_started_sid("known", "copilot", "known"),
            node_started("unknown", Some("opencode")),
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), "r", &builtin());
        let cost = breakdown.cost.unwrap();

        assert_eq!(cost.usd, 0.0);
        assert_eq!(cost.uncosted_harnesses, vec!["opencode"]);
        assert_eq!(cost.by_harness.len(), 1);
        assert!((cost.by_harness[0].usd - 1.0).abs() < 1e-9);
    }

    #[test]
    fn claude_cost_outside_node_sessions_is_infrastructure() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::RunStarted,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "harness": "claude" })),
            },
            Event {
                id: None,
                run_id: run_id.into(),
                ts: pdo_kernel::clock::now_iso(),
                kind: EventKind::MergeResolverStarted,
                node_id: None,
                iter: None,
                payload: None,
            },
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());

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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let event = |kind| Event {
            id: None,
            run_id: "copilot-infra".into(),
            ts: pdo_kernel::clock::now_iso(),
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
            &stores,
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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
            ts: pdo_kernel::clock::now_iso(),
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
                {"id":"legacy-agent","node_type":"agent", "isolated_worktree":false},
                {"id":"deterministic","node_type":"script"}
            ]
        }));
        let events = vec![
            started,
            event(Some("legacy-agent"), Some("agent")),
            event(Some("deterministic"), None),
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());

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
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
        let started = |node: &str, harness: &str, sid: &str, isolated: bool| Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some(node.into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent",
                "isolated_worktree": isolated,
                "harness": harness,
                "session_id": sid
            })),
        };
        let events = vec![
            started("claude-node", "claude", "sid-claude", true),
            started("copilot-node", "copilot", "sid-copilot", false),
        ];

        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let run_cost = breakdown.cost.clone().unwrap();
        let node_sum: f64 = breakdown
            .contributions
            .iter()
            .filter(|contribution| contribution.scope == CostScope::Node)
            .filter_map(|contribution| contribution.usd)
            .sum();
        let non_node_sum: f64 = breakdown
            .contributions
            .iter()
            .filter(|contribution| contribution.scope != CostScope::Node)
            .filter_map(|contribution| contribution.usd)
            .sum();
        assert!((run_cost.usd - node_sum - non_node_sum).abs() < 1e-9);
        assert_eq!(run_cost.by_harness.len(), 2);
        assert!((run_cost.usd - 7.0).abs() < 1e-9);
    }

    #[test]
    fn contribution_memo_refreshes_when_a_reported_cost_journal_changes() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
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
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type":"agent", "isolated_worktree":false,
                "harness":"copilot",
                "session_id":"memo-session"
            })),
        }];

        let first = compute_run_cost_breakdown_cached(
            &events,
            &claude,
            &stores,
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
            &stores,
            repo.path(),
            "memo-run",
            &builtin(),
        );
        assert!((refreshed.cost.unwrap().usd - 2.0).abs() < 1e-9);
    }

    // --- model × effort slices (ADR-0065, #735) ---

    /// Seed a claude transcript attributed to session `s1` (the file IS the
    /// identity: `<session-id>.jsonl` inside the encoded cwd dir).
    fn seed_session_transcript(projects: &Path, repo: &Path, run_id: &str, body: &str) {
        let worktree = repo.join(".pdo").join("runs").join(run_id).join("worktree");
        let proj = projects.join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("s1.jsonl"), format!("{body}\n")).unwrap();
    }

    fn claude_start(run_id: &str, model: Option<&str>, effort: Option<&str>) -> Event {
        Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "claude", "session_id": "s1",
                "model": model, "effort": effort
            })),
        }
    }

    #[test]
    fn a_two_model_transcript_ventilates_cost_and_executions_per_model() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "two-models";
        // opus-4-8 5/25 and sonnet-5 3/15: two observed models in one session.
        seed_session_transcript(
            &claude,
            repo.path(),
            run_id,
            &format!(
                "{}\n{}",
                assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
                assistant("m2", "r2", "claude-sonnet-5", 1_000_000, 0)
            ),
        );

        let events = vec![claude_start(run_id, Some("claude-opus-4-8"), Some("high"))];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();

        assert_eq!(node.model_slices.len(), 2, "one slice per observed model");
        let opus = node
            .model_slices
            .iter()
            .find(|slice| slice.model == "claude-opus-4-8")
            .unwrap();
        let sonnet = node
            .model_slices
            .iter()
            .find(|slice| slice.model == "claude-sonnet-5")
            .unwrap();
        // Verbatim ids (no de-dating), observed provenance, requested effort.
        assert!((opus.usd.unwrap() - 5.0).abs() < 1e-9);
        assert!((sonnet.usd.unwrap() - 3.0).abs() < 1e-9);
        for slice in [opus, sonnet] {
            assert!(slice.model_observed);
            assert_eq!(slice.effort.as_deref(), Some("high"));
            assert_eq!(slice.effort_observed, Some(false));
            assert_eq!(slice.executions, 1, "the execution counts in each bucket");
            assert!(slice.estimated);
            assert!(!slice.partial);
        }
        assert!((node.usd.unwrap() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn an_unpriced_observed_model_is_its_own_marked_slice() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "unpriced-slice";
        seed_session_transcript(
            &claude,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-fable-9-20260101", 1_000_000, 0),
        );
        let events = vec![claude_start(run_id, Some("claude-fable-9-20260101"), None)];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert_eq!(node.model_slices.len(), 1);
        let slice = &node.model_slices[0];
        assert_eq!(
            slice.model, "claude-fable-9-20260101",
            "verbatim, never de-dated"
        );
        assert_eq!(slice.usd, Some(0.0));
        assert!(slice.partial);
        assert_eq!(slice.unpriced_models, vec!["claude-fable-9"]);
    }

    #[test]
    fn a_mute_source_falls_back_to_the_requested_model_and_marks_it() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "mute-source";
        // No transcript for the claude node → the startup event's id in fallback,
        // carrying the execution's unreadability.
        let events = vec![claude_start(run_id, Some("sonnet"), Some("medium"))];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert_eq!(node.model_slices.len(), 1);
        let slice = &node.model_slices[0];
        assert_eq!(slice.model, "sonnet");
        assert!(!slice.model_observed, "the source was mute: requested");
        assert_eq!(slice.effort.as_deref(), Some("medium"));
        assert_eq!(slice.effort_observed, Some(false));
        assert_eq!(slice.usd, None);
        assert_eq!(
            slice.missing_reasons,
            vec!["no attributable Claude transcript"]
        );
    }

    #[test]
    fn a_reported_slice_falls_back_to_the_requested_model_without_estimating() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        seed_copilot_journal(&copilot, "s1", 100_000_000_000);
        let events = vec![Event {
            id: None,
            run_id: "reported-fallback".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "copilot", "session_id": "s1",
                "model": "gpt-5", "effort": "high"
            })),
        }];
        let breakdown = compute_run_cost_breakdown(
            &events,
            &claude,
            &stores,
            repo.path(),
            "reported-fallback",
            &builtin(),
        );
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert_eq!(node.model_slices.len(), 1);
        let slice = &node.model_slices[0];
        assert_eq!(slice.model, "gpt-5");
        assert!(
            !slice.model_observed,
            "the journal named no model: requested"
        );
        assert!((slice.usd.unwrap() - 1.0).abs() < 1e-9);
        assert!(!slice.estimated, "a reported cost is never a ~estimate");
    }

    /// Write a `pi` session file for `session_id` under `pi_root`, in the folder
    /// named after the Run's shared worktree — the resolution
    /// [`pdo_harness::adapters::transcripts::pi_session_by_id`] globs.
    fn seed_pi_session(pi_root: &Path, repo: &Path, run_id: &str, session_id: &str, body: &str) {
        let working_dir = repo.join(".pdo").join("runs").join(run_id).join("worktree");
        let dir = pi_root.join(pdo_harness::pi_session::session_dir_name(&working_dir));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("2026-09-07T10-00-00-000Z_{session_id}.jsonl")),
            body,
        )
        .unwrap();
    }

    #[test]
    fn a_pi_session_ventilates_its_reported_cost_by_observed_model_provider_and_effort() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "pi-observed";
        // Two priced messages on the same model/level, then a level change and a
        // third message: two observed slices, both carrying the provider.
        let message = |id: &str, cost: f64| {
            format!(
                r#"{{"type":"message","id":"{id}","message":{{"role":"assistant","model":"~anthropic/claude-haiku-latest","provider":"openrouter","usage":{{"input":100,"output":10,"totalTokens":110,"cost":{{"total":{cost}}}}},"stopReason":"stop"}}}}"#
            )
        };
        seed_pi_session(
            &stores.pi,
            repo.path(),
            run_id,
            "sid-pi",
            &format!(
                "{}\n{}\n{}\n{}\n{}\n",
                r#"{"type":"model_change","provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#,
                r#"{"type":"thinking_level_change","thinkingLevel":"low"}"#,
                message("m1", 0.10),
                r#"{"type":"thinking_level_change","thinkingLevel":"high"}"#,
                message("m2", 0.20),
            ),
        );
        let events = vec![Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "pi", "session_id": "sid-pi",
                "model": "sonnet", "effort": "medium"
            })),
        }];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert_eq!(node.usd, Some(0.30000000000000004), "the reported total");
        assert_eq!(node.model_slices.len(), 2, "one slice per observed effort");
        assert_eq!(node.model_slices[0].model, "~anthropic/claude-haiku-latest");
        assert!(node.model_slices[0].model_observed);
        assert_eq!(node.model_slices[0].effort.as_deref(), Some("low"));
        assert_eq!(node.model_slices[0].effort_observed, Some(true));
        assert_eq!(node.model_slices[0].provider.as_deref(), Some("openrouter"));
        assert_eq!(node.model_slices[0].usd, Some(0.10));
        assert!(!node.model_slices[0].estimated, "reported, not derived");
        assert_eq!(node.model_slices[1].effort.as_deref(), Some("high"));
        assert_eq!(node.model_slices[1].usd, Some(0.20));
        // The requested values are NOT used: the source spoke for both halves.
    }

    #[test]
    fn a_pi_session_without_a_readable_cost_keeps_its_observed_models_at_dash() {
        // The #707 hazard, now on the axis: tokens without a cost make the
        // dollars unavailable, but the models are still observed (#736).
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "pi-unpriced";
        seed_pi_session(
            &stores.pi,
            repo.path(),
            run_id,
            "sid-pi",
            &format!(
                "{}\n{}\n",
                r#"{"type":"thinking_level_change","thinkingLevel":"low"}"#,
                r#"{"type":"message","id":"m1","message":{"role":"assistant","model":"~zhipu/glm-4.7-flash","provider":"zhipu","usage":{"input":100,"output":10,"totalTokens":110,"cost":{"total":0}}},"stopReason":"stop"}"#
            ),
        );
        let events = vec![node_started_sid("worker", "pi", "sid-pi")];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert_eq!(node.usd, None);
        assert_eq!(node.model_slices.len(), 1);
        let slice = &node.model_slices[0];
        assert_eq!(slice.model, "~zhipu/glm-4.7-flash");
        assert!(slice.model_observed);
        assert_eq!(slice.usd, None, "unavailable, never $0");
        assert!(slice.missing_reasons[0].contains("catalogue absent"));
        assert_eq!(slice.provider.as_deref(), Some("zhipu"));
    }

    #[test]
    fn a_copilot_journal_ventilates_its_reported_cost_by_observed_model_and_effort() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let copilot = home.path().join(".copilot").join("session-state");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let dir = copilot.join("s1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("events.jsonl"),
            format!(
                "{}\n{}\n{}\n{}\n{}\n",
                r#"{"type":"session.start","data":{"selectedModel":"gpt-5.6-sol","reasoningEffort":"medium"}}"#,
                r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":10000000000,"modelCacheState":[{"modelId":"gpt-5.6-sol"}]}}"#,
                r#"{"type":"session.model_change","data":{"newModel":"claude-sonnet-4-5","reasoningEffort":"high"}}"#,
                r#"{"type":"session.usage_checkpoint","data":{"totalNanoAiu":25000000000,"modelCacheState":[{"modelId":"claude-sonnet-4-5"}]}}"#,
                r#"{"type":"session.shutdown","data":{"totalNanoAiu":25000000000}}"#,
            ),
        )
        .unwrap();
        let events = vec![Event {
            id: None,
            run_id: "copilot-observed".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "copilot", "session_id": "s1",
                "model": "gpt-5", "effort": "low"
            })),
        }];
        let breakdown = compute_run_cost_breakdown(
            &events,
            &claude,
            &stores,
            repo.path(),
            "copilot-observed",
            &builtin(),
        );
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert!(
            (node.usd.unwrap() - 0.25).abs() < 1e-9,
            "the reported total"
        );
        assert_eq!(node.model_slices.len(), 2, "one slice per observed model");
        assert_eq!(node.model_slices[0].model, "gpt-5.6-sol");
        assert!(node.model_slices[0].model_observed);
        assert_eq!(node.model_slices[0].effort.as_deref(), Some("medium"));
        assert_eq!(node.model_slices[0].effort_observed, Some(true));
        assert_eq!(node.model_slices[0].provider, None);
        assert!((node.model_slices[0].usd.unwrap() - 0.10).abs() < 1e-12);
        assert!(!node.model_slices[0].estimated, "reported, not derived");
        assert_eq!(node.model_slices[1].model, "claude-sonnet-4-5");
        assert_eq!(node.model_slices[1].effort.as_deref(), Some("high"));
        assert!((node.model_slices[1].usd.unwrap() - 0.15).abs() < 1e-12);
    }

    #[test]
    fn a_harness_without_cost_source_has_no_model_slices() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        // opencode pinned to a model: absent from « By model », never a
        // "default of X" bucket (#735 AC).
        let events = vec![Event {
            id: None,
            run_id: "no-source".into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false,
                "harness": "opencode", "model": "claude-opus-4-8"
            })),
        }];
        let breakdown = compute_run_cost_breakdown(
            &events,
            &claude,
            &stores,
            repo.path(),
            "no-source",
            &builtin(),
        );
        let node = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Node)
            .unwrap();
        assert!(node.model_slices.is_empty());
        assert_eq!(node.usd, None);
    }

    #[test]
    fn unattributable_leftover_ventilates_by_observed_model_with_no_effort() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude").join("projects");
        let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home.path());
        let repo = tempfile::tempdir().unwrap();
        let run_id = "orphan-models";
        // A transcript with no attributable node (no session id, non-isolated):
        // the lines end up Unassigned, but their models are still observed.
        seed_transcript(
            &claude,
            repo.path(),
            run_id,
            &format!(
                "{}\n{}\n",
                assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
                assistant("m2", "r2", "claude-sonnet-5", 1_000_000, 0)
            ),
        );
        let events = vec![Event {
            id: None,
            run_id: run_id.into(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::NodeStarted,
            node_id: Some("worker".into()),
            iter: Some(1),
            payload: Some(serde_json::json!({
                "node_type": "agent", "isolated_worktree": false, "harness": "claude"
            })),
        }];
        let breakdown =
            compute_run_cost_breakdown(&events, &claude, &stores, repo.path(), run_id, &builtin());
        let unassigned = breakdown
            .contributions
            .iter()
            .find(|contribution| contribution.scope == CostScope::Unassigned)
            .expect("orphan lines stay unassigned");
        assert_eq!(unassigned.model_slices.len(), 2);
        for slice in &unassigned.model_slices {
            assert!(slice.model_observed);
            assert_eq!(slice.effort, None, "no startup event: effort not set");
            assert_eq!(slice.effort_observed, None);
            assert_eq!(slice.executions, 0);
        }
    }
}
