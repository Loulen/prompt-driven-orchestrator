//! `POST /ext-cli/<id>`: the daemon's side of `pdo ext <id> <args…>` (#33,
//! ADR-0081). The CLI of the core sends the arguments, the standard input and
//! its working directory; the daemon hands them to the extension's `run_cli`
//! with the Actor `identify` established (the agent of the node session the
//! call comes from, the local operator otherwise), and answers what the
//! extension wrote and the code it exits with.
//!
//! Outside the `/ext/<id>` prefix on purpose: everything under it is the
//! extension's own.
//!
//! - `200` `{"code", "stdout", "stderr"}`: the extension answered, whatever
//!   its code;
//! - `404` `{"error": "extension_not_found" | "cli_not_found", "message", "id"}`:
//!   no extension `<id>`, or one without a command line (`pdo ext` exits
//!   [`EXIT_NO_CLI`](pdo_extension_api::EXIT_NO_CLI));
//! - `400` `{"error": "bad_request"}`: a standard input that is not base64;
//! - `500` `{"error": "extension_failed"}`: the extension panicked.
//!
//! The bytes travel in base64 (standard alphabet, padded): standard input and
//! output are bytes, not text.

use std::panic::{catch_unwind, AssertUnwindSafe};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pdo_extension_api::{CliInvocation, CliSpec};
use pdo_kernel::extension_host::Extensions;
use pdo_kernel::Actor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One `pdo ext <id> <args…>`, as the CLI sends it.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionCliRequest {
    /// The arguments after the id.
    #[serde(default)]
    pub args: Vec<String>,
    /// The standard input, base64; empty when the CLI read none.
    #[serde(default)]
    pub stdin: String,
    /// The caller's working directory.
    #[serde(default)]
    #[ts(optional)]
    pub cwd: Option<String>,
}

/// The extension's answer: its exit code, its standard output and error in
/// base64.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionCliResponse {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// An extension's command line, as `GET /ext` lists it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ExtensionCli {
    /// The one line `pdo ext` shows next to the extension's id.
    pub summary: String,
}

/// What the extension `id` declares about its command line; `None` when it
/// has none, or panics saying so.
pub(crate) fn spec(id: &str, extension: &dyn pdo_extension_api::Extension) -> Option<CliSpec> {
    catch_unwind(AssertUnwindSafe(|| extension.cli())).unwrap_or_else(|_| {
        tracing::warn!("extension `{id}` panicked in cli");
        None
    })
}

/// `POST /ext-cli/<id>`.
pub(crate) async fn run(
    State(extensions): State<Extensions>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    Json(request): Json<ExtensionCliRequest>,
) -> Response {
    let Some(extension) = extensions
        .iter()
        .find(|(manifest, _)| manifest.id == id)
        .map(|(_, extension)| extension.clone())
    else {
        let message = format!("no extension `{id}` in this daemon");
        return error(StatusCode::NOT_FOUND, "extension_not_found", message, &id);
    };
    if spec(&id, extension.as_ref()).is_none() {
        let message = format!("extension `{id}` has no command line");
        return error(StatusCode::NOT_FOUND, "cli_not_found", message, &id);
    }
    let stdin = match BASE64.decode(request.stdin.as_bytes()) {
        Ok(stdin) => stdin,
        Err(e) => {
            let message = format!("the standard input is not base64: {e}");
            return error(StatusCode::BAD_REQUEST, "bad_request", message, &id);
        }
    };
    let mut invocation = CliInvocation::new(request.args, stdin, actor);
    if let Some(cwd) = request.cwd {
        invocation = invocation.with_cwd(cwd);
    }
    let answer = tokio::task::spawn_blocking(move || {
        catch_unwind(AssertUnwindSafe(|| extension.run_cli(&invocation)))
    })
    .await;
    match answer {
        Ok(Ok(output)) => Json(ExtensionCliResponse {
            code: output.code,
            stdout: BASE64.encode(&output.stdout),
            stderr: BASE64.encode(&output.stderr),
        })
        .into_response(),
        _ => {
            tracing::error!("extension `{id}` panicked in run_cli");
            let message = format!("extension `{id}` failed to run its command line");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "extension_failed",
                message,
                &id,
            )
        }
    }
}

fn error(status: StatusCode, slug: &str, message: String, id: &str) -> Response {
    let body = serde_json::json!({ "error": slug, "message": message, "id": id });
    (status, Json(body)).into_response()
}
