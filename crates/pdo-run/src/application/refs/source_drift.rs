//! A Run's drift from its source branch since the fork point (#803, ADR-0070
//! §4): a measurement on local refs, recomputed on every read, never a fetch.

use std::path::Path;

use axum::extract::Json;
use axum::response::{IntoResponse, Response};
use pdo_workspace::adapters::git::repository::{count_commits, last_fetch_at, ref_exists, upstream_of};
use pdo_workspace::adapters::git::revisions;
use serde::Serialize;
use ts_rs::TS;

use crate::application::events::load_projected;
use crate::application::paths::effective_repo_root;
use crate::domain::run_refs;
use crate::domain::state::RunState;
use crate::Runs;

/// The Run's drift from its source branch since the fork point (#803, CONTEXT.md
/// § « Dérive de la source »).
///
/// A **measurement on local refs**, recomputed on every read and never a stored
/// snapshot: `ahead` is what the Run produced, `behind` what arrived on its source.
/// Nothing here fetches — ADR-0070 §1 is explicit that displaying a Run must not put
/// traffic on someone's repo; the Run view's fetch button is the only refresh, and it
/// is the #802 verb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SourceDrift {
    Available {
        /// The Run's source branch, verbatim as it was stored.
        source_branch: String,
        /// The fork point the Run was cut at, short.
        fork: String,
        /// Commits the Run's branch made since the fork.
        ahead: u32,
        /// Commits that arrived on the source since the fork — ONE number, the max
        /// of the local branch's and its tracking branch's (spec #801 Q3): the chip
        /// keeps the `n↑ m↓` shape of the form, and the tooltip splits the two.
        behind: u32,
        /// What the LOCAL source branch gained. `None` when the source is itself a
        /// remote-tracking ref (there is no local side to count).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        local_behind: Option<u32>,
        /// The source's tracking branch and what IT gained. `None` when the source
        /// tracks nothing (or already IS a tracking ref).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        upstream: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        upstream_behind: Option<u32>,
        /// When the repo last fetched successfully — the date the `behind` side of
        /// the remote is good as of. Same derivation as the form's (`FETCH_HEAD`).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        last_fetch_at: Option<String>,
    },
    /// The measurement cannot be made — an archived Run whose branch was cleaned
    /// up, a source branch that was deleted. **Not an error**: it is the expected
    /// end of a Run's life, and `reason` says which.
    Unavailable { reason: String },
}

/// Compute a Run's drift from its source (#803). Pure over a repo on disk, so the
/// tests drive it against real temporary repositories rather than a mocked git.
pub fn source_drift(repo: &Path, run_id: &str, run_state: &RunState) -> SourceDrift {
    let unavailable = |reason: &str| SourceDrift::Unavailable {
        reason: reason.to_string(),
    };

    let tip = run_refs::tip_branch(run_id);
    if revisions::rev_parse(repo, &tip).is_none() {
        return unavailable("the Run's branch no longer exists");
    }
    let fork_ref = run_refs::fork_base(run_state);
    let Some(fork_sha) = revisions::rev_parse(repo, fork_ref) else {
        return unavailable("the Run's fork point no longer resolves");
    };
    let Some(source) = run_state.source_branch.as_deref().filter(|s| !s.is_empty()) else {
        return unavailable("this Run recorded no source branch");
    };
    if revisions::rev_parse(repo, source).is_none() {
        return unavailable("the source branch no longer exists");
    }

    let Some(ahead) = count_commits(repo, &fork_sha, &tip) else {
        return unavailable("the Run's branch could not be counted");
    };

    // The source may be a local branch or a tracking ref (both are legitimate Run
    // sources, #571). Only a LOCAL one has an upstream to count separately.
    let is_local = ref_exists(repo, &format!("refs/heads/{source}"));
    let local_behind = count_commits(repo, &fork_sha, source);
    let upstream = is_local.then(|| upstream_of(repo, source)).flatten();
    let upstream_behind = upstream
        .as_deref()
        .and_then(|u| count_commits(repo, &fork_sha, &format!("refs/remotes/{u}")));

    SourceDrift::Available {
        source_branch: source.to_string(),
        fork: short_sha(&fork_sha),
        ahead,
        behind: local_behind.unwrap_or(0).max(upstream_behind.unwrap_or(0)),
        local_behind: is_local.then_some(local_behind).flatten(),
        upstream,
        upstream_behind,
        last_fetch_at: last_fetch_at(repo),
    }
}

/// The 7-char form every other PDO surface shows a SHA in.
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// `GET /runs/<id>/source-drift` (#803, ADR-0070 §4): how far the Run and its source
/// have drifted apart since the fork point.
///
/// A READ over local refs, recomputed per call — no fetch, ever (opening a Run must
/// not put traffic on someone's repository). A Run whose branch or source is gone
/// answers `unavailable` with a reason, **200**: an archived Run is the expected end
/// of a Run's life, not an error to report as one.
impl Runs {
    pub async fn source_drift(&self, run_id: &str) -> Response {
    let state = self;
    let (_events, run_state) = match load_projected(state, run_id).await {
        Ok(t) => t,
        Err(resp) => return *resp,
    };
    let repo = effective_repo_root(state, &run_state);
    Json(source_drift(&repo, run_id, &run_state)).into_response()
}
}

