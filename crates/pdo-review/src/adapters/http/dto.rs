//! The bodies and queries of the review's routes.

use serde::Deserialize;
use ts_rs::TS;

use crate::application::send_comments::DraftComment;

#[derive(Deserialize)]
pub struct StructuredDiffQuery {
    /// Source ref. Defaults to the Run's fork point.
    pub from: Option<String>,
    /// Destination ref. Defaults to the Run's working tree while it exists
    /// (#835), else to the Run's tip (`pdo/run-<id>`).
    pub to: Option<String>,
}

#[derive(Deserialize)]
pub struct ListReviewCommentsQuery {
    /// `open` (default for the CLI) | `resolved` | `all` (default here: the UI
    /// wants everything, resolved ones collapse client-side).
    #[serde(default)]
    pub state: Option<String>,
    /// #752: the displayed pair to **re-map** every comment onto.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

#[derive(Deserialize)]
pub struct ReplyReviewCommentRequest {
    pub text: String,
    /// The agent considers the comment addressed.
    #[serde(default)]
    pub resolved: bool,
    /// Fallback author when the call carries no session headers (the CLI
    /// always sends them from a node or manager session).
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ReviewDecisionRequest {
    /// Who decides — `user` (the Review page, default) or an agent id for a
    /// scripted resolution.
    #[serde(default)]
    pub by: Option<String>,
}

/// One draft the browser hands over for sending (#750). `from`/`to` are stable
/// Run ref ids (default `fork` → `tip`), never SHAs.
#[derive(Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct SendReviewCommentInput {
    pub path: String,
    pub side: String,
    pub line: i64,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    pub text: String,
}

impl From<SendReviewCommentInput> for DraftComment {
    fn from(c: SendReviewCommentInput) -> Self {
        DraftComment {
            path: c.path,
            side: c.side,
            line: c.line,
            from: c.from,
            to: c.to,
            text: c.text,
        }
    }
}

#[derive(Deserialize)]
pub struct SendReviewCommentsRequest {
    pub comments: Vec<SendReviewCommentInput>,
}

/// « Send again » (CORE-239): the sent comments to hand the manager once more,
/// as written.
#[derive(Deserialize)]
pub struct ResendReviewCommentsRequest {
    pub ids: Vec<String>,
}
