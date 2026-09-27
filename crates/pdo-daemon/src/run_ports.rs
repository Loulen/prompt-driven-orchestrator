//! The ports of the run context that what is still legacy implements: what a
//! Run's detail reads live from disk (its lines-of-code delta, its cost, the
//! run-scoped pipeline's graph).

use std::sync::Weak;

use pdo_run::application::ports::{BoxFuture, LiveFacts};

use crate::{
    augment_run_state_from_disk, effective_repo_root, event_log, price_table, run_sandbox, AppState,
};

/// The disk of the daemon's Runs, reached through its state.
pub(crate) struct LegacyLiveFacts {
    pub(crate) state: Weak<AppState>,
}

impl LiveFacts for LegacyLiveFacts {
    fn fill<'a>(
        &'a self,
        run: &'a mut event_log::RunState,
        events: &'a [event_log::Event],
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let Some(state) = self.state.upgrade() else {
                return;
            };
            let repo_root = effective_repo_root(&state, run);
            // Read the cost transcripts from the sandboxed Run's staged home while
            // it is live, `~/.claude/projects/` otherwise. HOME absent → degrade to
            // the host root; never fail a read.
            let (home_root, sandbox_root) =
                run_sandbox::sandbox_home_roots(&state).unwrap_or_else(|_| {
                    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
                    let sandbox = home.join(".pdo").join("sandbox");
                    (home, sandbox)
                });
            let projects_root = pdo_sandbox::adapters::staging::stores::transcripts_root(
                !run.sandbox.is_off(),
                &run.run_id,
                &home_root,
                &sandbox_root,
            );
            // Resolve the three price tiers ONCE, here at the edge. Deliberately NOT
            // an `AppState` cache and never a `OnceLock`: either would make a daemon
            // restart mandatory to change a price, annulling ADR-0034. Note the seam
            // asymmetry — prices come from `home_root` (the HOST home, since prices
            // are an instance concept) while transcripts come from `projects_root`,
            // which moves for a sandboxed Run.
            let prices = price_table::PriceTable::load(&home_root);
            // `copilot`'s journal is on the host; `pi`'s moves per Run (#708) — the
            // staged sink while this sandboxed Run lives, the host store after
            // merge-back — mirroring the Claude root's sandbox-aware seam above.
            let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::for_run(
                !run.sandbox.is_off(),
                &run.run_id,
                &home_root,
                &sandbox_root,
            );
            augment_run_state_from_disk(run, events, &projects_root, &stores, &repo_root, &prices);
        })
    }
}
