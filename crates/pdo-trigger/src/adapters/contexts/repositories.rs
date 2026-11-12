//! The repositories a Trigger names, through the workspace's git port and the
//! Run's write-boundary rule.

use std::path::PathBuf;
use std::sync::Arc;

use pdo_kernel::extension_host::{ExtensionHost, GitOperationKind};
use pdo_kernel::Actor;
use pdo_run::application::create_run::repos::required_target_repo;
use pdo_workspace::application::fetch_for;
use pdo_workspace::application::ports::Git;
use pdo_workspace::domain::branches::FetchError;
use tracing::warn;

use crate::application::ports::{BoxFuture, Repositories};

/// The repositories on this host, reached with git (the workspace's port),
/// with the credentials the extensions provide.
pub struct GitRepositories {
    git: Arc<dyn Git>,
    host: Arc<dyn ExtensionHost>,
}

impl GitRepositories {
    pub fn new(git: Arc<dyn Git>, host: Arc<dyn ExtensionHost>) -> Self {
        Self { git, host }
    }
}

impl Repositories for GitRepositories {
    fn required(&self, field: Option<&str>) -> Result<PathBuf, String> {
        required_target_repo(field)
    }

    fn refresh<'a>(
        &'a self,
        targets: &'a [String],
        editor: &'a Actor,
        project: Option<&'a str>,
    ) -> BoxFuture<'a, Option<FetchError>> {
        Box::pin(self.fetch_before_cut(targets, editor, project))
    }
}

impl GitRepositories {
    /// Fetch every remote of every repository a Trigger fire will cut from, before
    /// the cut (#804, ADR-0070 §1), for `editor`. Answers the **primary**'s failure —
    /// the one the Run surfaces — or `None` when it worked.
    ///
    /// A failure NEVER stops the fire: the Run is cut from the last state known
    /// locally and carries the reason, because losing a scheduled fire to a network
    /// blip is the worse outcome by far (spec #801, story 21). Never a fast-forward
    /// either, in any of these repos: there is nobody at a Trigger to arbitrate one
    /// (ADR-0070 §2). A refused fetch names whose credentials were refused.
    ///
    /// A secondary's failure is logged, not surfaced: its base ref is pinned to a SHA
    /// the moment after, and the Run detail states freshness for the source branch —
    /// one repo, one sentence. A path that is not a repository is skipped rather than
    /// reported, because the create chokepoint is about to refuse the fire naming that
    /// very path, and a git-remote error here would bury that message under a fetch
    /// one.
    async fn fetch_before_cut(
        &self,
        targets: &[String],
        editor: &Actor,
        project: Option<&str>,
    ) -> Option<FetchError> {
        let mut primary_error = None;
        for (index, path) in targets.iter().enumerate() {
            let Ok(repo) = self.git.validate_repository(path) else {
                continue;
            };
            let outcome = fetch_for(
                &*self.git,
                &*self.host,
                &repo,
                editor,
                GitOperationKind::TriggerFetch,
                project,
            )
            .await;
            if index == 0 {
                primary_error = outcome;
            } else if let Some(error) = outcome {
                warn!(
                    "trigger fire: pre-cut fetch of secondary repo {path} failed: {}",
                    error.message
                );
            }
        }
        primary_error
    }
}
