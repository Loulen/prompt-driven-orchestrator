//! Review comments (#750, ADR-0067 §2; CONTEXT.md § "Commentaire de review",
//! "Envoi au manager").
//!
//! A review comment is a remark anchored on one line of one file of the diff
//! between two Run refs: `(path, side, line, from, to)`. Its `draft` state lives
//! in the browser only. From `sent` on, everything is a **Run event**: the
//! comment itself (`review_comment_sent`), the agent's replies, the resolutions
//! and the reopenings. The projected shape of those events and their fold
//! belong to the run context (`pdo_run::domain::review_comment`); this module
//! owns the id scheme, the hunk excerpt, the author of a reply and the filter
//! of the list; [`anchor`] the re-map of an anchor, [`decision`] what a reply
//! or a decision changes, [`message`] the **single batch message** the
//! Pipeline Manager receives per send.
//!
//! Sent comments are immutable: no event edits or deletes one, and nothing here
//! offers to.

pub mod anchor;
pub mod decision;
pub mod message;

pub use pdo_run::domain::review_comment::{
    ReviewComment, ReviewCommentStatus, ReviewReply, ReviewSide,
};

/// The `PDO_NODE_ID` the manager's tmux session is wrapped with (see
/// `spawn_manager_session`): the one session id that is not a node.
pub const MANAGER_NODE_ID: &str = "__manager__";

/// The author of a reply, deduced from the session that issued it: the manager
/// session reads `manager`, a node session reads its node id. Without a session
/// claim the caller's own `author` field is taken, else the neutral `agent`.
pub fn reply_author(session_node_id: Option<&str>, declared: Option<&str>) -> String {
    match session_node_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(MANAGER_NODE_ID) => "manager".to_string(),
        Some(node) => node.to_string(),
        None => declared
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("agent")
            .to_string(),
    }
}

/// `?state=` of the list endpoint / `--state` of `pdo review list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    Open,
    Resolved,
    All,
}

impl StateFilter {
    pub fn parse(s: &str) -> Option<StateFilter> {
        match s.trim() {
            "open" | "sent" => Some(StateFilter::Open),
            "resolved" => Some(StateFilter::Resolved),
            "all" | "" => Some(StateFilter::All),
            _ => None,
        }
    }

    pub fn keeps(self, c: &ReviewComment) -> bool {
        match self {
            StateFilter::All => true,
            StateFilter::Open => c.status == ReviewCommentStatus::Sent,
            StateFilter::Resolved => c.status == ReviewCommentStatus::Resolved,
        }
    }
}

/// `rc-NNN` for the `n`-th comment of the Run (1-based). Three digits keep the
/// ids aligned in the manager's message; past 999 the number simply widens.
pub fn comment_id(n: usize) -> String {
    format!("rc-{n:03}")
}

/// The excerpt of `content` around `line` (1-based): `context` lines on each
/// side, numbered, the anchored line marked with `>`. Empty when the line is
/// outside the file (the anchor no longer resolves — the comment still sends).
pub fn excerpt(content: &str, line: i64, context: usize) -> String {
    if line < 1 {
        return String::new();
    }
    let lines: Vec<&str> = content.lines().collect();
    let idx = (line - 1) as usize;
    if idx >= lines.len() {
        return String::new();
    }
    let start = idx.saturating_sub(context);
    let end = (idx + context + 1).min(lines.len());
    let width = end.to_string().len();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate().take(end).skip(start) {
        let marker = if i == idx { '>' } else { ' ' };
        out.push_str(&format!("{marker} {:>width$} | {l}\n", i + 1));
    }
    out.trim_end_matches('\n').to_string()
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A `sent` comment of a test, anchored on `line` of a review page.
    pub(crate) fn sent(id: &str, line: i64) -> ReviewComment {
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
            excerpt: excerpt("l1\nl2\nl3\nl4\nl5\nl6", line, 2),
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
}

#[cfg(test)]
mod tests {
    use super::fixtures::sent;
    use super::*;

    #[test]
    fn ids_are_sequential_and_padded() {
        assert_eq!(comment_id(1), "rc-001");
        assert_eq!(comment_id(42), "rc-042");
        assert_eq!(comment_id(1000), "rc-1000");
    }

    #[test]
    fn excerpt_marks_the_line_and_clamps_at_the_edges() {
        let e = excerpt("a\nb\nc\nd\ne", 2, 2);
        assert_eq!(e, "  1 | a\n> 2 | b\n  3 | c\n  4 | d");
        let tail = excerpt("a\nb\nc", 3, 1);
        assert_eq!(tail, "  2 | b\n> 3 | c");
        assert_eq!(excerpt("a\nb", 5, 2), "");
        assert_eq!(excerpt("a\nb", 0, 2), "");
    }

    #[test]
    fn reply_author_comes_from_the_session_then_the_declared_field() {
        assert_eq!(reply_author(Some(MANAGER_NODE_ID), None), "manager");
        assert_eq!(reply_author(Some("xuTJYLUa"), Some("ignored")), "xuTJYLUa");
        assert_eq!(reply_author(None, Some(" fixer ")), "fixer");
        assert_eq!(reply_author(Some("  "), None), "agent");
        assert_eq!(reply_author(None, None), "agent");
    }

    #[test]
    fn state_filter_parses_and_keeps_by_status() {
        let mut resolved = sent("rc-002", 5);
        resolved.status = ReviewCommentStatus::Resolved;
        let open = sent("rc-001", 3);
        assert_eq!(StateFilter::parse("open"), Some(StateFilter::Open));
        assert_eq!(StateFilter::parse("resolved"), Some(StateFilter::Resolved));
        assert_eq!(StateFilter::parse("all"), Some(StateFilter::All));
        assert_eq!(StateFilter::parse("nope"), None);
        assert!(StateFilter::Open.keeps(&open) && !StateFilter::Open.keeps(&resolved));
        assert!(!StateFilter::Resolved.keeps(&open) && StateFilter::Resolved.keeps(&resolved));
        assert!(StateFilter::All.keeps(&open) && StateFilter::All.keeps(&resolved));
    }
}
