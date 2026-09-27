//! The ports of the run context that what is still legacy implements: what a
//! Run's detail reads live from disk (its lines-of-code delta, its cost, the
//! run-scoped pipeline's graph), the instance settings a spawn reads, and the
//! Run's surfaces the Pipeline Manager's commands reach (archive, create).

use std::collections::BTreeMap;
use std::sync::Weak;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_harness::agent_choice::ResolvedCombo;
use pdo_run::application::ports::{
    BoxFuture, InstanceDefaults, InstanceSettings, LiveFacts, RunSurfaces,
};

use crate::{
    agent_profile, augment_run_state_from_disk, effective_repo_root, event_log, instance_config,
    price_table, run_sandbox, AppState,
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
            let (home_root, sandbox_root) = run_sandbox::sandbox_home_roots(&state.run)
                .unwrap_or_else(|_| {
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

/// The instance settings and the agentic profiles, which the settings context
/// (still in this crate) owns, read fresh at every call.
pub(crate) struct LegacyInstanceSettings {
    pub(crate) db: sqlx::SqlitePool,
}

impl InstanceSettings for LegacyInstanceSettings {
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>> {
        Box::pin(async move {
            let c = instance_config::get(&self.db).await.ok()?;
            Some(InstanceDefaults {
                session_cap: c.session_cap,
                default_model: c.default_model,
                default_harness: c.default_harness,
                default_harness_model: c.default_harness_model,
                autocomplete_turn_end: c.autocomplete_turn_end,
                auto_fail: instance_config::resolve_instance_auto_fail(c.auto_fail),
                agent_choice: c.agent_choice,
                skills: c.skills,
            })
        })
    }

    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>> {
        Box::pin(async move { agent_profile::snapshot(&self.db).await.unwrap_or_default() })
    }
}

/// The Run's surfaces this crate still serves (archive, create), reached
/// through the daemon's state.
pub(crate) struct LegacyRunSurfaces {
    pub(crate) state: Weak<AppState>,
}

impl LegacyRunSurfaces {
    fn gone() -> Response {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "the daemon is shutting down",
        )
            .into_response()
    }
}

impl RunSurfaces for LegacyRunSurfaces {
    fn cleanup<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Response> {
        Box::pin(async move {
            match self.state.upgrade() {
                Some(state) => crate::cleanup_run(&state, run_id).await,
                None => Self::gone(),
            }
        })
    }

    fn retry_all<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Response> {
        Box::pin(async move {
            match self.state.upgrade() {
                Some(state) => crate::retry_all_run(&state, run_id).await,
                None => Self::gone(),
            }
        })
    }
}
