//! The review context, assembled: its ports and the reads every use case
//! shares (a Run, a Run ref, a sent comment).

use std::sync::Arc;

use pdo_kernel::authorization::{authorize, Denied};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_run::domain::review_comment::ReviewComment;
use pdo_run::domain::run_refs::{self, Resolved};
use pdo_workspace::adapters::git::revisions::is_safe_ref;

use super::failure::{Failure, Outcome};
use super::ports::{ReviewGit, ReviewRuns, ReviewSettings, ReviewedRun};

/// Every port the context needs, as the composition root plugs them in.
pub struct ReviewPorts {
    pub runs: Arc<dyn ReviewRuns>,
    pub git: Arc<dyn ReviewGit>,
    pub settings: Arc<dyn ReviewSettings>,
    /// Asked before every gesture on a comment.
    pub extensions: Arc<dyn ExtensionHost>,
}

/// The review context.
pub struct Review {
    pub(crate) runs: Arc<dyn ReviewRuns>,
    pub(crate) git: Arc<dyn ReviewGit>,
    pub(crate) settings: Arc<dyn ReviewSettings>,
    extensions: Arc<dyn ExtensionHost>,
}

/// What a `from`/`to` value resolved to (#835).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    /// A git ref (branch, SHA) to hand to `git`.
    Git(String),
    /// The Run's working tree — read from its dir, or its snapshot tree.
    Worktree,
}

impl Review {
    pub fn new(ports: ReviewPorts) -> Arc<Self> {
        Arc::new(Self {
            runs: ports.runs,
            git: ports.git,
            settings: ports.settings,
            extensions: ports.extensions,
        })
    }

    /// Ask the extensions whether `actor` may run `operation`, in the Projets
    /// of the Run its target names (`<run>` or `<run>/<comment>`).
    pub async fn authorize(&self, actor: &Actor, operation: &Operation) -> Result<(), Denied> {
        let run_id = operation
            .target
            .as_deref()
            .map(|target| target.split_once('/').map_or(target, |(run, _)| run));
        let operation = match run_id {
            Some(run_id) => operation
                .clone()
                .in_projects(self.runs.projects_of_run(run_id).await),
            None => operation.clone(),
        };
        authorize(self.extensions.as_ref(), actor, &operation)
    }

    /// The Run, or the JSON `500` / `404` a Run's surfaces answer.
    pub(crate) async fn run(&self, run_id: &str) -> Outcome<ReviewedRun> {
        match self.runs.load(run_id).await {
            Ok(Some(run)) => Ok(run),
            Ok(None) => Err(Failure::error(404, "run not found")),
            Err(e) => Err(Failure::error(500, format!("error: {e}"))),
        }
    }

    /// The `review_agent_can_resolve` instance setting, read fresh.
    pub(crate) async fn agent_can_resolve(&self) -> bool {
        self.settings.agent_can_resolve().await
    }
}

/// The id a destination defaults to: the working tree while it exists (#835),
/// the tip otherwise.
pub(crate) fn default_to_id(run: &ReviewedRun) -> &'static str {
    if run.worktree.is_some() {
        run_refs::WORKTREE_ID
    } else {
        run_refs::TIP_ID
    }
}

/// Resolve one `from`/`to` value (#749): absent ⇒ `default_id`; a stable Run
/// ref id ⇒ its git ref (`404` when the Run knows no such ref) or the working
/// tree (#835); anything else ⇒ a raw git ref, kept for compatibility once it
/// passes the injection guard (`400` otherwise).
pub(crate) fn resolve_ref(
    run_id: &str,
    run: &ReviewedRun,
    value: Option<&str>,
    default_id: &str,
) -> Outcome<RefTarget> {
    let value = value.unwrap_or(default_id);
    match run_refs::resolve_id(run_id, &run.state, &run.events, value) {
        Resolved::Git(r) => Ok(RefTarget::Git(r)),
        Resolved::Worktree => Ok(RefTarget::Worktree),
        Resolved::Unknown => Err(Failure::error(404, format!("unknown run ref: {value}"))),
        Resolved::NotAnId => {
            if !is_safe_ref(value) {
                return Err(Failure::Text(400, format!("invalid ref: {value:?}")));
            }
            Ok(RefTarget::Git(value.to_string()))
        }
    }
}

/// A sent comment by id, or the `404` the three verbs share.
pub(crate) fn find_comment<'a>(
    run: &'a ReviewedRun,
    comment_id: &str,
) -> Outcome<&'a ReviewComment> {
    run.state
        .review_comments
        .iter()
        .find(|c| c.id == comment_id)
        .ok_or_else(|| {
            Failure::error(
                404,
                format!("unknown review comment {comment_id:?} on this Run (ids read rc-001, rc-002, …; `pdo review list` shows them)"),
            )
        })
}
