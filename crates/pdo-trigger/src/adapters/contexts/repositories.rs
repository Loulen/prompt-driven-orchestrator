//! The repositories a Trigger names, through the workspace's git adapter and
//! the Run's write-boundary rule.

use std::path::PathBuf;

use pdo_run::application::create_run::repos::required_target_repo;
use pdo_workspace::adapters::git::fetch::fetch_all_remotes;
use pdo_workspace::adapters::git::repository::validate_target_repo;
use pdo_workspace::domain::branches::FetchError;
use tracing::warn;

use crate::application::ports::{BoxFuture, Repositories};

/// The repositories on this host, reached with git.
pub struct GitRepositories;

impl Repositories for GitRepositories {
    fn required(&self, field: Option<&str>) -> Result<PathBuf, String> {
        required_target_repo(field)
    }

    fn refresh<'a>(&'a self, targets: &'a [String]) -> BoxFuture<'a, Option<FetchError>> {
        Box::pin(fetch_before_cut(targets))
    }
}

/// Fetch every remote of every repository a Trigger fire will cut from, before the
/// cut (#804, ADR-0070 §1). Answers the **primary**'s failure — the one the Run
/// surfaces — or `None` when it worked.
///
/// A failure NEVER stops the fire: the Run is cut from the last state known locally
/// and carries the reason, because losing a scheduled fire to a network blip is the
/// worse outcome by far (spec #801, story 21). Never a fast-forward either, in any
/// of these repos: there is nobody at a Trigger to arbitrate one (ADR-0070 §2).
///
/// A secondary's failure is logged, not surfaced: its base ref is pinned to a SHA
/// the moment after, and the Run detail states freshness for the source branch —
/// one repo, one sentence. A path that is not a repository is skipped rather than
/// reported, because the create chokepoint is about to refuse the fire naming that
/// very path, and a git-remote error here would bury that message under a fetch one.
pub async fn fetch_before_cut(targets: &[String]) -> Option<FetchError> {
    let mut primary_error = None;
    for (index, path) in targets.iter().enumerate() {
        let Ok(repo) = validate_target_repo(path) else {
            continue;
        };
        let outcome = fetch_all_remotes(&repo).await;
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
