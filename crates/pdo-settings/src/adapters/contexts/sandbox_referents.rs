//! Who names a staging profile, read where each tier lives: the resolved
//! instance default (the stored settings), the Triggers (the trigger context's
//! store), the live Runs (the run context's log).

use pdo_sandbox::adapters::env as sandbox_env;
use pdo_sandbox::application::ports::{
    BoxFuture, ProfileReferents, Referents, RunReferent, TriggerReferent,
};
use pdo_sandbox::domain::mode::SandboxMode;
use tracing::error;

use crate::adapters::sqlite::instance_config;

/// The tiers of the daemon's database.
pub struct SandboxTierReferents {
    pub db: sqlx::SqlitePool,
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
    match pdo_trigger::adapters::sqlite::list(db).await {
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

impl ProfileReferents for SandboxTierReferents {
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
