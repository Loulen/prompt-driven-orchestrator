//! `POST /settings/harnesses/{name}/refresh`: the « Refresh models » gesture.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;

use super::refusal;
use crate::Settings;

/// `200` with the harness's entry of `harness_descriptors.harnesses`, probed
/// now; `404` for a harness the registry does not resolve; the kernel's `403`
/// when an extension refuses it.
pub(super) async fn refresh(
    State(settings): State<Arc<Settings>>,
    Extension(actor): Extension<Actor>,
    Path(name): Path<String>,
) -> Response {
    match settings.refresh_harness_catalogue(&actor, &name).await {
        Ok(entry) => Json(entry).into_response(),
        Err(e) => refusal(e),
    }
}
