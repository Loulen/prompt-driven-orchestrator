//! `/sessions/support*`: the Agent de support of the caller, one per Actor.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::support::{ChoiceRefusal, StillThere, SupportError, SupportStatus};
use crate::domain::support::SupportAgent;
use crate::domain::support_sources::CoreSourceRefusal;
use crate::Session;

/// The agent an Agent de support runs.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SupportAgentView {
    pub choice: AgentChoice,
    /// The profile's name, or `Custom`.
    pub label: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// RFC 3339.
    pub opened_at: String,
}

impl From<SupportAgent> for SupportAgentView {
    fn from(agent: SupportAgent) -> Self {
        Self {
            choice: agent.choice,
            label: agent.label,
            harness: agent.harness,
            model: agent.model,
            effort: agent.effort,
            opened_at: agent.opened_at,
        }
    }
}

/// `GET /sessions/support`, and the answer of every gesture: the caller's
/// Agent de support.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SupportStatusResponse {
    /// Its session's name, live or not.
    pub session: String,
    pub alive: bool,
    /// The agent it runs, while it lives.
    pub agent: Option<SupportAgentView>,
    /// The choice the caller last opened one with.
    pub last_choice: Option<AgentChoice>,
    /// The caller's last input into it (RFC 3339), while it lives.
    pub last_input_at: Option<String>,
    /// When it last wrote in its terminal (RFC 3339), while it lives: a new
    /// answer for a tab that does not show it.
    pub last_output_at: Option<String>,
    /// « Still there? », while it is asked.
    pub still_there: Option<StillThereView>,
}

/// « Still there? », asked: the agent closes at `closes_at` without an
/// answer.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct StillThereView {
    /// RFC 3339.
    pub closes_at: String,
    /// The seconds left now: what a countdown reads, whatever its clock says.
    pub closes_in_secs: u32,
    /// The idle timeout it was asked after.
    pub idle_timeout_secs: u32,
}

impl From<StillThere> for StillThereView {
    fn from(still: StillThere) -> Self {
        Self {
            closes_at: still.closes_at,
            closes_in_secs: still.closes_in_secs,
            idle_timeout_secs: still.idle_timeout_secs,
        }
    }
}

impl From<SupportStatus> for SupportStatusResponse {
    fn from(status: SupportStatus) -> Self {
        Self {
            session: status.session,
            alive: status.alive,
            agent: status.agent.map(Into::into),
            last_choice: status.last_choice,
            last_input_at: status.last_input_at,
            last_output_at: status.last_output_at,
            still_there: status.still_there.map(Into::into),
        }
    }
}

/// Body of `POST /sessions/support`.
#[derive(Deserialize, TS)]
#[ts(export)]
pub struct OpenSupportRequest {
    /// A profile or a custom combination; `inherit` is refused.
    pub choice: AgentChoice,
    /// The first question; none opens the agent waiting for one.
    #[serde(default)]
    #[ts(optional)]
    pub question: Option<String>,
    /// Close the agent open now, if any, and open this one.
    #[serde(default)]
    #[ts(optional)]
    pub replace: Option<bool>,
}

/// `POST /sessions/support/source`: the clone of the core at the instance's
/// tag.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct CoreSourceResponse {
    /// The tag of the core this instance is built on (`v2.80.4`).
    pub tag: String,
    /// The folder of the clone, on the daemon's machine.
    pub path: String,
}

/// Body of `POST /sessions/support/ask`.
#[derive(Deserialize, TS)]
#[ts(export)]
pub struct AskSupportRequest {
    pub question: String,
}

pub(super) async fn status(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    Json(SupportStatusResponse::from(
        session.support().status(&actor),
    ))
    .into_response()
}

pub(super) async fn open(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<OpenSupportRequest>,
) -> Response {
    let support = session.support();
    match support
        .open(
            &actor,
            &req.choice,
            req.question.as_deref(),
            req.replace.unwrap_or(false),
        )
        .await
    {
        Ok(_) => (
            StatusCode::CREATED,
            Json(SupportStatusResponse::from(support.status(&actor))),
        )
            .into_response(),
        Err(e) => refused(e),
    }
}

pub(super) async fn ask(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<AskSupportRequest>,
) -> Response {
    let support = session.support();
    match support.ask(&actor, &req.question) {
        Ok(_) => Json(SupportStatusResponse::from(support.status(&actor))).into_response(),
        Err(e) => refused(e),
    }
}

/// `POST /sessions/support/keep`: « Keep », the caller's agent stays.
pub(super) async fn keep(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    let support = session.support();
    match support.keep(&actor) {
        Ok(_) => Json(SupportStatusResponse::from(support.status(&actor))).into_response(),
        Err(e) => refused(e),
    }
}

/// `POST /sessions/support/source`: the core's code at the instance's tag.
pub(super) async fn source(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match session.support().core_source(&actor).await {
        Ok(checkout) => Json(CoreSourceResponse {
            tag: checkout.tag,
            path: checkout.dir.display().to_string(),
        })
        .into_response(),
        Err(e) => refused(e),
    }
}

pub(super) async fn close(
    State(session): State<Arc<Session>>,
    Extension(actor): Extension<Actor>,
) -> Response {
    let support = session.support();
    match support.close(&actor) {
        Ok(_) => Json(SupportStatusResponse::from(support.status(&actor))).into_response(),
        Err(denied) => denied.into_response(),
    }
}

/// A gesture not made, as a JSON body `{error, message}`: `error` names the
/// case for a client, `message` says it to a person.
fn refused(e: SupportError) -> Response {
    let (status, error, message) = match e {
        SupportError::Refused(denied) => return denied.into_response(),
        SupportError::Choice(refusal) => {
            let status = match refusal {
                ChoiceRefusal::NotInstalled(_) => StatusCode::CONFLICT,
                ChoiceRefusal::Unreadable(_) => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::BAD_REQUEST,
            };
            (status, choice_code(&refusal), refusal.to_string())
        }
        SupportError::AlreadyOpen(agent) => (
            StatusCode::CONFLICT,
            "support_agent_open",
            match agent {
                Some(agent) => format!(
                    "a support agent is open ({}): ask it, or close it to open another",
                    agent.harness
                ),
                None => "a support agent is open: ask it, or close it to open another".into(),
            },
        ),
        SupportError::NotOpen => (
            StatusCode::NOT_FOUND,
            "no_support_agent",
            "no support agent is open: open one first".into(),
        ),
        SupportError::NotDelivered => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "question_not_delivered",
            "the question did not reach the support agent: try again".into(),
        ),
        SupportError::EmptyQuestion => (
            StatusCode::BAD_REQUEST,
            "empty_question",
            "the question is empty".into(),
        ),
        SupportError::CoreSource(refusal) => {
            let (status, error) = match refusal {
                CoreSourceRefusal::NoPublicTag { .. } => (StatusCode::CONFLICT, "no_public_tag"),
                CoreSourceRefusal::Unchecked { .. } => {
                    (StatusCode::SERVICE_UNAVAILABLE, "core_source_unchecked")
                }
                CoreSourceRefusal::CloneFailed { .. } => {
                    (StatusCode::BAD_GATEWAY, "core_source_clone_failed")
                }
            };
            (status, error, refusal.to_string())
        }
        SupportError::Folder(message) | SupportError::Spawn(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "support_agent_failed",
            message,
        ),
    };
    (
        status,
        Json(serde_json::json!({ "error": error, "message": message })),
    )
        .into_response()
}

fn choice_code(refusal: &ChoiceRefusal) -> &'static str {
    match refusal {
        ChoiceRefusal::Inherit => "agent_required",
        ChoiceRefusal::UnknownProfile(_) => "unknown_profile",
        ChoiceRefusal::UnknownHarness(_) => "unknown_harness",
        ChoiceRefusal::NotInstalled(_) => "harness_not_installed",
        ChoiceRefusal::Unreadable(_) => "profiles_unreadable",
    }
}
