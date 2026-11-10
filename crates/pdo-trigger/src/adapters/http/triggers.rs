//! `/triggers` and `/triggers/{id}`: list, create, read, edit, delete, and a
//! Trigger's fires.

use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::attributes::AttributeFilter;
use pdo_kernel::Actor;
use pdo_platform::adapters::http::OriginHint;

use super::refusal;
use crate::application::create_trigger::CreateTriggerRequest;
use crate::application::update_trigger::PatchTriggerRequest;
use crate::Triggers;

/// `GET /triggers[?attr.<id>/<key>=<value>…]`.
pub(super) async fn list(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    RawQuery(query): RawQuery,
) -> Response {
    match triggers
        .list(&actor, AttributeFilter::from_query(query.as_deref()))
        .await
    {
        Ok(entries) => Json(entries).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn create(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
    Json(req): Json<CreateTriggerRequest>,
) -> Response {
    match triggers.create(&actor, hint, req).await {
        Ok(trigger) => (StatusCode::CREATED, Json(trigger)).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn get(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    Path(trigger_id): Path<String>,
) -> Response {
    match triggers.attributed(&actor, &trigger_id).await {
        Ok(trigger) => Json(trigger).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn update(
    State(triggers): State<Arc<Triggers>>,
    Path(trigger_id): Path<String>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
    Json(req): Json<PatchTriggerRequest>,
) -> Response {
    match triggers.update(&actor, hint, &trigger_id, req).await {
        Ok(trigger) => Json(trigger).into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn delete(
    State(triggers): State<Arc<Triggers>>,
    Path(trigger_id): Path<String>,
    Extension(actor): Extension<Actor>,
    OriginHint(hint): OriginHint,
) -> Response {
    match triggers.delete(&actor, hint, &trigger_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refusal(e),
    }
}

pub(super) async fn fires(
    State(triggers): State<Arc<Triggers>>,
    Extension(actor): Extension<Actor>,
    Path(trigger_id): Path<String>,
) -> Response {
    match triggers.fires(&actor, &trigger_id).await {
        Ok(fires) => Json(fires).into_response(),
        Err(e) => refusal(e),
    }
}
