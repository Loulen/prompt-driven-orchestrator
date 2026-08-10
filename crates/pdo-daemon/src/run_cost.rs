//! Estimated USD cost of a Run (#272), derived on read from the per-message
//! token `usage` recorded in each session's Claude Code transcript
//! (`<projects_root>/<encoded-cwd>/*.jsonl`) × a public price table.
//!
//! Since #427 that table is **injected** too, as a [`crate::price_table::PriceTable`]
//! resolved by the caller at the request edge (`manual → fetched → embedded`, see
//! ADR-0034). There is deliberately NO N-1-argument wrapper meaning "the embedded
//! prices": the next call site added would silently ignore both disk tiers and no
//! test could catch it.
//!
//! The `projects_root` is injected by the caller (the #408 observability seam,
//! [`crate::sandbox_run::transcripts_root`]): `~/.claude/projects/` for an
//! `off`/archived run, the staged home while a sandboxed run is live. This
//! module never reads `$HOME` — one root in, path-math + `std::fs` out.
//!
//! This is an **estimate, not an invoice**: it uses public list prices (no
//! enterprise discount), and any model absent from the table contributes $0 and
//! flips the `partial` flag (lower-bound signalling). It mirrors `LocStat`'s
//! "derived on read, never persisted" contract (see [`crate::event_log::CostStat`]),
//! and happens to be *more* durable than LOC: archival deletes the run branch
//! (so LOC → "—") but leaves `~/.claude/projects/` intact (merge_back flushed a
//! sandboxed run's transcripts there at cleanup), so an archived run still shows
//! its cost.
//!
//! ## Correctness notes (each verified against real transcripts, ADR-0022)
//! - **Dedup is mandatory.** Claude Code replays assistant messages on
//!   resume/compaction, so the same message is written ~2.3× in a real
//!   transcript. We dedup by `(message.id, requestId)`, keeping the first — the
//!   `usage` is byte-identical within a group, so keep-one is exact (matches
//!   `ccusage`). Without it the number is 2–3× too high.
//! - **Path encoder.** [`cc_project_dirname`] maps a working dir to the name CC
//!   writes under `~/.claude/projects/`. Since #373 it delegates to the (now
//!   correct) [`crate::stale_detector::encode_working_dir`] — one source of
//!   truth. Historically it reimplemented the mapping to route around a bug in
//!   that function (it stripped the leading `/` and left `.` unmapped, so the
//!   stale-detector's mtime probe resolved `None` for every node).
//! - **Cache tokens don't overlap `input_tokens`.** CC's `input_tokens` excludes
//!   cache tokens, so the four buckets sum without subtraction (matches ccusage).
//! - **Tolerant parsing.** Torn writes (an interleaved-flush `clauclaude-opus-4-8`
//!   was observed) are skipped line-by-line, never `?`-propagated.

use crate::event_log::CostStat;
use crate::price_table::PriceTable;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// Token counts from one assistant message's `usage`. The four cache buckets are
/// disjoint from `input`/`output` (CC's `input_tokens` excludes cache tokens).
#[derive(Default)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create_5m: u64,
    cache_create_1h: u64,
}

/// One cost-bearing transcript line: its dedup key `(message_id, request_id)`,
/// its model, and its token usage.
struct Line {
    message_id: Option<String>,
    request_id: Option<String>,
    model: String,
    usage: Usage,
}

/// Cost of one line, in USD (the 5-term ccusage formula; `in_p`/`out_p` are the
/// per-MTok input/output list prices — cache is derived from `in_p`).
fn line_cost(u: &Usage, in_p: f64, out_p: f64) -> f64 {
    (u.input as f64 * in_p
        + u.output as f64 * out_p
        + u.cache_create_5m as f64 * in_p * 1.25
        + u.cache_create_1h as f64 * in_p * 2.0
        + u.cache_read as f64 * in_p * 0.1)
        / 1_000_000.0
}

/// Parse one transcript line into a cost-bearing [`Line`], or `None` to skip it.
/// Tolerant: a torn/invalid JSON line is skipped, never propagated. Only
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

/// Dedup by `(message.id, requestId)` (keep first), price each surviving line
/// against the resolved `prices` table, and flag `partial` when any line used a
/// model no tier knows. Lines without a `message.id` are always counted (no key
/// to dedup on).
fn aggregate(lines: impl Iterator<Item = Line>, prices: &PriceTable) -> CostStat {
    let mut seen = std::collections::HashSet::new();
    let mut usd = 0.0;
    let mut partial = false;
    for l in lines {
        if let Some(id) = &l.message_id {
            if !seen.insert((id.clone(), l.request_id.clone())) {
                continue; // duplicate: replay on resume/compaction
            }
        }
        match prices.price_for(&l.model) {
            Some(p) => usd += line_cost(&l.usage, p.input, p.output),
            None => partial = true, // unknown real model → $0 + lower-bound flag
        }
    }
    CostStat { usd, partial }
}

/// Encode an absolute path exactly as Claude Code names its `~/.claude/projects`
/// directory: every non-`[A-Za-z0-9]` char → `-`, case preserved, runs NOT
/// collapsed. So a leading `/` becomes a leading `-` and `.pdo` becomes `--pdo`.
/// Verified against real dirs: `/home/u/.pdo/runs/X/worktree` →
/// `-home-u--pdo-runs-X-worktree`.
///
/// Delegates to [`crate::stale_detector::encode_working_dir`], the single source
/// of truth for this encoding. (Historically this reimplemented the mapping to
/// route around a bug in that function; #373 fixed and unified them.)
pub(crate) fn cc_project_dirname(path: &Path) -> String {
    crate::stale_detector::encode_working_dir(path)
}

/// Recursively collect every parseable cost line from `*.jsonl` under `dir`.
/// The recursion captures subagent transcripts nested at
/// `<project>/<uuid>/subagents/*.jsonl` (D7); dedup by `message.id` makes any
/// resulting double-count with the parent impossible.
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

/// Estimated cost for a run: aggregate every CC transcript whose project dir is
/// under `<repo_root>/.pdo/runs/<run_id>/` (all nodes, the manager, the
/// merge-resolver, and their subagents). `None` when no such dir exists (UI
/// "—"); `Some { usd: 0.0, .. }` when dirs exist but carry no priced tokens.
///
/// `projects_root` is the Claude Code `projects/` root to read (the #408
/// observability seam — `~/.claude/projects/` for an `off`/archived run, the
/// staged home for a live sandboxed run). `repo_root` must be the run's
/// **effective** repo root (honours `target_repo`) — pass the value the caller
/// already resolved via `effective_repo_root`; it builds the run-id dir prefix,
/// NOT the read root.
/// `prices` is the table resolved at the request edge (#427) — mandatory; see the
/// module header on why there is no defaulting wrapper.
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

// --- Read-side memo for the aggregate cost path (#377 / ADR-0029) ------------
//
// The Stats modal's `/stats/cost` endpoint fans [`compute_run_cost`] out over
// every run in the visible period — the exact anti-fan-out ADR-0022 kept off the
// `/runs` list handler. This memo is ADR-0022's *sanctioned escape hatch*: a
// derive-on-read RAM cache keyed on `(run_id, max transcript mtime, price-table
// fingerprint)`, NEVER persisted (no snapshot table, no metric-freezing event). A
// transcript change bumps the mtime and so invalidates the entry naturally. It is
// touched ONLY by [`compute_run_cost_cached`] (the aggregate path); `get_run`'s
// single-run read keeps calling [`compute_run_cost`] directly, so ADR-0022's
// per-read contract is byte-identical there.
//
// The THIRD key component is load-bearing (#427, ADR-0034). A price sync bumps no
// transcript mtime, so under the old two-part key `/stats/cost` (memoized) would
// re-serve pre-sync dollars until the daemon restarted while `GET /runs/:id`
// (not memoized) told the truth — two surfaces contradicting each other on the
// same Run, and the affected Runs (finished, transcripts frozen) are exactly the
// ones a sync is meant to repair. A sync deliberately does NOT clear the memo:
// under the new key a stale entry is simply unreachable, and clearing would also
// invalidate Runs whose prices did not move.

/// Memo key: `(run_id, max transcript mtime in epoch millis, price-table
/// fingerprint)`. A transcript change bumps the mtime and a price change bumps the
/// fingerprint, so either one changes the key and bypasses the old entry.
///
/// Consequence to know: [`COST_MEMO_CAP`] now holds several entries per Run across
/// a table change. Overflow clears the whole map, which stays
/// correctness-preserving by construction.
type CostMemoKey = (String, i64, u64);
/// The memoized value is exactly what [`compute_run_cost`] returns (`None` = no
/// transcript dir), so a hit is byte-identical to a recompute.
type CostMemoMap = HashMap<CostMemoKey, Option<CostStat>>;

static COST_MEMO: OnceLock<Mutex<CostMemoMap>> = OnceLock::new();

/// Soft cap on memo entries. On overflow the whole map is cleared — dropping the
/// cache is correctness-preserving (a miss just recomputes), so this bounds RAM
/// without pulling in an `lru` crate. `CostStat` is 16 bytes, so this is roomy.
const COST_MEMO_CAP: usize = 4096;

fn cost_memo() -> &'static Mutex<CostMemoMap> {
    COST_MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Recurse a project dir, folding the max `*.jsonl` mtime (epoch millis) into
/// `max_ms`. Mirrors [`collect_jsonl_recursive`]'s traversal but `stat`s only —
/// no file contents are read.
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

/// Max mtime (epoch millis) across every `*.jsonl` transcript that contributes
/// to `run_id`'s cost — the same recursive glob [`compute_run_cost`] aggregates.
/// `0` when no transcript dir/file exists yet (so a later write bumps the key and
/// invalidates the memo). A pure `stat` walk: no file contents are read, so it is
/// far cheaper than the aggregate it guards.
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

/// Memoized [`compute_run_cost`]: byte-identical result, cached on
/// `(run_id, max transcript mtime, price fingerprint)` (see the module memo
/// above). Used only by the `/stats/cost` aggregate (period-bounded fan-out);
/// `get_run`'s single-run path is deliberately left calling [`compute_run_cost`]
/// directly so ADR-0022's per-read contract is unchanged.
pub(crate) fn compute_run_cost_cached(
    projects_root: &Path,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> Option<CostStat> {
    // Load-bearing twice over: the SAME `projects_root` feeds the key (mtime) AND
    // the value (aggregate) — a mismatched root would desync the memo silently
    // (#408 P1) — and the SAME table feeds the fingerprint AND the pricing, so a
    // hit can never be a table the caller did not ask for (#427).
    let key = (
        run_id.to_string(),
        max_transcript_mtime_millis(projects_root, repo_root, run_id),
        prices.fingerprint(),
    );
    {
        let guard = cost_memo().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let value = compute_run_cost(projects_root, repo_root, run_id, prices);
    let mut guard = cost_memo().lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= COST_MEMO_CAP {
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
    }

    #[test]
    fn aggregate_synthetic_does_not_flip_partial() {
        let lines = vec![line(Some("m1"), Some("r1"), "<synthetic>", 1_000_000)];
        let c = aggregate(lines.into_iter(), &builtin());
        assert_eq!(c.usd, 0.0);
        assert!(!c.partial);
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

    // --- compute_run_cost_cached / memo (#377) ---

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
    fn cached_matches_uncached() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-eq";
        seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let direct = compute_run_cost(&projects, repo.path(), run_id, &builtin());
        let cached = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin());
        assert_eq!(direct, cached);
        assert!((cached.unwrap().usd - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cached_re_serves_from_memo_when_mtime_is_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-hit";
        let file = seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let orig =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap());

        // First call: memoize $5 under (run_id, mtime).
        let first = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert!((first.usd - 5.0).abs() < 1e-9);

        // Rewrite with a DIFFERENT cost ($10) but force the mtime back — the key
        // is unchanged, so the memo must re-serve the stale $5.
        std::fs::write(
            &file,
            format!(
                "{}\n",
                assistant("m2", "r2", "claude-opus-4-8", 2_000_000, 0)
            ),
        )
        .unwrap();
        filetime::set_file_mtime(&file, orig).unwrap();
        let hit = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert!((hit.usd - 5.0).abs() < 1e-9, "memo hit should re-serve $5");
        // But the uncached path sees the new content ($10) — proving the file
        // really changed and the hit above was the cache, not a recompute.
        let direct = compute_run_cost(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert!((direct.usd - 10.0).abs() < 1e-9);
    }

    #[test]
    fn cached_recomputes_when_only_the_price_table_changes() {
        // THE #427 regression test. A price sync bumps NO transcript mtime, so under
        // the old two-part key `/stats/cost` would re-serve pre-sync dollars until
        // the daemon restarted while `GET /runs/:id` showed the new ones — two
        // surfaces contradicting each other on a finished Run, which is exactly the
        // Run a sync exists to repair. Delete the third key component and this fails.
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-prices";
        let file = seed_transcript(
            &projects,
            repo.path(),
            run_id,
            // A model NO tier prices out of the box → $0 + partial, the very symptom.
            &assistant("m1", "r1", "claude-fable-5", 1_000_000, 0),
        );
        let mtime_before =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap());

        let unpriced = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert_eq!(unpriced.usd, 0.0);
        assert!(unpriced.partial, "an unknown model flags partial");

        // Now price it — a DIFFERENT table (different fingerprint), same transcript.
        let home2 = tempfile::tempdir().unwrap();
        let (manual, _) = crate::price_table::PriceTable::paths(home2.path());
        std::fs::create_dir_all(manual.parent().unwrap()).unwrap();
        std::fs::write(
            &manual,
            "models:\n  claude-fable-5: { input: 10.0, output: 50.0 }\n",
        )
        .unwrap();
        let synced = crate::price_table::PriceTable::load(home2.path());
        assert_ne!(synced.fingerprint(), builtin().fingerprint());

        // The transcript's mtime is untouched — only the table moved.
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap()),
            mtime_before
        );

        let repriced = compute_run_cost_cached(&projects, repo.path(), run_id, &synced).unwrap();
        assert!(
            (repriced.usd - 10.0).abs() < 1e-9,
            "the memo must MISS on a new price fingerprint, got ${}",
            repriced.usd
        );
        assert!(!repriced.partial, "the model is priced now");

        // And the old entry is still reachable under its own key — the sync does not
        // (and need not) clear the memo.
        let again = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert_eq!(again.usd, 0.0);
    }

    #[test]
    fn cached_recomputes_when_mtime_bumps() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join(".claude").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-bump";
        let file = seed_transcript(
            &projects,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let orig =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap());
        let first = compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert!((first.usd - 5.0).abs() < 1e-9);

        // New content AND a bumped mtime → new key → recompute picks up $10.
        std::fs::write(
            &file,
            format!(
                "{}\n",
                assistant("m2", "r2", "claude-opus-4-8", 2_000_000, 0)
            ),
        )
        .unwrap();
        let bumped = filetime::FileTime::from_unix_time(orig.unix_seconds() + 10, 0);
        filetime::set_file_mtime(&file, bumped).unwrap();
        let recomputed =
            compute_run_cost_cached(&projects, repo.path(), run_id, &builtin()).unwrap();
        assert!(
            (recomputed.usd - 10.0).abs() < 1e-9,
            "a bumped mtime must invalidate the memo"
        );
    }

    #[test]
    fn cached_honors_the_injected_projects_root() {
        // #408 P1: `compute_run_cost_cached` feeds the SAME `projects_root` to
        // both the mtime key AND the aggregate value — so the cached path honors
        // whichever root the seam picked (staging while live, host after cleanup).
        // Mirrors production, where the two roots for a run never share an mtime:
        // `merge_back` copies with `std::fs::copy` (no mtime preservation), so the
        // host file lands with a newer mtime than the staging original. We back-
        // date the host file to guarantee the two keys differ.
        let home = tempfile::tempdir().unwrap();
        let host = home.path().join(".claude").join("projects");
        let staging = home.path().join("staging").join("projects");
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-root";

        // Host root: $5 (back-dated). Staging root: $10 (fresh). Same run_id.
        let host_file = seed_transcript(
            &host,
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        filetime::set_file_mtime(
            &host_file,
            filetime::FileTime::from_unix_time(1_600_000_000, 0),
        )
        .unwrap();
        seed_transcript(
            &staging,
            repo.path(),
            run_id,
            &assistant("m2", "r2", "claude-opus-4-8", 2_000_000, 0),
        );

        let host_cost = compute_run_cost_cached(&host, repo.path(), run_id, &builtin()).unwrap();
        let staging_cost =
            compute_run_cost_cached(&staging, repo.path(), run_id, &builtin()).unwrap();
        assert!((host_cost.usd - 5.0).abs() < 1e-9, "host root → $5");
        assert!(
            (staging_cost.usd - 10.0).abs() < 1e-9,
            "staging root → $10 (its own key/value, not the host's memo)"
        );
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
}
