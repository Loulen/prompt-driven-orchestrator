//! The Sessions hébergées the core holds: their records, kept in memory and
//! in the [`HostedStore`], and the publication of every change of their state.

use std::sync::{Arc, Mutex, MutexGuard};

use pdo_extension_api::{HostedSession, HostedSessionChanged};
use pdo_kernel::event_bus::guarded;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Operation, ResourceKind, Verb};
use tracing::warn;

use super::ports::HostedStore;
use crate::application::session_ledger::SessionLedger;
use crate::domain::hosted::HostedRecord;

/// The records, in the order the sessions were opened. Every change is
/// written to the store, counted for the Cap (through the ledger the Runs
/// read) and published: to every extension and, on `/ws`, to the connections
/// that may read the session.
pub struct HostedRegistry {
    store: Arc<dyn HostedStore>,
    host: Arc<dyn ExtensionHost>,
    ledger: Arc<SessionLedger>,
    records: Mutex<Vec<HostedRecord>>,
}

impl HostedRegistry {
    /// The registry over what `store` holds.
    pub fn load(
        store: Arc<dyn HostedStore>,
        host: Arc<dyn ExtensionHost>,
        ledger: Arc<SessionLedger>,
    ) -> Self {
        let records = store.load();
        let registry = Self {
            store,
            host,
            ledger,
            records: Mutex::new(records),
        };
        registry.count();
        registry
    }

    pub fn store(&self) -> &Arc<dyn HostedStore> {
        &self.store
    }

    /// The session `key` of `extension`.
    pub fn find(&self, extension: &str, key: &str) -> Option<HostedRecord> {
        self.lock()
            .iter()
            .find(|r| r.session.extension == extension && r.session.key == key)
            .cloned()
    }

    /// The session named `id`.
    pub fn by_id(&self, id: &str) -> Option<HostedRecord> {
        self.lock().iter().find(|r| r.id() == id).cloned()
    }

    /// Every session of `extension`, in the order they were opened.
    pub fn list(&self, extension: &str) -> Vec<HostedSession> {
        self.lock()
            .iter()
            .filter(|r| r.session.extension == extension)
            .map(|r| r.session.clone())
            .collect()
    }

    /// Every record, in order.
    pub fn all(&self) -> Vec<HostedRecord> {
        self.lock().clone()
    }

    /// Whether a live session named `id` is recorded: what keeps it from the
    /// sweep.
    pub fn holds_live(&self, id: &str) -> bool {
        self.lock().iter().any(|r| r.id() == id && r.is_live())
    }

    /// The live sessions: what they take of the Cap.
    pub fn live_count(&self) -> usize {
        self.lock().iter().filter(|r| r.is_live()).count()
    }

    /// The Projets of the session `id`; none for a name not recorded.
    pub fn projects_of(&self, id: &str) -> Vec<String> {
        self.by_id(id).map(|r| r.projects()).unwrap_or_default()
    }

    /// Record a new session and publish it. Refused when its key or its id
    /// is taken.
    pub fn insert(&self, record: HostedRecord) -> Result<(), String> {
        {
            let mut records = self.lock();
            let taken = records.iter().any(|r| {
                r.id() == record.id()
                    || (r.session.extension == record.session.extension
                        && r.session.key == record.session.key)
            });
            if taken {
                return Err(format!(
                    "a hosted session `{}` exists already",
                    record.session.key
                ));
            }
            self.store.save(&record)?;
            records.push(record.clone());
        }
        self.changed(&record);
        Ok(())
    }

    /// Apply `change` to the record `id`; when it answers that it changed the
    /// record, store and publish it. The record as it now stands and whether
    /// it changed; `None` for an id not recorded. A record the store refuses
    /// is still changed in memory, and the refusal logged: the session's
    /// state is a fact.
    pub fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut HostedRecord) -> bool,
    ) -> Option<(HostedRecord, bool)> {
        let (record, changed) = {
            let mut records = self.lock();
            let record = records.iter_mut().find(|r| r.id() == id)?;
            let changed = change(record);
            if changed {
                if let Err(e) = self.store.save(record) {
                    warn!("the hosted session {id} could not be stored: {e}");
                }
            }
            (record.clone(), changed)
        };
        if changed {
            self.changed(&record);
        }
        Some((record, changed))
    }

    /// Forget the record `id` and its state folder.
    pub fn forget(&self, id: &str) {
        self.lock().retain(|r| r.id() != id);
        self.store.remove(id);
        self.count();
    }

    /// Forget the record `id` and its state folder if it is ended, checked
    /// under the lock. Whether it was forgotten.
    pub fn forget_ended(&self, id: &str) -> bool {
        {
            let mut records = self.lock();
            let before = records.len();
            records.retain(|r| r.id() != id || r.is_live());
            if records.len() == before {
                return false;
            }
            self.store.remove(id);
        }
        self.count();
        true
    }

    fn changed(&self, record: &HostedRecord) {
        self.count();
        self.announce(
            record,
            HostedSessionChanged::new(record.session.clone()).to_event(),
        );
    }

    /// Publish `event` about the session `record`: to every extension and,
    /// on `/ws`, to the connections that may read the session.
    pub fn announce(&self, record: &HostedRecord, event: serde_json::Value) {
        let read = Operation::new(ResourceKind::Session, Verb::Read)
            .on(record.id())
            .in_projects(record.projects());
        self.host.announce(guarded(event, &read));
    }

    fn count(&self) {
        self.ledger.set_hosted_live(self.live_count());
    }

    fn lock(&self) -> MutexGuard<'_, Vec<HostedRecord>> {
        match self.records.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
