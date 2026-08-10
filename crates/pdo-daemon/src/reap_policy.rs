//! Pure reap policy (#480, #128 Track A) — decides which terminal Runs surfaced
//! by `GET /runs/reapable` a disk janitor should reclaim, and in what order.
//!
//! **Layer 1: no I/O, no clock.** The daemon computes each entry's `age_secs` at
//! the moment of the listing, so the policy is a pure function of that number and
//! the Run's status — deterministic and unit-testable with zero stubbing. The CLI
//! (`pdo reap`) does the I/O around it; the shipped `disk-janitor` pipeline's
//! `script` node drives the CLI (ADR-0017: a script node is testable end-to-end
//! in CI with no stub, unlike an agent node).
//!
//! Why a **graded TTL** rather than the original recipe's flat
//! `completed`-only / 7-day rule:
//!   - `completed` Runs are pure residue once old → reclaimable on a short TTL.
//!   - `failed` / `halted` / `skipped` Runs are post-mortem evidence → a *longer*
//!     TTL, but **not** infinite: excluding them outright leaks their worktrees
//!     forever (the `auto-issue-implement` class carries a multi-GB `target/`),
//!     so the leak is bounded, never unbounded.
//!   - The janitor's **own** completed Runs pile up one-per-fire (an hourly cron
//!     ⇒ 24 lingering worktrees + 24 immortal `__manager__` sessions, marching
//!     toward the ~30-session tmux collapse). A short self-TTL lets the janitor
//!     tidy after itself without ever touching its *live* Run — a `running` Run is
//!     never reapable, so the janitor cannot delete itself out from under its
//!     own feet.
//!
//! Live / archived Runs are never listed by the endpoint, but the policy defends
//! against them anyway rather than trusting its caller.

use crate::event_log::RunStatus;
use serde::Deserialize;

/// One entry of `GET /runs/reapable` — the subset the policy consumes. Extra
/// fields in the payload (e.g. `effective_repo`, `worktree_present`) are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct ReapableRun {
    pub run_id: String,
    #[serde(default)]
    pub pipeline_name: String,
    pub status: RunStatus,
    /// Seconds since the terminal transition, computed by the daemon at list
    /// time. `None` when the timestamp was absent/unparseable — such a Run is
    /// never reaped (no defensible age to compare a TTL against).
    #[serde(default)]
    pub age_secs: Option<i64>,
    /// Present only when the listing was requested with `?size=true`.
    #[serde(default)]
    pub approx_disk_bytes: Option<u64>,
}

/// TTLs (in seconds) a janitor applies before reclaiming a Run, by category.
#[derive(Debug, Clone)]
pub struct ReapPolicy {
    /// `completed` Runs older than this are reclaimed. Pure residue.
    pub completed_ttl_secs: i64,
    /// `failed` / `halted` / `skipped` Runs older than this are reclaimed.
    /// Longer than `completed_ttl_secs` (debugging evidence), but bounded.
    pub terminal_ttl_secs: i64,
    /// Runs whose `pipeline_name` equals this get `self_ttl_secs` instead of the
    /// per-status TTL, so the janitor reclaims its own past Runs quickly. `None`
    /// disables the fast lane.
    pub self_pipeline: Option<String>,
    /// TTL for `self_pipeline` Runs (any terminal status).
    pub self_ttl_secs: i64,
}

/// 24 h — a `completed` Run is residue; reclaim it a day after it finished.
pub const DEFAULT_COMPLETED_TTL_SECS: i64 = 24 * 3600;
/// 72 h — `failed`/`halted`/`skipped` are post-mortem evidence; hold them longer
/// (still more conservative than the measured human archive latency), but bound
/// the leak.
pub const DEFAULT_TERMINAL_TTL_SECS: i64 = 72 * 3600;
/// 1 h — the janitor's own past Runs are boring; tidy them fast so hourly fires
/// do not accumulate worktrees + immortal manager sessions.
pub const DEFAULT_SELF_TTL_SECS: i64 = 3600;
/// The name of the shipped janitor pipeline, used as the default self fast-lane.
pub const DEFAULT_SELF_PIPELINE: &str = "disk-janitor";

impl Default for ReapPolicy {
    fn default() -> Self {
        Self {
            completed_ttl_secs: DEFAULT_COMPLETED_TTL_SECS,
            terminal_ttl_secs: DEFAULT_TERMINAL_TTL_SECS,
            self_pipeline: Some(DEFAULT_SELF_PIPELINE.to_string()),
            self_ttl_secs: DEFAULT_SELF_TTL_SECS,
        }
    }
}

/// One reclaim the policy selected, with the reason it qualified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReapDecision {
    pub run_id: String,
    pub status: RunStatus,
    pub age_secs: i64,
    pub approx_disk_bytes: Option<u64>,
    pub reason: String,
}

/// The ordered set of reclaims plus a rollup.
#[derive(Debug, Clone, Default)]
pub struct ReapPlan {
    /// Runs to reclaim, **biggest-first** (so a wall-clock-bounded reclaim frees
    /// the most disk per second). Ties broken by `run_id` for determinism.
    pub reclaim: Vec<ReapDecision>,
    /// Terminal Runs that were listed but did not meet their TTL (too young, or
    /// no parseable age).
    pub retained: usize,
    /// Sum of `approx_disk_bytes` over `reclaim` (0 when sizes were not fetched).
    pub reclaim_bytes: u64,
}

impl ReapPolicy {
    /// The TTL that applies to `run` and a short reason label, or `None` when the
    /// Run is not a reap candidate at all (a live/archived status that should
    /// never have been listed).
    fn ttl_for(&self, run: &ReapableRun) -> Option<(i64, String)> {
        // Only terminal, non-archived Runs are candidates. The endpoint
        // guarantees this, but the policy must not rely on its caller.
        let base = match run.status {
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed-evidence",
            RunStatus::Halted => "halted-evidence",
            RunStatus::Skipped => "skipped-evidence",
            RunStatus::Running
            | RunStatus::AwaitingUser
            | RunStatus::Paused
            | RunStatus::Archived => return None,
        };

        // Self fast-lane: the janitor's own past Runs, any terminal status.
        if let Some(self_pipe) = &self.self_pipeline {
            if !run.pipeline_name.is_empty() && &run.pipeline_name == self_pipe {
                return Some((self_ttl_or(self.self_ttl_secs), format!("self-pipeline:{base}")));
            }
        }

        let ttl = match run.status {
            RunStatus::Completed => self.completed_ttl_secs,
            // Failed / Halted / Skipped (the only remaining terminal statuses).
            _ => self.terminal_ttl_secs,
        };
        Some((ttl, base.to_string()))
    }

    /// Compute the reclaim plan for a listing.
    pub fn plan(&self, runs: &[ReapableRun]) -> ReapPlan {
        let mut reclaim = Vec::new();
        let mut retained = 0usize;

        for run in runs {
            let Some((ttl, reason)) = self.ttl_for(run) else {
                // Live/archived — must never be reaped. Not counted as retained
                // residue either (it is not a reap candidate at all).
                continue;
            };
            match run.age_secs {
                Some(age) if age >= ttl => reclaim.push(ReapDecision {
                    run_id: run.run_id.clone(),
                    status: run.status.clone(),
                    age_secs: age,
                    approx_disk_bytes: run.approx_disk_bytes,
                    reason: format!("{reason}: age {age}s ≥ ttl {ttl}s"),
                }),
                _ => retained += 1, // too young, or no parseable age
            }
        }

        // Biggest-first, ties by run_id → stable and deterministic.
        reclaim.sort_by(|a, b| {
            b.approx_disk_bytes
                .unwrap_or(0)
                .cmp(&a.approx_disk_bytes.unwrap_or(0))
                .then_with(|| a.run_id.cmp(&b.run_id))
        });

        let reclaim_bytes = reclaim
            .iter()
            .map(|d| d.approx_disk_bytes.unwrap_or(0))
            .sum();

        ReapPlan {
            reclaim,
            retained,
            reclaim_bytes,
        }
    }
}

/// A non-negative TTL; a nonsensical negative override collapses to 0 (reap
/// immediately) rather than "never".
fn self_ttl_or(secs: i64) -> i64 {
    secs.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, pipeline: &str, status: RunStatus, age: Option<i64>, bytes: Option<u64>) -> ReapableRun {
        ReapableRun {
            run_id: id.to_string(),
            pipeline_name: pipeline.to_string(),
            status,
            age_secs: age,
            approx_disk_bytes: bytes,
        }
    }

    // A policy with the self fast-lane off, to isolate the per-status rules.
    fn no_self_policy() -> ReapPolicy {
        ReapPolicy {
            self_pipeline: None,
            ..Default::default()
        }
    }

    #[test]
    fn defaults_are_graded() {
        let p = ReapPolicy::default();
        assert_eq!(p.completed_ttl_secs, 24 * 3600);
        assert_eq!(p.terminal_ttl_secs, 72 * 3600);
        assert_eq!(p.self_ttl_secs, 3600);
        assert_eq!(p.self_pipeline.as_deref(), Some("disk-janitor"));
        // Evidence is held strictly longer than residue.
        assert!(p.terminal_ttl_secs > p.completed_ttl_secs);
    }

    #[test]
    fn completed_reaped_at_or_past_ttl_boundary_inclusive() {
        let p = no_self_policy();
        let ttl = p.completed_ttl_secs;

        // age == ttl → reclaimed (boundary inclusive).
        let at = p.plan(&[run("r", "some-pipe", RunStatus::Completed, Some(ttl), Some(10))]);
        assert_eq!(at.reclaim.len(), 1, "age == ttl must reclaim");
        assert_eq!(at.retained, 0);

        // age == ttl - 1 → retained.
        let below = p.plan(&[run("r", "some-pipe", RunStatus::Completed, Some(ttl - 1), Some(10))]);
        assert_eq!(below.reclaim.len(), 0, "age < ttl must retain");
        assert_eq!(below.retained, 1);
    }

    #[test]
    fn failed_uses_terminal_ttl_not_completed_ttl() {
        let p = no_self_policy();
        // 25 h: past the 24 h completed TTL, but well under the 72 h evidence TTL.
        let young = p.plan(&[run("f", "some-pipe", RunStatus::Failed, Some(25 * 3600), Some(1))]);
        assert_eq!(young.reclaim.len(), 0, "failed evidence must survive the completed TTL");
        assert_eq!(young.retained, 1);

        // 72 h: reclaimed — the leak is bounded, not infinite.
        let old = p.plan(&[run("f", "some-pipe", RunStatus::Failed, Some(72 * 3600), Some(1))]);
        assert_eq!(old.reclaim.len(), 1, "failed evidence is reclaimed once past the evidence TTL");
    }

    #[test]
    fn halted_and_skipped_route_to_terminal_ttl() {
        let p = no_self_policy();
        for status in [RunStatus::Halted, RunStatus::Skipped] {
            let young = p.plan(&[run("x", "some-pipe", status.clone(), Some(48 * 3600), Some(1))]);
            assert_eq!(young.reclaim.len(), 0, "{status:?} at 48h < 72h evidence TTL must retain");
            let old = p.plan(&[run("x", "some-pipe", status.clone(), Some(72 * 3600), Some(1))]);
            assert_eq!(old.reclaim.len(), 1, "{status:?} at 72h must reclaim");
        }
    }

    #[test]
    fn live_and_archived_never_reaped_even_when_ancient() {
        let p = no_self_policy();
        let ancient = Some(1_000_000_000);
        for status in [
            RunStatus::Running,
            RunStatus::AwaitingUser,
            RunStatus::Paused,
            RunStatus::Archived,
        ] {
            let plan = p.plan(&[run("l", "some-pipe", status.clone(), ancient, Some(999))]);
            assert_eq!(plan.reclaim.len(), 0, "{status:?} must never be reaped");
            assert_eq!(plan.retained, 0, "{status:?} is not a reap candidate, not residue");
        }
    }

    #[test]
    fn missing_age_is_never_reaped() {
        let p = no_self_policy();
        let plan = p.plan(&[run("n", "some-pipe", RunStatus::Completed, None, Some(10))]);
        assert_eq!(plan.reclaim.len(), 0, "no parseable age → cannot apply a TTL");
        assert_eq!(plan.retained, 1);
    }

    #[test]
    fn self_pipeline_fast_lane_reaps_own_runs_early() {
        let p = ReapPolicy::default(); // self fast-lane on: disk-janitor @ 1h
        // A janitor's own completed Run at 2h: under the 24h completed TTL, but
        // past the 1h self TTL → reclaimed.
        let own = p.plan(&[run("own", "disk-janitor", RunStatus::Completed, Some(2 * 3600), Some(11))]);
        assert_eq!(own.reclaim.len(), 1, "the janitor tidies its own runs on the short self TTL");
        assert!(own.reclaim[0].reason.starts_with("self-pipeline:"));

        // A different pipeline's completed Run at 2h: normal completed TTL → retained.
        let other = p.plan(&[run("other", "some-pipe", RunStatus::Completed, Some(2 * 3600), Some(11))]);
        assert_eq!(other.reclaim.len(), 0, "other pipelines keep the full completed TTL");
    }

    #[test]
    fn self_fast_lane_still_respects_liveness() {
        // Even the janitor's own live Run must never be reaped (it is running).
        let p = ReapPolicy::default();
        let plan = p.plan(&[run("live", "disk-janitor", RunStatus::Running, Some(999_999), Some(11))]);
        assert_eq!(plan.reclaim.len(), 0);
    }

    #[test]
    fn reclaim_ordered_biggest_first_then_by_run_id() {
        let p = no_self_policy();
        let ttl = p.completed_ttl_secs;
        let runs = vec![
            run("small", "some-pipe", RunStatus::Completed, Some(ttl), Some(100)),
            run("big", "some-pipe", RunStatus::Completed, Some(ttl), Some(5000)),
            run("mid-b", "some-pipe", RunStatus::Completed, Some(ttl), Some(1000)),
            run("mid-a", "some-pipe", RunStatus::Completed, Some(ttl), Some(1000)),
        ];
        let plan = p.plan(&runs);
        let order: Vec<&str> = plan.reclaim.iter().map(|d| d.run_id.as_str()).collect();
        // 5000, then the 1000-tie broken by run_id (mid-a < mid-b), then 100.
        assert_eq!(order, vec!["big", "mid-a", "mid-b", "small"]);
        assert_eq!(plan.reclaim_bytes, 5000 + 1000 + 1000 + 100);
    }

    #[test]
    fn empty_listing_yields_empty_plan() {
        let plan = ReapPolicy::default().plan(&[]);
        assert_eq!(plan.reclaim.len(), 0);
        assert_eq!(plan.retained, 0);
        assert_eq!(plan.reclaim_bytes, 0);
    }

    #[test]
    fn ttl_overrides_take_effect() {
        // Tighten the completed TTL to 1h: a 2h completed Run now reclaims.
        let mut p = no_self_policy();
        p.completed_ttl_secs = 3600;
        let plan = p.plan(&[run("r", "some-pipe", RunStatus::Completed, Some(2 * 3600), Some(1))]);
        assert_eq!(plan.reclaim.len(), 1);
    }

    #[test]
    fn deserializes_reapable_endpoint_shape_ignoring_extra_fields() {
        // Mirrors the real `GET /runs/reapable?size=true` payload, including
        // fields the policy does not consume.
        let json = r#"[
            {
                "run_id": "20260805-140021-86b6b86",
                "pipeline_name": "auto-issue-implement",
                "status": "completed",
                "completed_at": "2026-08-05T14:10:00Z",
                "age_secs": 350874,
                "worktree_present": true,
                "effective_repo": "/home/x/repo",
                "approx_disk_bytes": 10066329
            }
        ]"#;
        let runs: Vec<ReapableRun> = serde_json::from_str(json).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Completed);
        assert_eq!(runs[0].age_secs, Some(350874));
        assert_eq!(runs[0].approx_disk_bytes, Some(10066329));

        let plan = no_self_policy().plan(&runs);
        assert_eq!(plan.reclaim.len(), 1, "a 4-day-old completed run is well past a 24h TTL");
    }
}
