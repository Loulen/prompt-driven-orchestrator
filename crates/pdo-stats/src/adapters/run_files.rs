//! [`RunFiles`] over this daemon's disk: where a Run's transcripts and stores
//! live (sandbox-aware, #408/#708), its memoized cost (the run context's
//! `run_cost`), its footprint, and the price table of the HOST home.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use pdo_run::adapters::run_cost::{
    compute_run_cost_breakdown_cached, event_fingerprint, max_transcript_mtime_millis,
    reported_stores_mtime_millis,
};
use pdo_run::domain::event::Event;
use pdo_run::domain::prices::PriceTable;
use pdo_run::domain::run_cost::CostContribution;
use pdo_sandbox::adapters::staging::stores::{transcripts_root, HarnessStores};
use pdo_sandbox::domain::mode::SandboxMode;

use crate::application::ports::{RunFiles, RunFilesRoots};

/// The Runs' files of this daemon.
pub struct DiskRunFiles {
    /// The daemon's repo root: the repository of a historical Run that names
    /// none (ADR-0033).
    repo_root: PathBuf,
    /// Replaces `$HOME` for the sandbox staging and the price files (the
    /// per-daemon test seam).
    home_override: Option<PathBuf>,
}

impl DiskRunFiles {
    pub fn new(repo_root: PathBuf, home_override: Option<PathBuf>) -> Self {
        Self {
            repo_root,
            home_override,
        }
    }

    /// `(home_root, sandbox_root)`: the override, else `$HOME`. HOME absent →
    /// degrade to the empty root rather than fail the aggregate.
    fn home_roots(&self) -> (PathBuf, PathBuf) {
        match &self.home_override {
            Some(home) => pdo_sandbox::adapters::host::roots_under(home),
            None => pdo_sandbox::adapters::host::default_roots_from_env().unwrap_or_else(|| {
                let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
                let sandbox = home.join(".pdo").join("sandbox");
                (home, sandbox)
            }),
        }
    }
}

impl RunFiles for DiskRunFiles {
    fn roots(&self, run_id: &str, payload: &serde_json::Value) -> RunFilesRoots {
        let (home_root, sandbox_root) = self.home_roots();
        let repo_root = payload
            .get("target_repo")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.repo_root.clone());
        // #432: all the folds ever need is the off-ness of the frozen sandbox,
        // read straight off the `run_started` payload — never a decode of the
        // closed enum, never a profile lookup inside a per-Run loop.
        let sandboxed = payload
            .get("sandbox")
            .and_then(|v| v.as_str())
            .is_some_and(|s| {
                let t = s.trim();
                !t.is_empty() && !t.eq_ignore_ascii_case(SandboxMode::OFF_WIRE)
            });
        RunFilesRoots {
            claude_root: transcripts_root(sandboxed, run_id, &home_root, &sandbox_root),
            repo_root,
            stores: HarnessStores::for_run(sandboxed, run_id, &home_root, &sandbox_root),
        }
    }

    fn cost_contributions(
        &self,
        events: &[Event],
        roots: &RunFilesRoots,
        run_id: &str,
        prices: &PriceTable,
    ) -> Vec<CostContribution> {
        compute_run_cost_breakdown_cached(
            events,
            &roots.claude_root,
            &roots.stores,
            &roots.repo_root,
            run_id,
            prices,
        )
        .contributions
    }

    fn footprint(&self, events: &[Event], roots: &RunFilesRoots, run_id: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        event_fingerprint(events).hash(&mut hasher);
        max_transcript_mtime_millis(&roots.claude_root, &roots.repo_root, run_id).hash(&mut hasher);
        reported_stores_mtime_millis(
            events,
            &roots.claude_root,
            &roots.stores,
            &roots.repo_root,
            run_id,
        )
        .hash(&mut hasher);
        hasher.finish()
    }

    fn prices(&self) -> PriceTable {
        // Prices are an instance concept: the HOST home, even for a sandboxed
        // Run — the #408 seam moves the TRANSCRIPT root, not this one.
        PriceTable::load(&self.home_roots().0)
    }
}
