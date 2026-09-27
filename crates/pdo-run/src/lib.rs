//! The `run` bounded context (ADR-0079), first slice (#923): the foundation
//! the scheduler and the Run's surfaces stand on.
//!
//! - [`domain`] holds the pure rules: a Run's events, its state and the
//!   projection that folds one into the other; the graph a Run walks (loops,
//!   routing, conditions, inputs); what a Run allows (transitions, the session
//!   cap, the completion, retry and restart verdicts, the reap policy).
//! - [`application`] holds the ports (the [`EventStore`] first) and the use
//!   cases: append an event, then announce it on the kernel's bus; read a Run.
//! - [`adapters`] holds the event store over SQLite, the routes that read a
//!   Run (`GET /runs/{id}`, `…/events`) and the `/ws` stream, which relays the
//!   bus.
//!
//! The rest of the Run (the scheduler, the node lifecycle, the Run's other
//! surfaces) still lives in the legacy crate and calls this one. It depends on
//! the kernel and on the pipeline, session, sandbox, workspace, skill and
//! harness contexts.
//!
//! [`EventStore`]: application::ports::EventStore

pub mod adapters;
pub mod application;
pub mod domain;

use std::sync::Arc;

use pdo_kernel::event_bus::EventBus;

use application::append_event::AppendEvent;
use application::ports::{EventStore, LiveFacts, ManagerSessions};
use application::read_run::ReadRun;

pub use adapters::http::router;

/// Every port the context needs, as the composition root plugs them in.
pub struct RunsPorts {
    pub store: Arc<dyn EventStore>,
    /// The kernel's bus: every appended event is published there, as `/ws`
    /// relays it.
    pub bus: Arc<dyn EventBus>,
    /// What a Run's detail reads from disk (implemented by the legacy side).
    pub live_facts: Arc<dyn LiveFacts>,
    pub managers: Arc<dyn ManagerSessions>,
}

/// The daemon's knobs the context reads.
#[derive(Debug, Clone)]
pub struct RunsOptions {
    pub daemon_port: u16,
    /// Extra WebSocket origins (`PDO_ALLOWED_WS_ORIGINS`), parsed at boot.
    pub allowed_ws_origins: Vec<String>,
}

/// The context, assembled: its use cases over its ports.
pub struct Runs {
    store: Arc<dyn EventStore>,
    bus: Arc<dyn EventBus>,
    options: RunsOptions,
    append_event: AppendEvent,
    read_run: ReadRun,
}

impl Runs {
    pub fn new(ports: RunsPorts, options: RunsOptions) -> Arc<Self> {
        let RunsPorts {
            store,
            bus,
            live_facts,
            managers,
        } = ports;
        Arc::new(Self {
            append_event: AppendEvent::new(store.clone(), bus.clone()),
            read_run: ReadRun::new(store.clone(), live_facts, managers),
            store,
            bus,
            options,
        })
    }

    /// The event log.
    pub fn store(&self) -> &Arc<dyn EventStore> {
        &self.store
    }

    /// The bus `/ws` relays.
    pub fn bus(&self) -> &Arc<dyn EventBus> {
        &self.bus
    }

    pub fn options(&self) -> &RunsOptions {
        &self.options
    }

    /// The one way an event enters a Run's log.
    pub fn append_event(&self) -> &AppendEvent {
        &self.append_event
    }

    pub fn read_run(&self) -> &ReadRun {
        &self.read_run
    }
}
