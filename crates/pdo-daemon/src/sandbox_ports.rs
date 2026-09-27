//! The sandbox context's routes, wired onto this daemon's store and home, and
//! the port the legacy daemon implements for it: who still names a staging
//! profile (the instance settings, the Triggers, the live Runs).
//!
//! The Run's side of its sandbox (its context, its merge-back) lives in the run
//! context (`pdo_run::application::sandbox`).

use std::sync::Arc;

use axum::Router;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_sandbox::adapters::env as sandbox_env;
use pdo_sandbox::adapters::host::EnvHome;
use pdo_sandbox::adapters::sqlite_profiles::SqliteProfiles;
use pdo_sandbox::application::ports::{
    BoxFuture, ProfileReferents, Referents, RunReferent, TriggerReferent,
};
use pdo_sandbox::application::profiles::SandboxProfiles;
use pdo_sandbox::domain::mode::SandboxMode;
use tracing::error;

use crate::{instance_config, trigger_store, AppState};

/// The staging profiles of `db`.
pub(crate) fn profile_store(db: &sqlx::SqlitePool) -> SqliteProfiles {
    SqliteProfiles::new(db.clone())
}

/// The routes of the sandbox context, wired onto this daemon's store, home and
/// referents, asking `host` before every mutation.
pub(crate) fn router(state: &Arc<AppState>, host: Arc<dyn ExtensionHost>) -> Router {
    let profiles = SandboxProfiles::new(
        Arc::new(profile_store(&state.db)),
        host,
        Arc::new(EnvHome::new(state.sandbox_home_override.clone())),
        Arc::new(LegacyReferents {
            db: state.db.clone(),
        }),
    );
    pdo_sandbox::adapters::http::router(Arc::new(profiles))
}

/// Who names a profile, in the stores still owned by the legacy daemon: the instance
/// settings, the Triggers, the Runs.
struct LegacyReferents {
    db: sqlx::SqlitePool,
}

/// The RESOLVED tier, so a stored value shadowed by the env is not reported as a
/// referent when it is not the one that wins.
async fn names_the_instance_default(db: &sqlx::SqlitePool, name: &str) -> bool {
    let stored_default = instance_config::get(db)
        .await
        .ok()
        .and_then(|c| c.default_sandbox);
    sandbox_env::default_sandbox_with(stored_default).profile() == Some(name)
}

async fn triggers_naming(db: &sqlx::SqlitePool, name: &str) -> Vec<TriggerReferent> {
    match trigger_store::list(db).await {
        Ok(list) => list
            .iter()
            .filter(|t| {
                t.sandbox
                    .as_deref()
                    .and_then(SandboxMode::parse)
                    .as_ref()
                    .and_then(SandboxMode::profile)
                    == Some(name)
            })
            .map(|t| TriggerReferent {
                id: t.id.clone(),
                name: t.name.clone(),
                enabled: t.enabled,
            })
            .collect(),
        Err(e) => {
            error!("failed to list triggers for profile referents: {e}");
            Vec::new()
        }
    }
}

/// One indexed pass over `run_started` payloads, excluding any run that already
/// emitted a terminal event. Cheaper than projecting each run, and terminality is
/// exactly "a terminal event exists" for this purpose.
async fn live_runs_naming(db: &sqlx::SqlitePool, name: &str) -> Vec<RunReferent> {
    let rows: Result<Vec<(String, String)>, _> = sqlx::query_as(
        "SELECT e.run_id, e.payload FROM events e \
         WHERE e.kind = 'run_started' AND e.payload IS NOT NULL \
           AND NOT EXISTS ( \
             SELECT 1 FROM events t WHERE t.run_id = e.run_id AND t.kind IN \
               ('run_completed', 'run_failed', 'run_skipped', 'run_halted', 'run_archived') \
           ) \
         ORDER BY e.ts DESC",
    )
    .fetch_all(db)
    .await;
    match rows {
        Ok(rows) => rows
            .iter()
            .filter_map(|(run_id, payload)| {
                let val = serde_json::from_str::<serde_json::Value>(payload).ok()?;
                if val.get("sandbox")?.as_str()? != name {
                    return None;
                }
                Some(RunReferent {
                    run_id: run_id.clone(),
                    pipeline_name: val
                        .get("pipeline_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    name: val.get("name").and_then(|v| v.as_str()).map(str::to_string),
                })
            })
            .collect(),
        Err(e) => {
            error!("failed to query runs for profile referents: {e}");
            Vec::new()
        }
    }
}
impl ProfileReferents for LegacyReferents {
    fn referents<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Referents> {
        Box::pin(async move {
            Referents {
                instance_default: names_the_instance_default(&self.db, name).await,
                triggers: triggers_naming(&self.db, name).await,
                runs: live_runs_naming(&self.db, name).await,
            }
        })
    }
}
