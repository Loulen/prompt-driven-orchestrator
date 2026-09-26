//! Review comments (#750, ADR-0067 §2; CONTEXT.md § "Commentaire de review",
//! "Envoi au manager").
//!
//! A review comment is a remark anchored on one line of one file of the diff
//! between two Run refs: `(path, side, line, from, to)`. Its `draft` state lives
//! in the browser only. From `sent` on, everything is a **Run event**: the
//! comment itself (`review_comment_sent`), and — for the next ticket (#751) — the
//! agent's replies, the resolutions and the reopenings. This module owns the
//! projected shape of those events, the id scheme, the hunk excerpt and the
//! **single batch message** the Pipeline Manager receives per send.
//!
//! Sent comments are immutable: no event edits or deletes one, and nothing here
//! offers to. The fold is additive on a terminal Run — review happens on a
//! completed Run, post-mortem, so the projection must never treat these events
//! as a re-opening.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::event_log::{Event, EventKind};

/// The side of the diff a comment is anchored on. `old` = the source ref's
/// content (a deleted or context line numbered on the left), `new` = the
/// destination ref's (an added or context line numbered on the right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewSide {
    Old,
    New,
}

impl ReviewSide {
    /// GitHub's letter for the anchor label: `L` for the left (old) side, `R`
    /// for the right (new) side — `ReviewPage.tsx:R48`.
    pub(crate) fn letter(self) -> char {
        match self {
            ReviewSide::Old => 'L',
            ReviewSide::New => 'R',
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReviewSide::Old => "old",
            ReviewSide::New => "new",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<ReviewSide> {
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
pub(crate) enum ReviewCommentStatus {
    Sent,
    Resolved,
}

/// One reply of an agent (manager or node of the Run, author recorded) to a
/// sent comment — `review_comment_replied`. Folded for #751's `pdo review reply`;
/// the shape is fixed here so the message the manager receives can already name
/// the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) struct ReviewReply {
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
pub(crate) struct ReviewComment {
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

/// How the agent's `--resolved` is treated (#751, ADR-0067 §4): a proposal the
/// human decides on, or a direct resolution under the instance setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplyOutcome {
    /// Plain reply, no `--resolved`.
    Replied,
    /// `--resolved` with the setting off: "Resolution proposed", Resolve / Reopen.
    Proposed,
    /// `--resolved` with the setting on: resolved on the spot (the human keeps Reopen).
    Resolved,
}

/// Middle tier of `stored → env → default(false)`; resolved by
/// [`review_agent_can_resolve_with`].
pub(crate) const REVIEW_AGENT_CAN_RESOLVE_ENV: &str = "PDO_REVIEW_AGENT_CAN_RESOLVE";

/// Built-in default: **off** — the human resolves, the agent proposes (ADR-0067 §4).
pub(crate) const REVIEW_AGENT_CAN_RESOLVE_DEFAULT: bool = false;

/// The env tier, through the shared boolean parser so a typo falls through to
/// the default rather than silently meaning `false`.
pub(crate) fn env_review_agent_can_resolve() -> Option<bool> {
    std::env::var(REVIEW_AGENT_CAN_RESOLVE_ENV)
        .ok()
        .as_deref()
        .and_then(crate::stale_detector::parse_bool_setting)
}

/// Resolve `review_agent_can_resolve`: `stored → env → default(false)`. `stored`
/// is the raw `instance_config.review_agent_can_resolve` column: `Some(0)` is a
/// stored **off** and wins over the env; only SQL `NULL` falls through — the
/// same discipline as `default_auto_name`.
pub(crate) fn review_agent_can_resolve_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_review_agent_can_resolve().unwrap_or(REVIEW_AGENT_CAN_RESOLVE_DEFAULT),
    }
}

/// The `PDO_NODE_ID` the manager's tmux session is wrapped with (see
/// `spawn_manager_session`): the one session id that is not a node.
pub(crate) const MANAGER_NODE_ID: &str = "__manager__";

/// The author of a reply, deduced from the session that issued it: the manager
/// session reads `manager`, a node session reads its node id. Without a session
/// claim the caller's own `author` field is taken, else the neutral `agent`.
pub(crate) fn reply_author(session_node_id: Option<&str>, declared: Option<&str>) -> String {
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
pub(crate) enum StateFilter {
    Open,
    Resolved,
    All,
}

impl StateFilter {
    pub(crate) fn parse(s: &str) -> Option<StateFilter> {
        match s.trim() {
            "open" | "sent" => Some(StateFilter::Open),
            "resolved" => Some(StateFilter::Resolved),
            "all" | "" => Some(StateFilter::All),
            _ => None,
        }
    }

    pub(crate) fn keeps(self, c: &ReviewComment) -> bool {
        match self {
            StateFilter::All => true,
            StateFilter::Open => c.status == ReviewCommentStatus::Sent,
            StateFilter::Resolved => c.status == ReviewCommentStatus::Resolved,
        }
    }
}

fn default_status() -> ReviewCommentStatus {
    ReviewCommentStatus::Sent
}

/// `rc-NNN` for the `n`-th comment of the Run (1-based). Three digits keep the
/// ids aligned in the manager's message; past 999 the number simply widens.
pub(crate) fn comment_id(n: usize) -> String {
    format!("rc-{n:03}")
}

/// The excerpt of `content` around `line` (1-based): `context` lines on each
/// side, numbered, the anchored line marked with `>`. Empty when the line is
/// outside the file (the anchor no longer resolves — the comment still sends).
pub(crate) fn excerpt(content: &str, line: i64, context: usize) -> String {
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

/// The **one message** a batch of freshly sent comments produces for the
/// Pipeline Manager (CONTEXT.md « Envoi au manager »): per comment its id, the
/// anchor, the ref pair, the hunk excerpt, the text, and the instruction to
/// answer through `pdo review reply`. Never one message per comment — the
/// manager would answer N times.
pub(crate) fn batch_message(
    run_id: &str,
    from_label: &str,
    to_label: &str,
    comments: &[ReviewComment],
) -> String {
    let n = comments.len();
    let mut out = format!(
        "Review comments — {n} new on Run {run_id} ({from_label} → {to_label}).\n\
         A human reviewed the diff between these two Run refs and left the remarks below. \
         Each one is an immutable Run event with its own id.\n\n"
    );
    for c in comments {
        let short =
            |s: &Option<String>| s.as_deref().map(|x| x.chars().take(7).collect::<String>());
        let shas = match (short(&c.from_sha), short(&c.to_sha)) {
            (Some(f), Some(t)) => format!(" ({f} → {t})"),
            _ => String::new(),
        };
        out.push_str(&format!(
            "[{id}] {path}:{letter}{line} · {side} side · {from} → {to}{shas}\n",
            id = c.id,
            path = c.path,
            letter = c.side.letter(),
            line = c.line,
            side = c.side.as_str(),
            from = c.from_ref,
            to = c.to_ref,
        ));
        if !c.excerpt.is_empty() {
            for l in c.excerpt.lines() {
                out.push_str("    ");
                out.push_str(l);
                out.push('\n');
            }
        }
        out.push_str("  Comment:\n");
        for l in c.text.lines() {
            out.push_str("  ");
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(
        "Reply to each comment with `pdo review reply <id> --text \"<your answer>\"` \
         (add `--resolved` once you consider it addressed; `pdo review list` lists the open ones). \
         Address the code the comment points at — fix it on the Run branch, or explain why not. \
         Comments themselves are immutable: never try to edit or delete one.",
    );
    out
}

// ---------------------------------------------------------------------------
// #752 — re-map of an anchor onto another destination (ADR-0067 §5).
// ---------------------------------------------------------------------------

/// Where a comment's line is once its anchored side is read at another ref:
/// **reported** at `line` (the line is unchanged — it may have shifted), or
/// **outdated** (the line was edited or deleted). Computed on read, never
/// written to the log: the anchor in the event stays the truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineMapping {
    Reported { line: i64 },
    Outdated,
}

/// Follow `line` (1-based, on the **old** side of `files`) through the hunks of
/// the two-dot diff `orig → target` of one path. A context line keeps its
/// content: reported at its new number. A deleted line — an edit is a delete
/// plus an add — is outdated. A line the diff never mentions shifts by the net
/// growth of the hunks above it. An empty `files` means identical content: the
/// line stays where it is. A file added or deleted between the refs has no
/// line to follow.
pub(crate) fn map_line(files: &[crate::structured_diff::FileDiff], line: i64) -> LineMapping {
    use crate::structured_diff::{FileStatus, LineKind};
    if line < 1 {
        return LineMapping::Outdated;
    }
    let Some(file) = files.first() else {
        return LineMapping::Reported { line };
    };
    if file.binary || matches!(file.status, FileStatus::Added | FileStatus::Deleted) {
        return LineMapping::Outdated;
    }
    let line_u = line as u32;
    let mut delta: i64 = 0;
    for hunk in &file.hunks {
        if hunk.old_lines == 0 {
            // Pure insertion: `-N,0` inserts AFTER old line N. Line N and
            // everything above stay; everything below shifts.
            if line_u <= hunk.old_start {
                break;
            }
            delta += hunk.new_lines as i64;
            continue;
        }
        if line_u < hunk.old_start {
            break;
        }
        if line_u < hunk.old_start + hunk.old_lines {
            // Inside the hunk: the line itself is listed, one way or another.
            for l in &hunk.lines {
                if l.old_no == Some(line_u) {
                    return match (l.kind, l.new_no) {
                        (LineKind::Context, Some(n)) => LineMapping::Reported { line: n as i64 },
                        _ => LineMapping::Outdated,
                    };
                }
            }
            // Listed hunk without our line (a malformed patch): be conservative.
            return LineMapping::Outdated;
        }
        delta += hunk.new_lines as i64 - hunk.old_lines as i64;
    }
    LineMapping::Reported { line: line + delta }
}

/// The annotation `GET …/review/comments?from=&to=` and `pdo review list` add to
/// a comment for a displayed pair: `outdated`, and the reported line when the
/// comment still has one. `None` when the mapping cannot be computed (no SHA
/// recorded at send time, a ref that no longer resolves): the caller shows the
/// comment where it was written, without pretending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "ReviewAnchorMapping")]
pub(crate) struct AnchorMapping {
    pub outdated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mapped_line: Option<i64>,
    /// The reported line differs from the written one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional = nullable)]
    pub moved: bool,
}

impl AnchorMapping {
    pub(crate) fn from_line(original: i64, mapping: LineMapping) -> AnchorMapping {
        match mapping {
            LineMapping::Reported { line } => AnchorMapping {
                outdated: false,
                mapped_line: Some(line),
                moved: line != original,
            },
            LineMapping::Outdated => AnchorMapping {
                outdated: true,
                mapped_line: None,
                moved: false,
            },
        }
    }
}

/// The SHA a comment's anchored side was written against: `to_sha` for the
/// destination (`new`) side, `from_sha` for the source (`old`) side — old-side
/// comments follow the same rule on the source ref.
pub(crate) fn anchored_sha(c: &ReviewComment) -> Option<&str> {
    match c.side {
        ReviewSide::New => c.to_sha.as_deref(),
        ReviewSide::Old => c.from_sha.as_deref(),
    }
}

/// `(orig, target, path) -> parsed two-dot diff of that path`, or `None` when
/// git cannot answer — the one seam [`map_comment`] needs from the repository.
pub(crate) type DiffPathFn<'a> =
    &'a dyn Fn(&str, &str, &str) -> Option<Vec<crate::structured_diff::FileDiff>>;

/// Map one comment onto the displayed pair `(from_sha, to_sha)`. `diff_path`
/// yields the parsed two-dot diff of the comment's path between two SHAs
/// (`None` when git cannot answer — unknown revision). Pure apart from that
/// closure, so the algorithm is testable without a repository.
pub(crate) fn map_comment(
    c: &ReviewComment,
    from_sha: Option<&str>,
    to_sha: Option<&str>,
    diff_path: DiffPathFn<'_>,
) -> Option<AnchorMapping> {
    let orig = anchored_sha(c)?;
    let target = match c.side {
        ReviewSide::New => to_sha?,
        ReviewSide::Old => from_sha?,
    };
    if orig == target {
        return Some(AnchorMapping::from_line(
            c.line,
            LineMapping::Reported { line: c.line },
        ));
    }
    let files = diff_path(orig, target, &c.path)?;
    Some(AnchorMapping::from_line(c.line, map_line(&files, c.line)))
}

/// Fold one review event into the projected list. Additive only; unknown ids
/// on a reply/resolve/reopen are ignored with a warning (a hand-crafted or
/// replayed event must never panic the projection).
pub(crate) fn fold(comments: &mut Vec<ReviewComment>, event: &Event) {
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
    fn batch_message_is_one_message_with_ids_anchors_refs_excerpts_and_reply_instruction() {
        let msg = batch_message(
            "run-1",
            "Fork point",
            "Run tip",
            &[sent("rc-001", 3), sent("rc-002", 5)],
        );
        assert!(msg.starts_with("Review comments — 2 new on Run run-1 (Fork point → Run tip)."));
        assert!(msg.contains("[rc-001] frontend/src/pages/ReviewPage.tsx:R3 · new side · fork → tip (a1b2c3d → d4e5f6a)"));
        assert!(msg.contains("[rc-002] frontend/src/pages/ReviewPage.tsx:R5"));
        assert!(msg.contains("    > 3 | l3"), "excerpt with marker: {msg}");
        assert!(msg.contains("  `canSend` should be false\n  while sending."));
        assert!(msg.contains("pdo review reply <id> --text"));
        assert!(msg.contains("pdo review list"));
        assert!(msg.contains("immutable"));
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

    #[test]
    fn review_agent_can_resolve_stored_beats_env_and_defaults_off() {
        // Stored decisions win whatever the env says; NULL falls through to the
        // built-in default (off). The env tier itself is covered by the settings
        // view test — process-global env is not toggled here.
        assert!(review_agent_can_resolve_with(Some(1)));
        assert!(!review_agent_can_resolve_with(Some(0)));
        if env_review_agent_can_resolve().is_none() {
            assert_eq!(
                review_agent_can_resolve_with(None),
                REVIEW_AGENT_CAN_RESOLVE_DEFAULT
            );
        }
    }

    fn patch(files: &str) -> Vec<crate::structured_diff::FileDiff> {
        crate::structured_diff::parse_patch(files)
    }

    const EDIT_ONE_LINE: &str = "diff --git a/f.rs b/f.rs\nindex 1..2 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -3 +3 @@\n-fn c() {}\n+fn c() { 1 }\n";
    const INSERT_ABOVE: &str = "diff --git a/f.rs b/f.rs\nindex 1..2 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,0 +2,2 @@\n+// one\n+// two\n";
    const DELETE_FILE: &str = "diff --git a/f.rs b/f.rs\ndeleted file mode 100644\nindex 1..0\n--- a/f.rs\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-a\n-b\n-c\n";

    #[test]
    fn map_line_reports_an_untouched_line_and_shifts_it_below_an_insertion() {
        // Identical content: the line stays.
        assert_eq!(map_line(&[], 7), LineMapping::Reported { line: 7 });
        // Two lines inserted after line 1: line 1 stays, line 2 becomes 4.
        let p = patch(INSERT_ABOVE);
        assert_eq!(map_line(&p, 1), LineMapping::Reported { line: 1 });
        assert_eq!(map_line(&p, 2), LineMapping::Reported { line: 4 });
        assert_eq!(map_line(&p, 9), LineMapping::Reported { line: 11 });
    }

    #[test]
    fn map_line_marks_an_edited_or_deleted_line_outdated_and_keeps_its_neighbours() {
        let p = patch(EDIT_ONE_LINE);
        assert_eq!(map_line(&p, 3), LineMapping::Outdated);
        assert_eq!(map_line(&p, 2), LineMapping::Reported { line: 2 });
        assert_eq!(map_line(&p, 4), LineMapping::Reported { line: 4 });
        assert_eq!(map_line(&patch(DELETE_FILE), 2), LineMapping::Outdated);
        assert_eq!(map_line(&[], 0), LineMapping::Outdated);
    }

    #[test]
    fn map_comment_uses_the_anchored_side_and_short_circuits_on_the_same_sha() {
        let c = sent("rc-001", 3);
        let calls = std::cell::Cell::new(0);
        let diff = |_: &str, _: &str, _: &str| {
            calls.set(calls.get() + 1);
            Some(patch(EDIT_ONE_LINE))
        };
        // Same destination SHA as written: reported in place, no git.
        let m = map_comment(
            &c,
            Some("a1b2c3d4e5f6a7b8"),
            Some("d4e5f6a7b8c9d0e1"),
            &diff,
        )
        .unwrap();
        assert_eq!(
            m,
            AnchorMapping {
                outdated: false,
                mapped_line: Some(3),
                moved: false
            }
        );
        assert_eq!(calls.get(), 0);
        // Destination moved: the new side is mapped, line 3 was edited.
        let m = map_comment(&c, Some("a1b2c3d4e5f6a7b8"), Some("ffff"), &diff).unwrap();
        assert!(m.outdated && m.mapped_line.is_none());
        assert_eq!(calls.get(), 1);
        // Old-side comment follows the SOURCE ref: destination moving is irrelevant.
        let mut old = sent("rc-002", 2);
        old.side = ReviewSide::Old;
        let m = map_comment(&old, Some("a1b2c3d4e5f6a7b8"), Some("ffff"), &diff).unwrap();
        assert_eq!(
            m,
            AnchorMapping {
                outdated: false,
                mapped_line: Some(2),
                moved: false
            }
        );
        let m = map_comment(&old, Some("eeee"), Some("ffff"), &diff).unwrap();
        assert_eq!(
            m.mapped_line,
            Some(2),
            "line 2 is untouched by the edit of line 3"
        );
        // No SHA recorded / ref gone: no verdict.
        let mut bare = sent("rc-003", 3);
        bare.to_sha = None;
        assert!(map_comment(&bare, Some("x"), Some("y"), &diff).is_none());
        assert!(map_comment(&c, Some("x"), None, &diff).is_none());
        // git cannot answer: no verdict either.
        assert!(map_comment(&c, Some("x"), Some("y"), &|_, _, _| None).is_none());
        let wire = serde_json::to_value(AnchorMapping::from_line(
            3,
            LineMapping::Reported { line: 5 },
        ))
        .unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "outdated": false, "mapped_line": 5, "moved": true })
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
