//! The Runs an extension creates through its `Host` (API 0.9): the same
//! create as `POST /runs`, for the Actor the extension names.

use std::sync::{Arc, Weak};

use pdo_extension_api::{NewRun, RunCreation, RunNotCreated};
use pdo_kernel::extension_host::RunCreator;
use pdo_kernel::Actor;

use crate::application::create_run::{reject_unknown_create_run_fields, CreateRunRequest};
use crate::Runs;

/// The run context, as what creates the extensions' Runs. Weak: the
/// extensions must not keep the Runs alive past the daemon.
pub struct ExtensionRuns(pub Weak<Runs>);

impl ExtensionRuns {
    pub fn of(runs: &Arc<Runs>) -> Self {
        Self(Arc::downgrade(runs))
    }
}

impl RunCreator for ExtensionRuns {
    fn create_run(&self, actor: Actor, run: NewRun) -> RunCreation {
        let runs = self.0.upgrade();
        Box::pin(async move {
            let Some(runs) = runs else {
                return Err(RunNotCreated::unavailable("the daemon is stopping"));
            };
            // The body `POST /runs` would receive, read as it reads it.
            let body = run.to_request_body();
            reject_unknown_create_run_fields(&body)
                .map_err(|(status, body)| RunNotCreated::new(status.as_u16(), body))?;
            let request: CreateRunRequest = serde_json::from_value(body).map_err(|e| {
                RunNotCreated::new(400, serde_json::json!({ "error": format!("invalid run: {e}") }))
            })?;
            // A root Run (no node session), created by `actor`.
            let response = runs.create_run(&actor, request, Vec::new(), None).await;
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap_or_default();
            let answer: serde_json::Value =
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            if status.is_success() {
                match answer.get("run_id").and_then(|id| id.as_str()) {
                    Some(id) => Ok(id.to_string()),
                    None => Err(RunNotCreated::new(500, answer)),
                }
            } else {
                Err(RunNotCreated::new(status.as_u16(), answer))
            }
        })
    }
}
