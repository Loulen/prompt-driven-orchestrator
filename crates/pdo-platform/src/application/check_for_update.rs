//! Check the release source for a newer version (#697): on the operator's
//! demand, on the periodic tick, or right after the check is turned back on.

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::update_context::{UpdateContext, IN_FLIGHT};
use crate::domain::update_check::UPDATE_CHECK_INTERVAL;

/// Why a check on demand did not produce a fresh answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    Refused(Denied),
    /// The `update_check` setting is off: no request leaves the daemon.
    Off,
    /// Another check is in flight.
    InFlight(String),
    /// The release source failed; the cache records it beside the last good
    /// values.
    Failed(String),
}

pub struct CheckForUpdate {
    update: Arc<UpdateContext>,
    host: Arc<dyn ExtensionHost>,
}

impl CheckForUpdate {
    pub fn new(update: Arc<UpdateContext>, host: Arc<dyn ExtensionHost>) -> Self {
        Self { update, host }
    }

    /// Load the journal's cache, so the reads answer from it at once (a no-op
    /// once loaded).
    pub async fn load(&self) {
        self.update.ensure_loaded().await;
    }

    /// `POST /update/check`: the operator forces a check now.
    pub async fn execute(&self, actor: &Actor) -> Result<(), CheckError> {
        let operation = Operation::new(ResourceKind::Update, Verb::Action("check".into()));
        authorize(self.host.as_ref(), actor, &operation).map_err(CheckError::Refused)?;
        self.update.ensure_loaded().await;
        if !self.update.ports.setting.enabled().await {
            return Err(CheckError::Off);
        }
        match self.update.perform_check().await {
            Ok(_) => Ok(()),
            Err(e) if e == IN_FLIGHT => Err(CheckError::InFlight(e)),
            Err(e) => Err(CheckError::Failed(e)),
        }
    }

    /// One periodic pass: loads the journal's cache if not yet done, then checks
    /// ONLY if the setting is on and the cache is older than the interval.
    pub async fn periodic(&self) {
        self.update.ensure_loaded().await;
        if !self.update.ports.setting.enabled().await {
            return; // off: no request ever leaves the daemon
        }
        let fresh = self
            .update
            .cached()
            .await
            .is_some_and(|c| c.is_fresh(UPDATE_CHECK_INTERVAL));
        if fresh {
            return; // checked within the interval: the cache IS the answer
        }
        if let Err(e) = self.update.perform_check().await {
            tracing::warn!("update check: {e} — keeping the last known values");
        }
    }

    /// Check once now, whatever the cache says — what turning the setting back
    /// on triggers. The setting was just written, so it is not read again.
    pub async fn now(&self) {
        let _ = self.update.perform_check().await;
    }
}
