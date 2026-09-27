//! The read-side memo of a Run's cost breakdown, keyed by everything that can
//! change it: the transcripts' and the reported stores' mtimes, the price
//! table, the event log and the roots.

use super::*;

// Read-side memo for `/stats/cost`, which fans over every Run in the cohort. It
// stores the complete harness contribution breakdown, never a superseded scalar.
// Nothing is persisted.
const BREAKDOWN_MEMO_CAP: usize = 4096;

/// Mirrors [`collect_jsonl_recursive`]'s traversal but `stat`s only — no file
/// contents are read.
pub fn max_mtime_recursive(dir: &Path, max_ms: &mut i64) {
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
pub fn max_transcript_mtime_millis(projects_root: &Path, repo_root: &Path, run_id: &str) -> i64 {
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

pub fn breakdown_memo() -> &'static Mutex<BreakdownMemoMap> {
    BREAKDOWN_MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The reported-cost mirror of [`max_transcript_mtime_millis`]: the newest mtime of
/// every `copilot` journal and `pi` session file the Run's executions resolve to
/// (#615/#707). `pub(crate)` so a whole-cohort cache key (`stats_performance`'s) can
/// fold a Run's reported-store contribution alongside its Claude one without
/// re-deriving the lookup. Resolution is the fold's own ([`collect_executions`] +
/// the per-harness `resolve_transcript`), so the key tracks exactly the files the
/// fold reads.
pub fn reported_stores_mtime_millis(
    events: &[crate::domain::event::Event],
    claude_root: &Path,
    stores: &HarnessStores,
    repo_root: &Path,
    run_id: &str,
) -> i64 {
    collect_executions(events, claude_root, repo_root, run_id)
        .iter()
        .filter_map(|execution| {
            let session_id = execution.session_id.as_deref()?;
            let path = if execution.harness == pdo_harness::registry::COPILOT {
                stores.copilot.join(session_id).join("events.jsonl")
            } else if execution.harness == pdo_harness::registry::PI {
                pdo_harness::adapters::transcripts::resolve_transcript(
                    &execution.harness,
                    &stores.pi,
                    execution.working_dir.as_deref()?,
                    Some(session_id),
                )?
            } else {
                return None;
            };
            std::fs::metadata(path).and_then(|m| m.modified()).ok()
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
pub fn event_fingerprint(events: &[crate::domain::event::Event]) -> u64 {
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

pub fn compute_run_cost_breakdown_cached(
    events: &[crate::domain::event::Event],
    claude_root: &Path,
    stores: &HarnessStores,
    repo_root: &Path,
    run_id: &str,
    prices: &PriceTable,
) -> RunCostBreakdown {
    let mut roots = DefaultHasher::new();
    claude_root.hash(&mut roots);
    stores.copilot.hash(&mut roots);
    stores.pi.hash(&mut roots);
    repo_root.hash(&mut roots);
    let key = (
        run_id.to_string(),
        max_transcript_mtime_millis(claude_root, repo_root, run_id),
        reported_stores_mtime_millis(events, claude_root, stores, repo_root, run_id),
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
    let value = compute_run_cost_breakdown(events, claude_root, stores, repo_root, run_id, prices);
    let mut guard = breakdown_memo()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if guard.len() >= BREAKDOWN_MEMO_CAP {
        guard.clear();
    }
    guard.insert(key, value.clone());
    value
}
