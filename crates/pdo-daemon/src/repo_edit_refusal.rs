//! Why a mid-run repo-list edit was refused, and how that refusal projects onto
//! the wire (#465 slice 2, ADR-0042).
//!
//! Same shape as [`completion_refusal`](crate::completion_refusal): the cause is a
//! value, and [`repo_edit_refusal_response`] is the **single** place a status is
//! chosen. [`RepoEditRefusal::status`] is a closed `match` with **no wildcard**, so
//! a new variant cannot compile until its status is decided — the `event_log`
//! dispatch patron. The whole chain travels as `Option<RepoEditRefusal>` (≈40 bytes)
//! and never as `Result<_, axum::Response>`: a `Response` returned by value in a
//! `Result::Err` trips `clippy::result_large_err`, which the CI treats as
//! `-D warnings`.
//!
//! `PATCH /runs/{id}/repos` is additive — it only ever adds or drops read-only
//! secondary snapshots — so there is **no** `recoverable` axis here (that flag is
//! `pdo complete`'s exit-3-vs-4 signal, meaningless to an HTTP edit). Every body is
//! `{ "error": <slug>, "message": <prose>, ...detail }`.

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};

use crate::event_log::RunStatus;

/// The cause of a refused `PATCH /runs/{id}/repos`.
///
/// `Clone` for symmetry with `CompletionRefusal` (the handler logs the reason then
/// projects it); the variants are small.
#[derive(Debug, Clone)]
pub(crate) enum RepoEditRefusal {
    /// Run tombstoned (#328 / ADR-0024). Keeps its `410` — "never 2xx" is not
    /// "always 409".
    RunForgotten { run_id: String },
    /// The projection renders no Run for this id — unknown target, `404`.
    RunNotFound,
    /// The Run is terminal (`Completed`/`Failed`/`Skipped`/`Halted`/`Archived`) and
    /// its repo list is frozen for good (#221). `409` — the request is well-formed,
    /// the target is simply not editable any more. The reducer no-ops too (double
    /// guard), so even a forced append cannot un-terminalize the Run.
    RunTerminal { status: RunStatus },
    /// An `add` entry resolves (by canonical path) to the Run's **primary** repo. A
    /// repo cannot be its own read-only secondary — the nodes already work in the
    /// primary. `400`.
    SelfReference { repo: String },
    /// An `add` entry is already an active secondary of this Run. `409`.
    RepoAlreadyPinned { repo: String },
    /// An `add` entry is not an absolute path / not a git repo, or its base ref does
    /// not resolve to a single commit **locally** (no fetch, ever). `400` — the same
    /// fail-fast boundary the create path enforces, so a bad repo yields no snapshot
    /// and no event.
    BadRepo { repo: String, error: String },
    /// `git worktree add --detach` failed while materialising the snapshot — a panne,
    /// not a verdict, so `500`. Raised **before** any event is appended: the log
    /// never carries a pin whose snapshot is missing.
    SnapshotMaterializeFailed { repo: String, error: String },
    /// A failure to load / project the Run before any verdict (db read, forgotten
    /// check). `500` — the daemon could not decide, not a refusal it argues.
    Internal { error: String },
    /// The `RunReposEdited` event could not be appended after the snapshots were
    /// materialised. `500`.
    AppendFailed { error: String },
}

impl RepoEditRefusal {
    /// The stable slug clients discriminate on — **never** the status (a status has
    /// too few bits for the causes here). Matches the table in the ADR/plan.
    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Self::RunForgotten { .. } => "run_forgotten",
            Self::RunNotFound => "run_not_found",
            Self::RunTerminal { .. } => "run_not_editable",
            Self::SelfReference { .. } => "secondary_is_primary",
            Self::RepoAlreadyPinned { .. } => "secondary_already_pinned",
            Self::BadRepo { .. } => "bad_secondary_repo",
            Self::SnapshotMaterializeFailed { .. } => "snapshot_materialize_failed",
            Self::Internal { .. } => "internal_error",
            Self::AppendFailed { .. } => "append_failed",
        }
    }

    /// HTTP status. Closed `match`, **no wildcard**, **never** `2xx`: adding a
    /// variant does not compile until its status is decided (the `event_log`
    /// dispatch patron), and `a_refusal_never_projects_to_a_2xx` proves the range.
    fn status(&self) -> StatusCode {
        match self {
            Self::RunForgotten { .. } => StatusCode::GONE,
            Self::RunNotFound => StatusCode::NOT_FOUND,
            Self::RunTerminal { .. } | Self::RepoAlreadyPinned { .. } => StatusCode::CONFLICT,
            Self::SelfReference { .. } | Self::BadRepo { .. } => StatusCode::BAD_REQUEST,
            Self::SnapshotMaterializeFailed { .. }
            | Self::Internal { .. }
            | Self::AppendFailed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The human-readable prose, folded into the body under `message`. Kept close to
    /// the create-path wording so the two surfaces read the same (anti-#509).
    pub(crate) fn message(&self) -> String {
        match self {
            Self::RunForgotten { run_id } => format!("run {run_id} has been forgotten"),
            Self::RunNotFound => "run not found".into(),
            Self::RunTerminal { status } => format!(
                "run is {} and its repository list is frozen; a terminal Run is not editable",
                status_slug(status)
            ),
            Self::SelfReference { repo } => format!(
                "secondary repo {repo} is the same as the primary; a repo cannot be its own secondary"
            ),
            Self::RepoAlreadyPinned { repo } => {
                format!("secondary repo {repo} is already pinned to this run")
            }
            Self::BadRepo { repo, error } => format!("invalid secondary repo {repo}: {error}"),
            Self::SnapshotMaterializeFailed { repo, error } => {
                format!("failed to materialise the snapshot for secondary {repo}: {error}")
            }
            Self::Internal { error } => error.clone(),
            Self::AppendFailed { error } => {
                format!("failed to append the repos-edited event: {error}")
            }
        }
    }

    /// The variant-specific detail, merged flat into the body next to `error` and
    /// `message` — mirrors [`completion_refusal`](crate::completion_refusal).
    fn detail(&self) -> serde_json::Value {
        match self {
            Self::RunTerminal { status } => serde_json::json!({ "status": status_slug(status) }),
            Self::SelfReference { repo }
            | Self::RepoAlreadyPinned { repo }
            | Self::BadRepo { repo, .. }
            | Self::SnapshotMaterializeFailed { repo, .. } => serde_json::json!({ "repo": repo }),
            Self::RunForgotten { .. }
            | Self::RunNotFound
            | Self::Internal { .. }
            | Self::AppendFailed { .. } => serde_json::json!({}),
        }
    }
}

/// The snake_case wire token of a [`RunStatus`], for the body / prose. Local so
/// this module owns its projection and does not lean on `serde` internals leaking.
fn status_slug(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::AwaitingUser => "awaiting_user",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Skipped => "skipped",
        RunStatus::Halted => "halted",
        RunStatus::Paused => "paused",
        RunStatus::Archived => "archived",
    }
}

/// The **single** projection of a repo-edit refusal onto HTTP.
///
/// Takes a reference: the caller logs the refusal before projecting it, and a
/// `Response` returned by value from a `Result::Err` would trip
/// `clippy::result_large_err` (`-D warnings` in CI).
pub(crate) fn repo_edit_refusal_response(r: &RepoEditRefusal) -> Response {
    let mut body = serde_json::json!({
        "error": r.slug(),
        "message": r.message(),
    });
    if let (Some(obj), Some(extra)) = (body.as_object_mut(), r.detail().as_object()) {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    (r.status(), Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample per variant, produced behind an exhaustive `match` **without a
    /// wildcard** — a new variant that is not sampled here stops compiling. Same
    /// floor as `completion_refusal::every_refusal`.
    fn every_refusal() -> Vec<RepoEditRefusal> {
        let all = vec![
            RepoEditRefusal::RunForgotten {
                run_id: "r1".into(),
            },
            RepoEditRefusal::RunNotFound,
            RepoEditRefusal::RunTerminal {
                status: RunStatus::Completed,
            },
            RepoEditRefusal::SelfReference {
                repo: "/repos/primary".into(),
            },
            RepoEditRefusal::RepoAlreadyPinned {
                repo: "/repos/lib".into(),
            },
            RepoEditRefusal::BadRepo {
                repo: "/repos/nope".into(),
                error: "not a git repository".into(),
            },
            RepoEditRefusal::SnapshotMaterializeFailed {
                repo: "/repos/lib".into(),
                error: "git worktree add failed".into(),
            },
            RepoEditRefusal::Internal {
                error: "db is locked".into(),
            },
            RepoEditRefusal::AppendFailed {
                error: "disk full".into(),
            },
        ];

        let mut seen = std::collections::BTreeSet::new();
        for r in &all {
            let key = match r {
                RepoEditRefusal::RunForgotten { .. } => "RunForgotten",
                RepoEditRefusal::RunNotFound => "RunNotFound",
                RepoEditRefusal::RunTerminal { .. } => "RunTerminal",
                RepoEditRefusal::SelfReference { .. } => "SelfReference",
                RepoEditRefusal::RepoAlreadyPinned { .. } => "RepoAlreadyPinned",
                RepoEditRefusal::BadRepo { .. } => "BadRepo",
                RepoEditRefusal::SnapshotMaterializeFailed { .. } => "SnapshotMaterializeFailed",
                RepoEditRefusal::Internal { .. } => "Internal",
                RepoEditRefusal::AppendFailed { .. } => "AppendFailed",
            };
            seen.insert(key);
        }
        assert_eq!(
            seen.len(),
            all.len(),
            "every_refusal() must hold exactly one sample per variant"
        );
        all
    }

    /// The invariant: no refusal ever projects to a `2xx`.
    #[test]
    fn a_refusal_never_projects_to_a_2xx() {
        for r in every_refusal() {
            let status = repo_edit_refusal_response(&r).status();
            assert!(
                status.is_client_error() || status.is_server_error(),
                "{} projected to a non-error status ({status})",
                r.slug()
            );
        }
    }

    /// "Never 2xx" ≠ "always 409": the tombstone keeps its 410, the unknown target
    /// its 404, the bad-repo / self-reference their 400, the pannes their 500.
    #[test]
    fn statuses_match_the_table() {
        let cases = [
            (
                RepoEditRefusal::RunForgotten { run_id: "r".into() },
                StatusCode::GONE,
            ),
            (RepoEditRefusal::RunNotFound, StatusCode::NOT_FOUND),
            (
                RepoEditRefusal::RunTerminal {
                    status: RunStatus::Failed,
                },
                StatusCode::CONFLICT,
            ),
            (
                RepoEditRefusal::RepoAlreadyPinned { repo: "x".into() },
                StatusCode::CONFLICT,
            ),
            (
                RepoEditRefusal::SelfReference { repo: "x".into() },
                StatusCode::BAD_REQUEST,
            ),
            (
                RepoEditRefusal::BadRepo {
                    repo: "x".into(),
                    error: "e".into(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                RepoEditRefusal::SnapshotMaterializeFailed {
                    repo: "x".into(),
                    error: "e".into(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                RepoEditRefusal::Internal { error: "e".into() },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                RepoEditRefusal::AppendFailed { error: "e".into() },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (r, want) in cases {
            assert_eq!(
                repo_edit_refusal_response(&r).status(),
                want,
                "{}",
                r.slug()
            );
        }
    }

    /// Every body carries a string `error` slug and a string `message`, and the
    /// slug round-trips.
    #[tokio::test]
    async fn every_body_carries_error_and_message() {
        for r in every_refusal() {
            let resp = repo_edit_refusal_response(&r);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .expect("refusal body");
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).expect("refusal body is JSON");
            assert_eq!(body["error"].as_str(), Some(r.slug()));
            assert!(
                body["message"].is_string(),
                "{} has no string message: {body}",
                r.slug()
            );
        }
    }

    /// The terminal refusal carries the offending status slug so a client can tell
    /// a Completed Run from an Archived one.
    #[tokio::test]
    async fn terminal_refusal_names_the_status() {
        let r = RepoEditRefusal::RunTerminal {
            status: RunStatus::Archived,
        };
        let resp = repo_edit_refusal_response(&r);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "run_not_editable");
        assert_eq!(body["status"], "archived");
    }
}
