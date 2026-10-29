//! The Sessions hébergées (ADR-0091): an agent or a shell an extension has
//! the core hold outside any Run. Opened and terminated on an extension's
//! gesture (each an Opération on the Ressource `session`), recorded so they
//! survive a restart of the daemon, never swept while recorded live, counted
//! by the Cap de sessions concurrentes, and ended when their process exits.

mod lifecycle;
mod open;
pub mod ports;
mod registry;

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;

pub use lifecycle::TERMINATE;
pub use open::OPEN;
pub use ports::{
    AdmissionSlot, HostedHarness, HostedPlaces, HostedStore, NotAdmitted, SessionAdmission,
};
pub use registry::HostedRegistry;

use crate::application::ports::Tmux;
use crate::application::session_ledger::SessionLedger;

/// The ports of the Sessions hébergées, as the composition root plugs them.
pub struct HostedPorts {
    pub store: Arc<dyn HostedStore>,
    pub places: Arc<dyn HostedPlaces>,
    pub harness: Arc<dyn HostedHarness>,
    pub admission: Arc<dyn SessionAdmission>,
}

/// The Sessions hébergées of every extension, and their gestures.
pub struct Hosted {
    registry: Arc<HostedRegistry>,
    tmux: Arc<dyn Tmux>,
    host: Arc<dyn ExtensionHost>,
    ledger: Arc<SessionLedger>,
    places: Arc<dyn HostedPlaces>,
    harnesses: Arc<dyn HostedHarness>,
    admission: Arc<dyn SessionAdmission>,
    daemon_port: u16,
    tmux_cmd_override: Option<String>,
}

impl Hosted {
    /// The Sessions hébergées `ports.store` holds. `tmux_cmd_override`
    /// replaces an agent's tail (the per-daemon test seam).
    pub fn new(
        ports: HostedPorts,
        tmux: Arc<dyn Tmux>,
        host: Arc<dyn ExtensionHost>,
        ledger: Arc<SessionLedger>,
        daemon_port: u16,
        tmux_cmd_override: Option<String>,
    ) -> Self {
        let HostedPorts {
            store,
            places,
            harness,
            admission,
        } = ports;
        Self {
            registry: Arc::new(HostedRegistry::load(store, host.clone(), ledger.clone())),
            tmux,
            host,
            ledger,
            places,
            harnesses: harness,
            admission,
            daemon_port,
            tmux_cmd_override,
        }
    }

    /// The records, as the sweep, the terminals and the Host read them.
    pub fn registry(&self) -> &Arc<HostedRegistry> {
        &self.registry
    }
}
