//! In-memory adapters for the use-case tests (stage 2, ADR-0080): every port,
//! plus an extension host that records what it was asked and may refuse.

mod fakes;
mod stores;

use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::{ExtensionHost, GitCredentials, GitOperation};
use pdo_kernel::{Actor, Operation};
use serde_json::Value;

pub use fakes::*;
pub use stores::*;

use super::ports::BoxFuture;
use super::workspace::{Workspace, WorkspacePorts};

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

/// Whether an Operation asked by an Actor is refused.
type Refuses = Box<dyn Fn(&Actor, &Operation) -> bool + Send + Sync>;

/// An extension host that records every Operation and refuses them all when
/// given a reason.
#[derive(Default)]
pub struct RecordingHost {
    refusal: Option<String>,
    /// Refuse only the reads in these Projets (CORE-5).
    reads_refused_in: Vec<String>,
    /// Refuse what this says, with « refused ».
    refuses: Option<Refuses>,
    asked: Mutex<Vec<Operation>>,
    git_asked: Mutex<Vec<GitOperation>>,
    /// Stands for a daemon without extension (`allows_everything`).
    no_extension: bool,
}

/// The variables [`RecordingHost`] provides for `alice`'s git operations.
pub const ALICE_GIT_ENV: (&str, &str) = ("TEST_GIT_TOKEN", "alice-token");

/// The Actor [`RecordingHost`] provides git credentials for.
pub fn alice() -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: "alice".into(),
    }
}

impl RecordingHost {
    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }
    /// Every git operation it was asked credentials for, in order.
    pub fn git_asked(&self) -> Vec<GitOperation> {
        self.git_asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for RecordingHost {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }
    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        if self.refuses.as_ref().is_some_and(|f| f(actor, operation)) {
            return Authorization::deny("refused");
        }
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
    fn allows_everything(&self) -> bool {
        self.no_extension
    }
    /// [`ALICE_GIT_ENV`] for [`alice`], the daemon's credentials otherwise.
    fn git_credentials(&self, operation: &GitOperation) -> GitCredentials {
        self.git_asked.lock().unwrap().push(operation.clone());
        if operation.actor != alice() {
            return GitCredentials::daemon();
        }
        let (name, value) = ALICE_GIT_ENV;
        GitCredentials::provided(
            [(name.to_string(), value.to_string())].into(),
            "Alice",
            None,
        )
    }
}

pub struct Fixture {
    pub workspace: Workspace,
    pub projects: Arc<InMemoryProjects>,
    pub provisioning: Arc<InMemoryProvisioning>,
    pub recent: Arc<InMemoryRecent>,
    pub events: Arc<RecordingEvents>,
    pub deletions: Arc<RecordingDeletions>,
    pub git: Arc<FakeGit>,
    pub source_tree: Arc<RecordingSourceTree>,
    pub skills: Arc<ScriptedSkills>,
    pub host: Arc<RecordingHost>,
}

/// A workspace as a daemon without extension has it: nothing refused, the
/// host says it allows everything.
pub fn without_extension() -> Fixture {
    build_with(RecordingHost {
        no_extension: true,
        ..RecordingHost::default()
    })
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
    let deletions = Arc::new(RecordingDeletions::default());
    let git = Arc::new(FakeGit::default());
    let source_tree = Arc::new(RecordingSourceTree::default());
    let skills = Arc::new(ScriptedSkills::default());
    let host = Arc::new(host);
    let workspace = Workspace::new(
        WorkspacePorts {
            projects: projects.clone(),
            provisioning: provisioning.clone(),
            recent: recent.clone(),
            events: events.clone(),
            deletions: deletions.clone(),
            git: git.clone(),
            source_tree: source_tree.clone(),
            files: Arc::new(NoFiles),
            clones_folder: Arc::new(FixedClonesFolder),
            skills: skills.clone(),
        },
        host.clone(),
    );
    Fixture {
        workspace,
        projects,
        provisioning,
        recent,
        events,
        deletions,
        git,
        source_tree,
        skills,
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

/// A workspace whose extension refuses what `refuses` says, with « refused ».
pub fn refusing(refuses: impl Fn(&Actor, &Operation) -> bool + Send + Sync + 'static) -> Fixture {
    build_with(RecordingHost {
        refuses: Some(Box::new(refuses)),
        ..RecordingHost::default()
    })
}

/// A person identified by the test extension.
pub fn person(id: &str) -> Actor {
    Actor::Identified {
        namespace: "test".into(),
        id: id.into(),
    }
}

/// Whether `operation` is a creation of a repository or a read of one in no
/// Projet (the disk), or an update of a Projet other than `project`: what
/// a person who may only write in `project` is refused.
pub fn outside_of(project: &str, operation: &Operation) -> bool {
    use pdo_kernel::{ResourceKind, Verb};
    let in_project = operation.projects.iter().any(|p| p == project);
    match (&operation.resource, &operation.verb) {
        (ResourceKind::Repository, _) => !in_project,
        (ResourceKind::Project, Verb::Update) => !in_project,
        _ => false,
    }
}
