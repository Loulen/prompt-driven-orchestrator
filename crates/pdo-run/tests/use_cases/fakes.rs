//! In-memory adapters of every port of the run context.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use pdo_kernel::event_bus::EventBus;
use pdo_run::application::ports::*;
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::state::RunState;
use pdo_run::{Runs, RunsOptions, RunsPorts};
use tokio::sync::broadcast;

/// The event log, as a list. A Run listed in `forgotten` refuses every append;
/// `broken` makes every read fail.
#[derive(Default)]
pub struct Store {
    pub events: Mutex<Vec<Event>>,
    pub forgotten: Mutex<HashSet<String>>,
    pub broken: bool,
}

impl EventStore for Store {
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            if self.forgotten.lock().unwrap().contains(&event.run_id) {
                anyhow::bail!("run {} has been forgotten", event.run_id);
            }
            let mut events = self.events.lock().unwrap();
            let mut stored = event.clone();
            stored.id = Some(events.len() as i64 + 1);
            events.push(stored);
            Ok(())
        })
    }

    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>> {
        Box::pin(async move {
            if self.broken {
                anyhow::bail!("failed to load events");
            }
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.run_id == run_id)
                .cloned()
                .collect())
        })
    }

    fn run_ids(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>> {
        Box::pin(async move {
            let mut ids: Vec<String> = self
                .events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.run_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids.reverse();
            Ok(ids)
        })
    }

    fn has_children<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.events.lock().unwrap().iter().any(|e| {
                e.kind == EventKind::RunStarted
                    && e.payload
                        .as_ref()
                        .and_then(|p| p.get("parent_run_id"))
                        .and_then(|v| v.as_str())
                        == Some(run_id)
            })
        })
    }
}

/// The bus, recording what was published.
pub struct Bus {
    pub published: Mutex<Vec<serde_json::Value>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            published: Mutex::default(),
            tx: broadcast::channel(16).0,
        }
    }
}

impl EventBus for Bus {
    fn publish(&self, event: serde_json::Value) {
        self.published.lock().unwrap().push(event.clone());
        let _ = self.tx.send(event);
    }

    fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.tx.subscribe()
    }
}

/// The disk, as a lines-of-code delta it writes on every Run it fills.
#[derive(Default)]
pub struct Disk {
    pub filled: Mutex<Vec<(String, usize)>>,
}

impl LiveFacts for Disk {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.filled
                .lock()
                .unwrap()
                .push((run.run_id.clone(), events.len()));
            run.loc = Some(pdo_run::domain::state::LocStat {
                insertions: 3,
                deletions: 1,
                files_changed: 1,
            });
        })
    }
}

/// The managers with a live session.
#[derive(Default)]
pub struct Managers(pub Mutex<HashSet<String>>);

impl ManagerSessions for Managers {
    fn exists(&self, run_id: &str) -> bool {
        self.0.lock().unwrap().contains(run_id)
    }
}

/// The context over fresh fakes, and the fakes.
pub struct World {
    pub runs: Arc<Runs>,
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    pub disk: Arc<Disk>,
    pub managers: Arc<Managers>,
}

pub fn world() -> World {
    world_over(Store::default())
}

pub fn world_over(store: Store) -> World {
    let store = Arc::new(store);
    let bus = Arc::new(Bus::default());
    let disk = Arc::new(Disk::default());
    let managers = Arc::new(Managers::default());
    let runs = Runs::new(
        RunsPorts {
            store: store.clone(),
            bus: bus.clone(),
            live_facts: disk.clone(),
            managers: managers.clone(),
        },
        RunsOptions {
            daemon_port: 6172,
            allowed_ws_origins: Vec::new(),
        },
    );
    World {
        runs,
        store,
        bus,
        disk,
        managers,
    }
}

pub fn event(run_id: &str, kind: EventKind, node_id: Option<&str>) -> Event {
    Event {
        id: None,
        run_id: run_id.into(),
        ts: "2026-09-27T12:00:00.000Z".into(),
        kind,
        node_id: node_id.map(String::from),
        iter: node_id.map(|_| 1),
        payload: None,
    }
}

/// A `RunStarted` with one agent node, `worker`.
pub fn run_started(run_id: &str) -> Event {
    let mut started = event(run_id, EventKind::RunStarted, None);
    started.payload = Some(serde_json::json!({
        "pipeline_name": "p",
        "node_defs": [{
            "id": "worker", "node_type": "agent", "inputs": [], "outputs": [],
            "view_x": null, "view_y": null
        }],
        "edges": [],
    }));
    started
}
