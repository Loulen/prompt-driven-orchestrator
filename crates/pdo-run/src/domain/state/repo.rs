//! The repositories a Run is pinned to beside its primary one, and why a
//! pre-cut fetch did not happen.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A secondary repository pinned to a Run (#465, ADR-0042).
///
/// `target_repos[0]` is the **primary** repo and mirrors the legacy scalar
/// `target_repo` exactly (ADR-0033) — it is not represented as a `RepoPin` in
/// slice 1, it stays in `target_repo`. `RepoPin` describes each **secondary**
/// (index `[1..]`): a read-only snapshot materialised by
/// `git worktree add --detach <sha>` under
/// `<primary>/.pdo/runs/<id>/repos/<alias>/`.
///
/// The `sha` is resolved once at Run start (`git rev-parse --verify` against
/// `base_branch`, no fetch → base is the LOCAL ref) and frozen in the
/// `RunStarted` payload, exactly like the sandbox freeze siblings — so a Run's
/// view of a secondary can never move under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepoPin {
    /// Absolute path of the secondary repository on the host.
    pub repo: String,
    /// Directory name of the snapshot under `.pdo/runs/<id>/repos/`, disambiguated
    /// on basename collision. Never derived from the basename alone at read time.
    pub alias: String,
    /// The commit the snapshot is detached at, frozen at Run start.
    pub sha: String,
    /// The ref the SHA was resolved from (default: `HEAD`, the local ref). Kept
    /// for provenance / UI display; the SHA is what is authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub base_branch: Option<String>,
    /// Opt-in read-only flag (ADR-0047). `false` (the default) means the
    /// secondary is **writable**: the agent may modify/commit/deliver it and the
    /// completion guard tolerates a dirty tracked tree. `true` restores the
    /// ADR-0042 behaviour — the snapshot is read-only context and writing a
    /// tracked file trips `secondary_repo_dirtied` (409).
    ///
    /// Polarity: `#[serde(default)]` reads an absent key as `false`, so a
    /// historical pin (written before this flag existed) is treated as writable.
    /// This is intentional and safe — see ADR-0047 decision 2.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
}

/// serde `skip_serializing_if` helper: a `read_only == false` pin serialises
/// byte-identically to a pre-ADR-0047 pin (no `read_only` key).
fn is_false(b: &bool) -> bool {
    !*b
}

/// Why a Trigger's **pre-cut fetch** did not happen (#804, ADR-0070 §1) — the
/// `{kind, message}` pair the branch verbs already answer, so the Run detail and
/// the launch form speak of a failed fetch in exactly one shape.
///
/// `kind` is a plain `String`, deliberately NOT the daemon's closed
/// `FetchErrorKind` enum: this is projected out of a stored payload, and a Run
/// written by a newer binary that named a kind this one has never heard of must
/// still read back — historical data stays readable, that is the one guarantee
/// the project makes across versions. The surface renders `message` anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SourceFetchError {
    pub kind: String,
    pub message: String,
}
