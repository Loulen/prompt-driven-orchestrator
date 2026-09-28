//! Read the version-check state (#697): the cache, never the network — two page
//! loads are zero requests.

use std::sync::Arc;

use super::update_context::UpdateContext;
use crate::domain::release_channel::CORE_VERSION;
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
        let channel = self.update.channel();
        let (method, manual_command) = self.update.upgrader();
        let active_runs = ports.runs.count_live().await;
        let last_attempt = if ports.journal.is_available() {
            ports.journal.read_attempt()
        } else {
            None
        };
        update_status(StatusInputs {
            channel_name: &channel.name,
            installed: &channel.installed_version,
            core_version: CORE_VERSION,
            enabled,
            cache: cache.as_ref(),
            source_url: source_url.as_deref(),
            method,
            manual_command: manual_command.as_deref(),
            supervision: ports.installation.supervision(),
            active_runs,
            last_attempt,
            now: chrono::Utc::now(),
        })
    }
}
