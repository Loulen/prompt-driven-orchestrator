//! What a reply or a decision does to a comment (#751, ADR-0067 §4): the agent
//! proposes, the human decides, and the instance may let agents resolve.

use serde::{Deserialize, Serialize};

use super::{ReviewComment, ReviewCommentStatus};

/// How the agent's `--resolved` is treated: a proposal the human decides on,
/// or a direct resolution under the instance setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyOutcome {
    /// Plain reply, no `--resolved`.
    Replied,
    /// `--resolved` with the setting off: "Resolution proposed", Resolve / Reopen.
    Proposed,
    /// `--resolved` with the setting on: resolved on the spot (the human keeps Reopen).
    Resolved,
}

impl ReplyOutcome {
    /// The outcome of a reply to `status`: a reply on a resolved comment is
    /// just a reply, `resolved` or not.
    pub fn of(resolved: bool, status: ReviewCommentStatus, can_resolve: bool) -> ReplyOutcome {
        if !resolved || status == ReviewCommentStatus::Resolved {
            ReplyOutcome::Replied
        } else if can_resolve {
            ReplyOutcome::Resolved
        } else {
            ReplyOutcome::Proposed
        }
    }

    /// How the daemon's log names the outcome.
    pub fn describe(self) -> &'static str {
        match self {
            ReplyOutcome::Replied => "reply",
            ReplyOutcome::Proposed => "resolution proposed",
            ReplyOutcome::Resolved => "resolved directly, setting on",
        }
    }
}

/// Whether a reply `resolved: true` carries a proposal: only on a comment that
/// is not resolved yet.
pub fn proposes_resolution(resolved: bool, status: ReviewCommentStatus) -> bool {
    resolved && status != ReviewCommentStatus::Resolved
}

/// The human's verbs on a sent comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Accept a proposal, or close a comment outright.
    Resolve,
    /// Back to `sent`: reopen a resolved comment, or decline a pending proposal.
    Reopen,
}

impl Decision {
    /// Whether the decision changes `comment`. Already resolved, or a `sent`
    /// comment with nothing pending, is no change: two browsers clicking is
    /// not an error.
    pub fn changes(self, comment: &ReviewComment) -> bool {
        match self {
            Decision::Resolve => comment.status != ReviewCommentStatus::Resolved,
            Decision::Reopen => {
                comment.status == ReviewCommentStatus::Resolved || comment.proposal_pending
            }
        }
    }

    /// The Operation's action.
    pub fn action(self) -> &'static str {
        match self {
            Decision::Resolve => "resolve",
            Decision::Reopen => "reopen",
        }
    }
}

/// Who decides when no session speaks: the declared `by`, else the Review
/// page's `user`.
pub fn decider(declared: Option<&str>) -> String {
    declared
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or("user")
        .to_string()
}

/// Built-in default of `review_agent_can_resolve`: **off** — the human
/// resolves, the agent proposes.
pub const AGENT_CAN_RESOLVE_DEFAULT: bool = false;

/// Resolve `review_agent_can_resolve`: `stored → env → default(false)`.
/// `stored` is the raw column: `Some(0)` is a stored **off** and wins over the
/// env; only SQL `NULL` falls through — the same discipline as
/// `default_auto_name`.
pub fn agent_can_resolve(stored: Option<i64>, env: Option<bool>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env.unwrap_or(AGENT_CAN_RESOLVE_DEFAULT),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::sent;
    use super::*;

    #[test]
    fn review_agent_can_resolve_stored_beats_env_and_defaults_off() {
        assert!(agent_can_resolve(Some(1), Some(false)));
        assert!(!agent_can_resolve(Some(0), Some(true)));
        assert!(agent_can_resolve(None, Some(true)));
        assert_eq!(agent_can_resolve(None, None), AGENT_CAN_RESOLVE_DEFAULT);
    }

    #[test]
    fn a_resolved_reply_proposes_unless_the_instance_lets_agents_resolve() {
        let sent_status = ReviewCommentStatus::Sent;
        let resolved = ReviewCommentStatus::Resolved;
        assert_eq!(
            ReplyOutcome::of(false, sent_status, true),
            ReplyOutcome::Replied
        );
        assert_eq!(
            ReplyOutcome::of(true, sent_status, false),
            ReplyOutcome::Proposed
        );
        assert_eq!(
            ReplyOutcome::of(true, sent_status, true),
            ReplyOutcome::Resolved
        );
        assert_eq!(
            ReplyOutcome::of(true, resolved, true),
            ReplyOutcome::Replied
        );
        assert!(proposes_resolution(true, sent_status));
        assert!(!proposes_resolution(true, resolved));
        assert!(!proposes_resolution(false, sent_status));
    }

    #[test]
    fn a_decision_changes_only_what_it_would_move() {
        let mut c = sent("rc-001", 3);
        assert!(Decision::Resolve.changes(&c));
        assert!(!Decision::Reopen.changes(&c), "nothing pending");
        c.proposal_pending = true;
        assert!(Decision::Reopen.changes(&c), "declines the proposal");
        c.status = ReviewCommentStatus::Resolved;
        assert!(!Decision::Resolve.changes(&c));
        assert!(Decision::Reopen.changes(&c));
    }

    #[test]
    fn the_decider_is_the_declared_one_else_the_user() {
        assert_eq!(decider(None), "user");
        assert_eq!(decider(Some("  ")), "user");
        assert_eq!(decider(Some(" bot ")), "bot");
    }
}
