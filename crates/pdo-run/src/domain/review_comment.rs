//! Review comments as the Run folds them (#750, ADR-0067 §2): a remark sent on
//! one line of the diff between two Run refs, then its replies, resolutions and
//! reopenings. Everything from `sent` on is a Run event; this module owns the
//! projected shape of those events and their fold. The review context composes
//! the batch message, the excerpt and the anchor mapping on top of it.
//!
//! Sent comments are immutable: no event edits or deletes one. The fold is
//! additive on a terminal Run — review happens post-mortem, so the projection
//! never treats these events as a re-opening.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::event::{Event, EventKind};

/// The side of the diff a comment is anchored on. `old` = the source ref's
/// content (a deleted or context line numbered on the left), `new` = the
/// destination ref's (an added or context line numbered on the right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSide {
    Old,
    New,
}

impl ReviewSide {
    /// GitHub's letter for the anchor label: `L` for the left (old) side, `R`
    /// for the right (new) side — `ReviewPage.tsx:R48`.
    pub fn letter(self) -> char {
        match self {
            ReviewSide::Old => 'L',
            ReviewSide::New => 'R',
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ReviewSide::Old => "old",
            ReviewSide::New => "new",
        }
    }

    pub fn parse(s: &str) -> Option<ReviewSide> {
        match s {
            "old" => Some(ReviewSide::Old),
            "new" => Some(ReviewSide::New),
            _ => None,
        }
    }
}

/// Lifecycle after `sent`. `Resolved` and the reopen path are folded here so
/// #751 only adds emitters; nothing in #750 produces them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCommentStatus {
    Sent,
    Resolved,
}

/// One reply of an agent (manager or node of the Run, author recorded) to a
/// sent comment — `review_comment_replied`. Folded for #751's `pdo review reply`;
/// the shape is fixed here so the message the manager receives can already name
/// the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewReply {
    pub author: String,
    pub text: String,
    pub at: String,
    /// The agent proposes a resolution with this reply (#751 decides whether it
    /// resolves directly, under `review_agent_can_resolve`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proposes_resolution: bool,
}

/// A sent comment as projected into `RunState::review_comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewComment {
    /// `rc-001`, `rc-002`, … — sequential per Run, the id the manager echoes in
    /// `pdo review reply`.
    pub id: String,
    pub path: String,
    pub side: ReviewSide,
    pub line: i64,
    /// Stable Run ref ids of the pair the comment was written against
    /// (`fork`, `tip`, `node:<id>:<iter>:after`, …).
    pub from_ref: String,
    pub to_ref: String,
    /// The SHAs those ids resolved to at send time — what makes the anchor
    /// meaningful once the Run tip moved (#752's outdated logic reads these).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub to_sha: Option<String>,
    pub text: String,
    /// The hunk excerpt around the anchored line at send time, `>` marking it.
    /// Kept so the comment reads post-mortem even when the file is gone.
    #[serde(default)]
    pub excerpt: String,
    pub author: String,
    pub sent_at: String,
    /// The send batch this comment travelled in — one message to the manager
    /// per batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batch_id: Option<String>,
    #[serde(default = "default_status")]
    pub status: ReviewCommentStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<ReviewReply>,
    /// #751: a `--resolved` reply is waiting for the human's decision (the
    /// setting `review_agent_can_resolve` was off). Cleared by a resolve or a
    /// reopen — derived, so the UI never has to scan the thread.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proposal_pending: bool,
    /// Who resolved (`user`, `manager`, or a node id) and when — `None` while
    /// `sent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_at: Option<String>,
    /// The last reopen, kept so the footer can read "Reopened by you · time".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reopened_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reopened_at: Option<String>,
    /// The last reopen landed on a comment that was still `sent`: it declined a
    /// pending proposal rather than reopening a resolved comment. Cleared by the
    /// next reply.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proposal_declined: bool,
}

fn default_status() -> ReviewCommentStatus {
    ReviewCommentStatus::Sent
}

/// Fold one review event into the projected list. Additive only; unknown ids
/// on a reply/resolve/reopen are ignored with a warning (a hand-crafted or
/// replayed event must never panic the projection).
pub fn fold(comments: &mut Vec<ReviewComment>, event: &Event) {
    let Some(payload) = event.payload.as_ref() else {
        return;
    };
    match event.kind {
        EventKind::ReviewCommentSent => {
            match serde_json::from_value::<ReviewComment>(payload.clone()) {
                Ok(mut c) => {
                    if c.sent_at.is_empty() {
                        c.sent_at = event.ts.clone();
                    }
                    if comments.iter().any(|x| x.id == c.id) {
                        tracing::warn!(
                            "review_comment_sent replays id {} on run {}; keeping the first",
                            c.id,
                            event.run_id
                        );
                        return;
                    }
                    comments.push(c);
                }
                Err(e) => tracing::warn!(
                    "review_comment_sent carries an unreadable payload ({payload}): {e}; skipped"
                ),
            }
        }
        EventKind::ReviewCommentReplied => {
            let Some(id) = payload.get("id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(c) = comments.iter_mut().find(|c| c.id == id) else {
                tracing::warn!("review_comment_replied names unknown comment {id}; skipped");
                return;
            };
            let proposes_resolution = payload
                .get("proposes_resolution")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            c.replies.push(ReviewReply {
                author: payload
                    .get("author")
                    .and_then(|v| v.as_str())
                    .unwrap_or("agent")
                    .to_string(),
                text: payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                at: event.ts.clone(),
                proposes_resolution,
            });
            // A proposal on an open comment waits for the human; a following
            // `review_comment_resolved` (setting on) clears it right away.
            if proposes_resolution && c.status == ReviewCommentStatus::Sent {
                c.proposal_pending = true;
            }
            c.proposal_declined = false;
        }
        EventKind::ReviewCommentResolved | EventKind::ReviewCommentReopened => {
            let Some(id) = payload.get("id").and_then(|v| v.as_str()) else {
                return;
            };
            let Some(c) = comments.iter_mut().find(|c| c.id == id) else {
                tracing::warn!("{:?} names unknown comment {id}; skipped", event.kind);
                return;
            };
            let by = payload
                .get("by")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string();
            if event.kind == EventKind::ReviewCommentResolved {
                c.status = ReviewCommentStatus::Resolved;
                c.resolved_by = Some(by);
                c.resolved_at = Some(event.ts.clone());
                c.proposal_pending = false;
                c.proposal_declined = false;
            } else {
                // Reopen on a `sent` comment declines its pending proposal (the
                // spec keeps one verb for both: the event is the same).
                let declined = c.status == ReviewCommentStatus::Sent && c.proposal_pending;
                c.status = ReviewCommentStatus::Sent;
                c.resolved_by = None;
                c.resolved_at = None;
                c.reopened_by = Some(by);
                c.reopened_at = Some(event.ts.clone());
                c.proposal_pending = false;
                c.proposal_declined = declined;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(id: &str, line: i64) -> ReviewComment {
        ReviewComment {
            id: id.into(),
            path: "frontend/src/pages/ReviewPage.tsx".into(),
            side: ReviewSide::New,
            line,
            from_ref: "fork".into(),
            to_ref: "tip".into(),
            from_sha: Some("a1b2c3d4e5f6a7b8".into()),
            to_sha: Some("d4e5f6a7b8c9d0e1".into()),
            text: "`canSend` should be false\nwhile sending.".into(),
            excerpt: format!("> {line} | l{line}"),
            author: "user".into(),
            sent_at: "2026-09-09T10:00:00.000Z".into(),
            batch_id: Some("b1".into()),
            status: ReviewCommentStatus::Sent,
            replies: vec![],
            proposal_pending: false,
            resolved_by: None,
            resolved_at: None,
            reopened_by: None,
            reopened_at: None,
            proposal_declined: false,
        }
    }

    fn ev(kind: EventKind, payload: serde_json::Value) -> Event {
        Event {
            id: None,
            run_id: "r1".into(),
            ts: "2026-09-09T10:00:00.000Z".into(),
            kind,
            node_id: None,
            iter: None,
            payload: Some(payload),
        }
    }

    #[test]
    fn fold_appends_sent_comments_and_ignores_replays() {
        let mut list = vec![];
        let c = sent("rc-001", 3);
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::to_value(&c).unwrap(),
            ),
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::to_value(&c).unwrap(),
            ),
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], c);
    }

    #[test]
    fn fold_fills_sent_at_from_the_event_when_absent_and_skips_garbage() {
        let mut list = vec![];
        let mut v = serde_json::to_value(sent("rc-001", 3)).unwrap();
        v["sent_at"] = serde_json::json!("");
        fold(&mut list, &ev(EventKind::ReviewCommentSent, v));
        assert_eq!(list[0].sent_at, "2026-09-09T10:00:00.000Z");
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::json!({ "nope": 1 }),
            ),
        );
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn fold_replies_resolves_and_reopens_by_id() {
        let mut list = vec![];
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::to_value(sent("rc-001", 3)).unwrap(),
            ),
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReplied,
                serde_json::json!({ "id": "rc-001", "author": "manager", "text": "done", "proposes_resolution": true }),
            ),
        );
        assert_eq!(list[0].replies.len(), 1);
        assert_eq!(list[0].replies[0].author, "manager");
        assert!(list[0].replies[0].proposes_resolution);
        assert!(
            list[0].proposal_pending,
            "a --resolved reply waits for the human"
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentResolved,
                serde_json::json!({ "id": "rc-001" }),
            ),
        );
        assert_eq!(list[0].status, ReviewCommentStatus::Resolved);
        assert_eq!(
            list[0].resolved_by.as_deref(),
            Some("user"),
            "no `by` ⇒ the human"
        );
        assert_eq!(
            list[0].resolved_at.as_deref(),
            Some("2026-09-09T10:00:00.000Z")
        );
        assert!(!list[0].proposal_pending);
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReopened,
                serde_json::json!({ "id": "rc-001", "by": "user" }),
            ),
        );
        assert_eq!(list[0].status, ReviewCommentStatus::Sent);
        assert!(list[0].resolved_by.is_none());
        assert_eq!(list[0].reopened_by.as_deref(), Some("user"));
        assert!(
            !list[0].proposal_declined,
            "reopening a resolved comment is not a decline"
        );
        // Unknown id: ignored, never a panic.
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReplied,
                serde_json::json!({ "id": "rc-999", "text": "x" }),
            ),
        );
        assert_eq!(list[0].replies.len(), 1);
    }

    #[test]
    fn reopen_on_a_sent_comment_declines_the_pending_proposal_and_a_reply_clears_the_decline() {
        let mut list = vec![];
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::to_value(sent("rc-001", 3)).unwrap(),
            ),
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReplied,
                serde_json::json!({ "id": "rc-001", "author": "xuTJYLUa", "text": "done", "proposes_resolution": true }),
            ),
        );
        assert!(list[0].proposal_pending);
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReopened,
                serde_json::json!({ "id": "rc-001", "by": "user" }),
            ),
        );
        assert_eq!(list[0].status, ReviewCommentStatus::Sent);
        assert!(!list[0].proposal_pending);
        assert!(
            list[0].proposal_declined,
            "Reopen on a proposal = declined, comment stays open"
        );
        assert_eq!(list[0].reopened_by.as_deref(), Some("user"));
        // The agent answers again: the decline is history, the thread grows.
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReplied,
                serde_json::json!({ "id": "rc-001", "author": "xuTJYLUa", "text": "second try" }),
            ),
        );
        assert!(!list[0].proposal_declined);
        assert!(!list[0].proposal_pending);
        assert_eq!(list[0].replies.len(), 2);
    }

    #[test]
    fn agent_resolving_directly_records_the_agent_as_resolver() {
        let mut list = vec![];
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentSent,
                serde_json::to_value(sent("rc-001", 3)).unwrap(),
            ),
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentReplied,
                serde_json::json!({ "id": "rc-001", "author": "manager", "text": "fixed", "proposes_resolution": true }),
            ),
        );
        fold(
            &mut list,
            &ev(
                EventKind::ReviewCommentResolved,
                serde_json::json!({ "id": "rc-001", "by": "manager" }),
            ),
        );
        assert_eq!(list[0].status, ReviewCommentStatus::Resolved);
        assert_eq!(list[0].resolved_by.as_deref(), Some("manager"));
        assert!(
            !list[0].proposal_pending,
            "the direct resolution clears the proposal flag"
        );
        let wire = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(wire["resolved_by"], "manager");
        assert!(
            wire.get("proposal_pending").is_none(),
            "false flags stay off the wire"
        );
    }

    #[test]
    fn serde_round_trip_keeps_the_wire_names() {
        let v = serde_json::to_value(sent("rc-001", 3)).unwrap();
        assert_eq!(v["side"], "new");
        assert_eq!(v["status"], "sent");
        assert!(v.get("replies").is_none(), "empty replies are skipped");
        let back: ReviewComment = serde_json::from_value(v).unwrap();
        assert_eq!(back.side, ReviewSide::New);
    }
}
