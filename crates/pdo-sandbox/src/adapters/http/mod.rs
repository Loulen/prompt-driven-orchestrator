//! The HTTP routes of the sandbox context: the staging-profile editor
//! (`/settings/sandbox-profiles*`), byte for byte as before the move.
//!
//! The Actor of every request is the one the `identify` middleware set; the use
//! cases ask the Autorisation d'opération with it before any write.

mod views;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use pdo_kernel::Actor;
use serde::Deserialize;
use tracing::{error, warn};

use crate::application::profiles::{ProfileError, SandboxProfiles};
use crate::domain::image::ProfileImage;
use crate::domain::profile::draft::ProfileDraft;

use views::profile_view;

/// The routes of the context, served on `profiles`.
pub fn router(profiles: Arc<SandboxProfiles>) -> Router {
    Router::new()
        .route("/settings/sandbox-profiles", get(list_sandbox_profiles))
        .route(
            "/settings/sandbox-profiles/{name}",
            get(get_sandbox_profile)
                .put(put_sandbox_profile)
                .delete(delete_sandbox_profile),
        )
        .route(
            "/settings/sandbox-profiles/{name}/referents",
            get(get_sandbox_profile_referents),
        )
        .with_state(profiles)
}

/// `{"error": message}` with `status`.
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

fn profile_error(e: ProfileError) -> Response {
    match e {
        ProfileError::Invalid(msg) => error(StatusCode::BAD_REQUEST, msg),
        ProfileError::NotFound(msg) => error(StatusCode::NOT_FOUND, msg),
        // An Operation an extension refused: 403, the reason in the `error` the
        // gesture already shows.
        ProfileError::Denied(denied) => error(StatusCode::FORBIDDEN, denied.to_string()),
        ProfileError::Store(msg) => error(StatusCode::INTERNAL_SERVER_ERROR, msg),
    }
}

/// `GET /settings/sandbox-profiles` — every profile the instance can serve: the two
/// virtual defaults ∪ the materialised rows, each fully resolved.
async fn list_sandbox_profiles(State(profiles): State<Arc<SandboxProfiles>>) -> Response {
    let home_root = profiles.home();
    let names = match profiles.names().await {
        Ok(n) => n,
        Err(e) => {
            error!("failed to list sandbox profiles: {e}");
            return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    };
    let mut out = Vec::with_capacity(names.len());
    for (name, _) in &names {
        match profiles.resolve(name).await {
            Ok(Some(p)) => out.push(profile_view(home_root.as_deref(), &p)),
            Ok(None) => warn!("sandbox profile `{name}` vanished mid-list"),
            Err(e) => {
                error!("failed to resolve sandbox profile `{name}`: {e}");
                return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
            }
        }
    }
    Json(serde_json::json!({
        "profiles": out,
        "home": home_root.map(|h| h.to_string_lossy().into_owned()),
    }))
    .into_response()
}

/// `GET /settings/sandbox-profiles/{name}` — one resolved profile, or 404.
async fn get_sandbox_profile(
    State(profiles): State<Arc<SandboxProfiles>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let home_root = profiles.home();
    match profiles.resolve(&name).await {
        Ok(Some(p)) => Json(profile_view(home_root.as_deref(), &p)).into_response(),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            format!("no staging profile named `{name}`"),
        ),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// The body of `PUT /settings/sandbox-profiles/{name}` — the **diff**, never a snapshot
/// (ADR-0031 §2). Every field defaults to empty so a bare `{}` means "materialise this
/// profile exactly as the current default, with no environment".
#[derive(Deserialize)]
struct UpsertSandboxProfileRequest {
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default)]
    extras: Vec<String>,
    /// Environment variables posed at `docker create` (ADR-0031 §8). A FULL
    /// replacement like the two lists above — an absent key CLEARS the env rather than
    /// leaving it alone, which is what makes "remove a variable" expressible at all.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Where this profile's image comes from (ADR-0031 §9), or absent/`null` to pose
    /// nothing and let the profile default decide. A FULL replacement like every other
    /// field: omitting it CLEARS the image source.
    ///
    /// Typed rather than a `serde_json::Value`, so an unknown `kind` is rejected by
    /// serde itself — naming the offending token and the two it expects — before this
    /// handler runs at all.
    #[serde(default)]
    image: Option<ProfileImage>,
}

/// `PUT /settings/sandbox-profiles/{name}` — upsert the diff (see
/// [`SandboxProfiles::upsert`] for every refusal).
async fn put_sandbox_profile(
    State(profiles): State<Arc<SandboxProfiles>>,
    AxumPath(name): AxumPath<String>,
    Extension(actor): Extension<Actor>,
    Json(req): Json<UpsertSandboxProfileRequest>,
) -> Response {
    let draft = ProfileDraft {
        disabled: req.disabled,
        extras: req.extras,
        env: req.env,
        image: req.image,
    };
    match profiles.upsert(&actor, &name, &draft).await {
        Ok(p) => Json(profile_view(profiles.home().as_deref(), &p)).into_response(),
        Err(e) => profile_error(e),
    }
}

/// `DELETE /settings/sandbox-profiles/{name}` — drop the materialised row
/// (see [`SandboxProfiles::delete`]).
async fn delete_sandbox_profile(
    State(profiles): State<Arc<SandboxProfiles>>,
    AxumPath(name): AxumPath<String>,
    Extension(actor): Extension<Actor>,
) -> Response {
    match profiles.delete(&actor, &name).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => profile_error(e),
    }
}

/// `GET /settings/sandbox-profiles/{name}/referents` — who still points at this profile.
///
/// Server-side because the frontend **cannot** derive the third class: `RunListEntry`
/// does not carry `sandbox` (only the full `RunState` does), so a client would need N
/// requests. The distinction between the first two classes and the third is the whole
/// point of the dialog:
/// - `instance_default` / `triggers` — deleting will NOT repoint them, and the next Run
///   they produce **fails**; it does not fall back to a default;
/// - `runs` — live Runs that already froze their entry list at start (ADR-0031 §6) and
///   are therefore **unaffected**.
async fn get_sandbox_profile_referents(
    State(profiles): State<Arc<SandboxProfiles>>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let referents = profiles.referents(&name).await;
    let triggers: Vec<serde_json::Value> = referents
        .triggers
        .iter()
        .map(|t| serde_json::json!({ "id": t.id, "name": t.name, "enabled": t.enabled }))
        .collect();
    let runs: Vec<serde_json::Value> = referents
        .runs
        .iter()
        .map(|r| {
            serde_json::json!({
                "run_id": r.run_id,
                "pipeline_name": r.pipeline_name,
                "name": r.name,
            })
        })
        .collect();
    Json(serde_json::json!({
        "name": name,
        "instance_default": referents.instance_default,
        "triggers": triggers,
        "runs": runs,
    }))
    .into_response()
}

/// Stage 4 lives in `crates/pdo/tests/sandbox_profiles.rs`, against a real daemon;
/// these cases pin the adapter's own translation (status codes, the Actor from the
/// middleware) on in-memory ports.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::profiles::fixture::{refusing, with_home};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn app(profiles: SandboxProfiles) -> Router {
        router(Arc::new(profiles)).layer(Extension(Actor::LocalOperator))
    }

    async fn send(app: Router, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_put_then_a_get_serve_the_same_view() {
        let f = with_home(&[]);
        let app = app(f.profiles);
        let (status, put) = send(app.clone(), "PUT", "/settings/sandbox-profiles/team", "{}").await;
        assert_eq!(status, StatusCode::OK, "{put}");
        let (status, got) = send(app, "GET", "/settings/sandbox-profiles/team", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(put, got);
    }

    #[tokio::test]
    async fn the_statuses_follow_the_use_case_outcomes() {
        let f = with_home(&[]);
        let app = app(f.profiles);
        let (status, body) = send(app.clone(), "PUT", "/settings/sandbox-profiles/OFF", "{}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let (status, _) = send(app.clone(), "DELETE", "/settings/sandbox-profiles/full", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(app, "GET", "/settings/sandbox-profiles/nope", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_refused_operation_is_a_403_carrying_the_reason() {
        let (f, _) = refusing();
        let (status, body) = send(
            app(f.profiles),
            "PUT",
            "/settings/sandbox-profiles/team",
            "{}",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body.contains("admins only"), "{body}");
    }

    #[tokio::test]
    async fn the_referents_keep_their_wire_shape() {
        let f = with_home(&[]);
        let (status, body) = send(
            app(f.profiles),
            "GET",
            "/settings/sandbox-profiles/team/referents",
            "",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({
                "name": "team",
                "instance_default": false,
                "triggers": [],
                "runs": [],
            })
        );
    }
}
