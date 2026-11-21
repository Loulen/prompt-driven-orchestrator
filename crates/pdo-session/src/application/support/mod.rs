//! The Agent de support: one interactive agent per Actor, outside any Run,
//! with the rights of its Actor (the token its launch hooks give). Opened with
//! an agentic profile or a custom combination and a first question, asked
//! further questions while it lives, read, closed; closed as well once it
//! stays idle and nobody answers « Still there? ».

mod ask;
mod close;
mod idle;
mod open;
mod ports;
mod status;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use pdo_kernel::authorization::Denied;
use pdo_kernel::event_bus::{BroadcastEventBus, EventBus};
use pdo_kernel::extension_host::ExtensionHost;

pub use ask::ASK;
pub use idle::IDLE_WATCH_INTERVAL;
pub use open::SupportOpened;
pub use ports::{ChoiceRefusal, ResolvedAgent, SupportAgents, SupportClock, SystemClock};
pub use status::{StillThere, SupportStatus};

use crate::application::ports::Tmux;
use crate::application::session_ledger::SessionLedger;
use crate::domain::support::{support_dir, support_record_path, SupportAgent, SupportRecord};

/// Where the agents run, and how their launch may be stubbed.
#[derive(Debug, Clone)]
pub struct SupportLaunch {
    /// The daemon's state folder for the Agents de support: one folder per
    /// Actor under it.
    pub root: PathBuf,
    pub daemon_port: u16,
    /// Replaces the agent (the per-daemon test seam).
    pub tmux_cmd_override: Option<String>,
}

/// Why a gesture on an Agent de support was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportError {
    Refused(Denied),
    /// The choice of agent does not resolve to one that can run here.
    Choice(ChoiceRefusal),
    /// An agent is open already: close it first, or open with `replace`.
    AlreadyOpen(Option<Box<SupportAgent>>),
    /// No agent is open to ask.
    NotOpen,
    /// The question is empty.
    EmptyQuestion,
    /// tmux refused the question: it did not reach the agent.
    NotDelivered,
    /// The agent's folder could not be prepared.
    Folder(String),
    /// The session did not come up.
    Spawn(String),
}

/// The Agents de support of the daemon, one per Actor.
pub struct Support {
    tmux: Arc<dyn Tmux>,
    agents: Arc<dyn SupportAgents>,
    host: Arc<dyn ExtensionHost>,
    ledger: Arc<SessionLedger>,
    launch: SupportLaunch,
    clock: Arc<dyn SupportClock>,
    /// Where the changes of an agent go: the bus, hence `/ws`.
    events: Arc<dyn EventBus>,
    /// The clock of every live agent, by session.
    idle: Mutex<HashMap<String, idle::Watched>>,
    /// How many clocks were started.
    starts: AtomicU64,
}

impl Support {
    pub fn new(
        tmux: Arc<dyn Tmux>,
        agents: Arc<dyn SupportAgents>,
        host: Arc<dyn ExtensionHost>,
        launch: SupportLaunch,
    ) -> Self {
        Self {
            ledger: Arc::new(SessionLedger::new(tmux.clone(), host.clone())),
            tmux,
            agents,
            host,
            launch,
            clock: Arc::new(SystemClock),
            events: Arc::new(BroadcastEventBus::new(1)),
            idle: Mutex::default(),
            starts: AtomicU64::new(0),
        }
    }

    /// The same, announcing the sessions it starts in `ledger`.
    pub fn with_ledger(mut self, ledger: Arc<SessionLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    /// The same, telling the changes of its agents on `events`.
    pub fn with_events(mut self, events: Arc<dyn EventBus>) -> Self {
        self.events = events;
        self
    }

    /// The same, on `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn SupportClock>) -> Self {
        self.clock = clock;
        self
    }

    /// The record of the session `session`; empty when none was written or
    /// it does not read.
    fn record(&self, session: &str) -> SupportRecord {
        std::fs::read_to_string(self.record_path(session))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_record(&self, session: &str, record: &SupportRecord) -> Result<(), String> {
        let path = self.record_path(session);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    fn record_path(&self, session: &str) -> PathBuf {
        support_record_path(&support_dir(&self.launch.root, session))
    }
}
