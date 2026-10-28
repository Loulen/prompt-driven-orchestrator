//! What the Sessions hébergées need from the outside world: where their
//! records and state folders live, where a folder resolves, which harness an
//! agent runs on, and the Cap de sessions concurrentes.

use std::path::PathBuf;

use pdo_harness::registry::HarnessDescriptor;

use crate::application::ports::BoxFuture;
use crate::domain::hosted::HostedRecord;

/// The records of the Sessions hébergées, and their state folders: under the
/// daemon's `~/.pdo/`, never in the folder a session works in.
pub trait HostedStore: Send + Sync {
    /// Every record, in the order the sessions were opened.
    fn load(&self) -> Vec<HostedRecord>;

    /// Write `record`, replacing the one of the same id.
    fn save(&self, record: &HostedRecord) -> Result<(), String>;

    /// Forget the record `id` and its state folder.
    fn remove(&self, id: &str);

    /// The state folder of the session `id`: its primer, its hooks, its
    /// Artefacts live. Not created.
    fn state_dir(&self, id: &str) -> PathBuf;
}

/// Where the folder of a Session hébergée resolves.
pub trait HostedPlaces: Send + Sync {
    /// The home folder of the daemon's user, if it has one.
    fn home(&self) -> Option<PathBuf>;

    /// The primary repository of the Projet `project`; `None` for a Projet
    /// the daemon does not know, or one without a repository.
    fn project_repo<'a>(&'a self, project: &'a str) -> BoxFuture<'a, Option<PathBuf>>;
}

/// The harness an agent session runs on.
pub trait HostedHarness: Send + Sync {
    /// The harness `name`, or the instance's default one for `None`; `None`
    /// for a name the daemon does not know.
    fn resolve<'a>(&'a self, name: Option<&'a str>) -> BoxFuture<'a, Option<HarnessDescriptor>>;
}

/// Why a session was given no slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAdmitted {
    /// The Cap de sessions concurrentes is full.
    CapReached { live: usize, cap: usize },
    /// The Cap cannot be read (the daemon is starting or stopping).
    Unavailable(String),
}

/// A slot of the Cap, taken under the admission lock: the lock is released
/// when the slot is dropped, once the session that takes it is recorded.
pub struct AdmissionSlot(#[allow(dead_code)] Box<dyn Send + Sync>);

impl AdmissionSlot {
    /// The slot whose `guard` holds the admission lock until dropped.
    pub fn holding(guard: impl Send + Sync + 'static) -> Self {
        Self(Box::new(guard))
    }
}

/// The Cap de sessions concurrentes, shared with the Nodes of the Runs
/// (implemented by the `run` side): the same lock, the same count.
pub trait SessionAdmission: Send + Sync {
    /// Take a slot, or say why not. Nothing waits for a slot.
    fn admit(&self) -> BoxFuture<'_, Result<AdmissionSlot, NotAdmitted>>;

    /// A session gave its slot back: the Nodes waiting for one may start.
    fn released(&self);
}
