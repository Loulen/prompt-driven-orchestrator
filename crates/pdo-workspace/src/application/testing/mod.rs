//! In-memory adapters for the use-case tests (stage 2, ADR-0080): every port,
//! plus an extension host that records what it was asked and may refuse.

mod fakes;
mod stores;

use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use serde_json::Value;

pub use fakes::*;
pub use stores::*;

use super::ports::BoxFuture;
use super::workspace::{Workspace, WorkspacePorts};

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

/// An extension host that records every Operation and refuses them all when
/// given a reason.
#[derive(Default)]
pub struct RecordingHost {
    refusal: Option<String>,
    /// Refuse only the reads in these Projets (CORE-5).
    reads_refused_in: Vec<String>,
    asked: Mutex<Vec<Operation>>,
}

impl RecordingHost {
    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for RecordingHost {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }
    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        if operation.is_read()
            && operation
                .projects
                .iter()
                .any(|p| self.reads_refused_in.contains(p))
        {
            return Authorization::deny("no reading there");
        }
        match &self.refusal {
            Some(reason) => Authorization::deny(reason.clone()),
            None => Authorization::Allow,
        }
    }
    fn publish(&self, _event: &Value) {}
}

pub struct Fixture {
    pub workspace: Workspace,
    pub projects: Arc<InMemoryProjects>,
    pub provisioning: Arc<InMemoryProvisioning>,
    pub recent: Arc<InMemoryRecent>,
    pub events: Arc<RecordingEvents>,
    pub git: Arc<FakeGit>,
    pub source_tree: Arc<RecordingSourceTree>,
    pub host: Arc<RecordingHost>,
}

fn build(refusal: Option<String>) -> Fixture {
    build_with(RecordingHost {
        refusal,
        ..RecordingHost::default()
    })
}

fn build_with(host: RecordingHost) -> Fixture {
    let projects = Arc::new(InMemoryProjects::default());
    let provisioning = Arc::new(InMemoryProvisioning::default());
    let recent = Arc::new(InMemoryRecent::default());
    let events = Arc::new(RecordingEvents::default());
    let git = Arc::new(FakeGit::default());
    let source_tree = Arc::new(RecordingSourceTree::default());
    let host = Arc::new(host);
    let workspace = Workspace::new(
        WorkspacePorts {
            projects: projects.clone(),
            provisioning: provisioning.clone(),
            recent: recent.clone(),
            events: events.clone(),
            git: git.clone(),
            source_tree: source_tree.clone(),
            files: Arc::new(NoFiles),
        },
        host.clone(),
    );
    Fixture {
        workspace,
        projects,
        provisioning,
        recent,
        events,
        git,
        source_tree,
        host,
    }
}

/// A workspace with no extension refusing anything.
pub fn fixture() -> Fixture {
    build(None)
}

/// A workspace whose extension refuses every Operation with `reason`.
pub fn denying(reason: &str) -> Fixture {
    build(Some(reason.to_string()))
}

/// A workspace whose extension refuses only the reads in `projects`.
pub fn denying_reads_in(projects: &[&str]) -> Fixture {
    build_with(RecordingHost {
        reads_refused_in: projects.iter().map(|p| p.to_string()).collect(),
        ..RecordingHost::default()
    })
}
