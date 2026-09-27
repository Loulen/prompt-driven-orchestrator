//! Read the version-check state (#697): the cache, never the network — two page
//! loads are zero requests.

use std::sync::Arc;

use super::update_context::UpdateContext;
use crate::domain::release::INSTALLED_VERSION;
use crate::domain::update_status::{update_status, StatusInputs, UpdateStatus};

pub struct GetUpdateStatus {
    update: Arc<UpdateContext>,
}

impl GetUpdateStatus {
    pub fn new(update: Arc<UpdateContext>) -> Self {
        Self { update }
    }

    pub async fn execute(&self) -> UpdateStatus {
        self.update.ensure_loaded().await;
        let ports = &self.update.ports;
        let enabled = ports.setting.enabled().await;
        let cache = self.update.cached().await;
        let source_url = self.update.source_url();
        let active_runs = ports.runs.count_live().await;
        let last_attempt = if ports.journal.is_available() {
            ports.journal.read_attempt()
        } else {
            None
        };
        update_status(StatusInputs {
            installed: INSTALLED_VERSION,
            enabled,
            cache: cache.as_ref(),
            source_url: &source_url,
            method: ports.installation.method(),
            supervision: ports.installation.supervision(),
            active_runs,
            last_attempt,
            now: chrono::Utc::now(),
        })
    }
}
