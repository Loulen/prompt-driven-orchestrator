//! The question of authorization (CORE-5): `POST /authorize` asks whether the
//! caller may run each of a list of Operations, without running any. The UI of
//! the core uses it to hide what would be refused (the loupe of the New Run
//! form, the Projets a Run cannot be created in); an extension's UI may too.
//!
//! The body is `{"operations": [<Operation>, …]}`, each in its wire form
//! (`{"resource", "verb", "target"?, "projects"?}`); the answer
//! `{"decisions": [{"decision": "allow"} | {"decision": "deny", "reason"}, …]}`,
//! in the same order. An Operation that names no Projet is placed where the
//! core places its own: one on a Run or a node, in the Projet of the Run; one
//! on a Projet, in that Projet; a page mount asked from a node session, in the
//! Projet of its Run, and an unmount, in the Projet of the mount's Run.
//!
//! Nothing is recorded nor announced, nothing is logged: a question is not a
//! gesture. Without extension, everything is allowed.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Json, Router};
use pdo_extension_api::Authorization;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind};
use pdo_platform::Platform;
use pdo_run::Runs;
use serde::{Deserialize, Serialize};

/// How many Operations one question may hold.
pub const MAX_OPERATIONS: usize = 500;

#[derive(Clone)]
struct Asked {
    host: Arc<dyn ExtensionHost>,
    runs: Arc<Runs>,
    platform: Arc<Platform>,
}

#[derive(Deserialize)]
struct Question {
    operations: Vec<Operation>,
}

#[derive(Serialize)]
struct Answer {
    decisions: Vec<Authorization>,
}

/// `POST /authorize`, over the extension host `host`; the Projets of a Run
/// come from `runs`, those of a page mount from `platform`.
pub fn router(host: Arc<dyn ExtensionHost>, runs: Arc<Runs>, platform: Arc<Platform>) -> Router {
    Router::new()
        .route("/authorize", post(ask))
        .with_state(Asked {
            host,
            runs,
            platform,
        })
}

async fn ask(
    State(asked): State<Asked>,
    Extension(actor): Extension<Actor>,
    question: Result<Json<Question>, JsonRejection>,
) -> Response {
    let Json(question) = match question {
        Ok(question) => question,
        Err(rejection) => return bad_request(rejection.body_text()),
    };
    if question.operations.len() > MAX_OPERATIONS {
        return bad_request(format!(
            "at most {MAX_OPERATIONS} operations per question, got {}",
            question.operations.len()
        ));
    }
    let mut decisions = Vec::with_capacity(question.operations.len());
    for operation in question.operations {
        let decision = if asked.host.allows_everything() {
            Authorization::Allow
        } else if asked
            .platform
            .place_page_mount()
            .owned_by(&actor, &operation)
            .await
        {
            // The owner of a page unmounts it without the extensions.
            Authorization::Allow
        } else {
            let operation = placed(&asked, &actor, operation).await;
            asked.host.authorize(&actor, &operation)
        };
        decisions.push(decision);
    }
    Json(Answer { decisions }).into_response()
}

/// `operation`, asked by `actor`, in the Projets the core would put it in,
/// when it names none.
async fn placed(asked: &Asked, actor: &Actor, operation: Operation) -> Operation {
    if !operation.is_outside_projects() {
        return operation;
    }
    match (&operation.resource, operation.target.clone()) {
        (ResourceKind::Project, Some(id)) => operation.in_projects([id]),
        (ResourceKind::Run | ResourceKind::Node, Some(_)) => asked.runs.scoped(operation).await,
        (ResourceKind::PageMount, _) => {
            asked
                .platform
                .place_page_mount()
                .execute(actor, operation)
                .await
        }
        _ => operation,
    }
}

fn bad_request(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "invalid_question", "message": message })),
    )
        .into_response()
}
