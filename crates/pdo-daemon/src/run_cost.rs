//! Estimated USD cost of a Run (#272), derived on read from the per-message
//! token `usage` recorded in each session's Claude Code transcript
//! (`~/.claude/projects/<encoded-cwd>/*.jsonl`) × a hardcoded public price table.
//!
//! This is an **estimate, not an invoice**: it uses public list prices (no
//! enterprise discount), and any model absent from the table contributes $0 and
//! flips the `partial` flag (lower-bound signalling). It mirrors `LocStat`'s
//! "derived on read, never persisted" contract (see [`crate::event_log::CostStat`]),
//! and happens to be *more* durable than LOC: archival deletes the run branch
//! (so LOC → "—") but leaves `~/.claude/projects/` intact, so an archived run
//! still shows its cost.
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
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

// Source: https://platform.claude.com/docs/en/about-claude/pricing (fetched 2026-07-06).
// Per-MTok list prices `(family_key, input, output)`. UPDATE when Anthropic
// changes pricing or ships a model. Cache prices are DERIVED (write_5m = 1.25×in,
// write_1h = 2×in, read = 0.1×in) — verified universal across every current row.
// Match on the FULL family key: Opus 4.5–4.8 are $5/$25 but Opus 4.1/4.0 are
// $15/$75 — never a `starts_with("opus-4")` shortcut.
const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-5", 5.0, 25.0),
    ("claude-opus-4-1", 15.0, 75.0),
    ("claude-opus-4-0", 15.0, 75.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-5", 3.0, 15.0),
    ("claude-sonnet-4-0", 3.0, 15.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.80, 4.0),
    // NOTE: `claude-sonnet-5` intro price ($2/$10) expires 2026-08-31 → $3/$15.
    // PDO runs opus-4-8 today; add sonnet-5 (date-gated) only if it starts appearing.
];

/// Drop a trailing 8-digit date segment so a dated id resolves to its family
/// key: `claude-sonnet-4-5-20250929` → `claude-sonnet-4-5`. A version-only id is
/// returned unchanged.
fn strip_date_suffix(model: &str) -> &str {
    if let Some((head, tail)) = model.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head;
        }
    }
    model
}

/// Per-MTok `(input, output)` price for a model, or `None` for an unknown real
/// model (the caller then flags `partial` and the model contributes $0).
/// `<synthetic>` — CC's local/no-cost sentinel — is priced at $0, NOT treated as
/// unknown (so it never flips `partial`).
fn price_for(model: &str) -> Option<(f64, f64)> {
    if model == "<synthetic>" {
        return Some((0.0, 0.0));
    }
    let key = strip_date_suffix(model);
    PRICES
        .iter()
        .find(|(k, ..)| *k == key)
        .map(|(_, i, o)| (*i, *o))
}

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

/// Dedup by `(message.id, requestId)` (keep first), price each surviving line,
/// and flag `partial` when any line used a model absent from [`PRICES`]. Lines
/// without a `message.id` are always counted (no key to dedup on).
fn aggregate(lines: impl Iterator<Item = Line>) -> CostStat {
    let mut seen = std::collections::HashSet::new();
    let mut usd = 0.0;
    let mut partial = false;
    for l in lines {
        if let Some(id) = &l.message_id {
            if !seen.insert((id.clone(), l.request_id.clone())) {
                continue; // duplicate: replay on resume/compaction
            }
        }
        match price_for(&l.model) {
            Some((i, o)) => usd += line_cost(&l.usage, i, o),
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
pub fn cc_project_dirname(path: &Path) -> String {
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
/// `repo_root` must be the run's **effective** repo root (honours `target_repo`)
/// — pass the value the caller already resolved via `effective_repo_root`.
pub fn compute_run_cost(repo_root: &Path, run_id: &str) -> Option<CostStat> {
    let home = std::env::var("HOME").ok()?;
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    // Trailing '-' anchors the run_id: a run whose id is a lexical prefix of
    // another can't leak its sessions in (after run_id comes `-nodes`/`-worktree`).
    let prefix = format!("{}-", cc_project_dirname(&run_dir));
    let projects = Path::new(&home).join(".claude").join("projects");
    let mut lines = Vec::new();
    let mut found = false;
    for entry in std::fs::read_dir(&projects).ok()?.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        found = true;
        collect_jsonl_recursive(&entry.path(), &mut lines);
    }
    if !found {
        return None;
    }
    Some(aggregate(lines.into_iter()))
}

// --- Read-side memo for the aggregate cost path (#377 / ADR-0029) ------------
//
// The Stats modal's `/stats/cost` endpoint fans [`compute_run_cost`] out over
// every run in the visible period — the exact anti-fan-out ADR-0022 kept off the
// `/runs` list handler. This memo is ADR-0022's *sanctioned escape hatch*: a
// derive-on-read RAM cache keyed on `(run_id, max transcript mtime)`, NEVER
// persisted (no snapshot table, no metric-freezing event). A transcript change
// bumps the mtime and so invalidates the entry naturally. It is touched ONLY by
// [`compute_run_cost_cached`] (the aggregate path); `get_run`'s single-run read
// keeps calling [`compute_run_cost`] directly, so ADR-0022's per-read contract
// is byte-identical there.

/// Memo key: `(run_id, max transcript mtime in epoch millis)`. A transcript
/// change bumps the mtime, so the key changes and the old entry is bypassed.
type CostMemoKey = (String, i64);
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
pub fn max_transcript_mtime_millis(repo_root: &Path, run_id: &str) -> i64 {
    let Ok(home) = std::env::var("HOME") else {
        return 0;
    };
    let run_dir = repo_root.join(".pdo").join("runs").join(run_id);
    let prefix = format!("{}-", cc_project_dirname(&run_dir));
    let projects = Path::new(&home).join(".claude").join("projects");
    let Ok(entries) = std::fs::read_dir(&projects) else {
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
/// `(run_id, max transcript mtime)` (see the module memo above). Used only by
/// the `/stats/cost` aggregate (period-bounded fan-out); `get_run`'s single-run
/// path is deliberately left calling [`compute_run_cost`] directly so ADR-0022's
/// per-read contract is unchanged.
pub fn compute_run_cost_cached(repo_root: &Path, run_id: &str) -> Option<CostStat> {
    let key = (
        run_id.to_string(),
        max_transcript_mtime_millis(repo_root, run_id),
    );
    {
        let guard = cost_memo().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let value = compute_run_cost(repo_root, run_id);
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

    // --- strip_date_suffix ---

    #[test]
    fn strips_trailing_8_digit_date() {
        assert_eq!(
            strip_date_suffix("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            strip_date_suffix("claude-3-5-haiku-20241022"),
            "claude-3-5-haiku"
        );
    }

    #[test]
    fn leaves_version_only_id_untouched() {
        assert_eq!(strip_date_suffix("claude-opus-4-8"), "claude-opus-4-8");
        // A short numeric tail (not 8 digits) is not a date suffix.
        assert_eq!(strip_date_suffix("claude-opus-4-8"), "claude-opus-4-8");
    }

    // --- price_for ---

    #[test]
    fn prices_known_models() {
        assert_eq!(price_for("claude-opus-4-8"), Some((5.0, 25.0)));
        assert_eq!(price_for("claude-sonnet-4-5"), Some((3.0, 15.0)));
        assert_eq!(price_for("claude-haiku-4-5"), Some((1.0, 5.0)));
    }

    #[test]
    fn opus_4_1_and_4_0_are_not_collapsed_with_4_5_plus() {
        // The single most error-prone row: same "opus-4" prefix, different price.
        assert_eq!(price_for("claude-opus-4-1"), Some((15.0, 75.0)));
        assert_eq!(price_for("claude-opus-4-0"), Some((15.0, 75.0)));
        assert_ne!(price_for("claude-opus-4-1"), price_for("claude-opus-4-8"));
    }

    #[test]
    fn dated_id_resolves_to_family_price() {
        assert_eq!(price_for("claude-sonnet-4-5-20250929"), Some((3.0, 15.0)));
    }

    #[test]
    fn synthetic_is_zero_not_unknown() {
        assert_eq!(price_for("<synthetic>"), Some((0.0, 0.0)));
    }

    #[test]
    fn unknown_model_is_none() {
        assert_eq!(price_for("gpt-9"), None);
        assert_eq!(price_for("claude-opus-9-9"), None);
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
        let c = aggregate(lines.into_iter());
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
        let c = aggregate(lines.into_iter());
        assert!((c.usd - 10.0).abs() < 1e-9, "usd = {}", c.usd);
    }

    #[test]
    fn aggregate_flags_partial_on_unknown_model() {
        let lines = vec![
            line(Some("m1"), Some("r1"), "claude-opus-4-8", 1_000_000),
            line(Some("m2"), Some("r2"), "some-future-model", 1_000_000),
        ];
        let c = aggregate(lines.into_iter());
        // Only the priced line contributes; the unknown one flags partial + $0.
        assert!((c.usd - 5.0).abs() < 1e-9, "usd = {}", c.usd);
        assert!(c.partial);
    }

    #[test]
    fn aggregate_synthetic_does_not_flip_partial() {
        let lines = vec![line(Some("m1"), Some("r1"), "<synthetic>", 1_000_000)];
        let c = aggregate(lines.into_iter());
        assert_eq!(c.usd, 0.0);
        assert!(!c.partial);
    }

    // --- compute_run_cost (filesystem) ---

    /// RAII guard swapping HOME for a temp dir while holding the crate-wide HOME
    /// lock (mirrors `stale_detector::TempHome` / lib.rs `FakeHome`).
    struct TempHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        tmp: tempfile::TempDir,
        prev: Option<std::ffi::OsString>,
    }

    impl TempHome {
        fn new() -> Self {
            let lock = crate::library_store::HOME_TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tmp = tempfile::tempdir().unwrap();
            let prev = std::env::var_os("HOME");
            std::env::set_var("HOME", tmp.path());
            Self {
                _lock: lock,
                tmp,
                prev,
            }
        }

        fn path(&self) -> &Path {
            self.tmp.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn compute_run_cost_aggregates_and_dedups_across_sessions() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        let run_id = "20260706-abc-node";
        // A session cwd under the run dir (the worktree, where the manager runs).
        let worktree = repo
            .path()
            .join(".pdo")
            .join("runs")
            .join(run_id)
            .join("worktree");
        let proj = home
            .path()
            .join(".claude")
            .join("projects")
            .join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&proj).unwrap();

        let l1 = assistant("msg_1", "req_1", "claude-opus-4-8", 1000, 500);
        let l2 = assistant("msg_2", "req_2", "claude-opus-4-8", 2000, 1000);
        // l1 replayed (same msg_1, req_1) → deduped.
        std::fs::write(proj.join("s.jsonl"), format!("{l1}\n{l1}\n{l2}\n")).unwrap();

        let cost = compute_run_cost(repo.path(), run_id).unwrap();
        // (1000*5 + 500*25)/1e6 + (2000*5 + 1000*25)/1e6 = 0.0175 + 0.035 = 0.0525
        assert!((cost.usd - 0.0525).abs() < 1e-9, "usd = {}", cost.usd);
        assert!(!cost.partial);
    }

    #[test]
    fn compute_run_cost_recurses_into_subagents() {
        let home = TempHome::new();
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
        let proj = home
            .path()
            .join(".claude")
            .join("projects")
            .join(cc_project_dirname(&node));
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

        let cost = compute_run_cost(repo.path(), run_id).unwrap();
        // 1M input × $5/MTok, twice (main + subagent) = $10.
        assert!((cost.usd - 10.0).abs() < 1e-9, "usd = {}", cost.usd);
    }

    #[test]
    fn compute_run_cost_none_when_no_transcript_dir() {
        let _home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        assert!(compute_run_cost(repo.path(), "no-such-run").is_none());
    }

    // --- compute_run_cost_cached / memo (#377) ---

    /// Write a single-line transcript for `run_id`'s worktree and return the
    /// `.jsonl` path so the test can manipulate its mtime.
    fn seed_transcript(home: &Path, repo: &Path, run_id: &str, line: &str) -> std::path::PathBuf {
        let worktree = repo.join(".pdo").join("runs").join(run_id).join("worktree");
        let proj = home
            .join(".claude")
            .join("projects")
            .join(cc_project_dirname(&worktree));
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("s.jsonl");
        std::fs::write(&file, format!("{line}\n")).unwrap();
        file
    }

    #[test]
    fn cached_matches_uncached() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-eq";
        seed_transcript(
            home.path(),
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let direct = compute_run_cost(repo.path(), run_id);
        let cached = compute_run_cost_cached(repo.path(), run_id);
        assert_eq!(direct, cached);
        assert!((cached.unwrap().usd - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cached_re_serves_from_memo_when_mtime_is_unchanged() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-hit";
        let file = seed_transcript(
            home.path(),
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let orig =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap());

        // First call: memoize $5 under (run_id, mtime).
        let first = compute_run_cost_cached(repo.path(), run_id).unwrap();
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
        let hit = compute_run_cost_cached(repo.path(), run_id).unwrap();
        assert!((hit.usd - 5.0).abs() < 1e-9, "memo hit should re-serve $5");
        // But the uncached path sees the new content ($10) — proving the file
        // really changed and the hit above was the cache, not a recompute.
        let direct = compute_run_cost(repo.path(), run_id).unwrap();
        assert!((direct.usd - 10.0).abs() < 1e-9);
    }

    #[test]
    fn cached_recomputes_when_mtime_bumps() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        let run_id = "memo-bump";
        let file = seed_transcript(
            home.path(),
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1_000_000, 0),
        );
        let orig =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&file).unwrap());
        let first = compute_run_cost_cached(repo.path(), run_id).unwrap();
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
        let recomputed = compute_run_cost_cached(repo.path(), run_id).unwrap();
        assert!(
            (recomputed.usd - 10.0).abs() < 1e-9,
            "a bumped mtime must invalidate the memo"
        );
    }

    #[test]
    fn max_transcript_mtime_is_zero_without_a_transcript_and_positive_with_one() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        assert_eq!(
            max_transcript_mtime_millis(repo.path(), "no-such-run"),
            0,
            "no transcript dir → 0 (so a later write bumps the key)"
        );
        let run_id = "mtime-run";
        seed_transcript(
            home.path(),
            repo.path(),
            run_id,
            &assistant("m1", "r1", "claude-opus-4-8", 1000, 0),
        );
        assert!(max_transcript_mtime_millis(repo.path(), run_id) > 0);
    }

    #[test]
    fn compute_run_cost_prefix_does_not_leak_across_runs() {
        let home = TempHome::new();
        let repo = tempfile::tempdir().unwrap();
        // Two runs where one id is a lexical prefix of the other.
        let other = repo
            .path()
            .join(".pdo")
            .join("runs")
            .join("run-1x") // "run-1" is a prefix of "run-1x"
            .join("worktree");
        let proj = home
            .path()
            .join(".claude")
            .join("projects")
            .join(cc_project_dirname(&other));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("s.jsonl"),
            format!("{}\n", assistant("m", "r", "claude-opus-4-8", 1_000_000, 0)),
        )
        .unwrap();

        // Querying "run-1" must NOT pick up "run-1x"'s transcript.
        assert!(compute_run_cost(repo.path(), "run-1").is_none());
    }
}
