//! `GET /runs/{run_id}`, `GET /runs/{run_id}/events` and the search over the
//! Run's transcripts, `GET /runs/{run_id}/transcripts/search`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use pdo_kernel::attributes;
use pdo_kernel::ResourceKind;
use serde::Deserialize;

use crate::domain::event::{Event, EventKind};
use crate::Runs;

pub(super) async fn detail(State(runs): State<Arc<Runs>>, Path(run_id): Path<String>) -> Response {
    match runs.read_run().detail(&run_id).await {
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "run not found").into_response(),
        Ok(Some(detail)) => {
            let mut response = serde_json::to_value(detail.state).expect("RunState serializes");
            inject_frozen_node_provisioning(&mut response, &detail.events);
            response["has_manager"] = serde_json::Value::Bool(detail.has_manager);
            let carried =
                attributes::of_entity(runs.extensions(), ResourceKind::Run, &run_id).await;
            if !carried.is_empty() {
                response["attributes"] = serde_json::json!(carried);
            }
            Json(response).into_response()
        }
    }
}

/// The query of a search over the Run's transcripts: the text, and the search
/// box asking (its next search cancels this one).
#[derive(Deserialize)]
pub(super) struct TranscriptSearchQuery {
    #[serde(default)]
    q: String,
    client: Option<String>,
}

pub(super) async fn transcript_search(
    State(runs): State<Arc<Runs>>,
    Path(run_id): Path<String>,
    Query(query): Query<TranscriptSearchQuery>,
) -> Response {
    crate::application::transcript_search::search(&runs, &run_id, &query.q, query.client.as_deref())
        .await
}

pub(super) async fn events(State(runs): State<Arc<Runs>>, Path(run_id): Path<String>) -> Response {
    match runs.read_run().events(&run_id).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("error: {e}")).into_response(),
    }
}

/// Copy onto each node of a Run's detail the provisioning its current
/// iteration froze at spawn (`NodeStarted`): the rules, the plan, and when.
fn inject_frozen_node_provisioning(response: &mut serde_json::Value, events: &[Event]) {
    let Some(nodes) = response
        .get_mut("nodes")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for event in events {
        if event.kind != EventKind::NodeStarted {
            continue;
        }
        let (Some(node_id), Some(iter), Some(rules)) = (
            event.node_id.as_deref(),
            event.iter,
            event
                .payload
                .as_ref()
                .and_then(|payload| payload.get("provisioning")),
        ) else {
            continue;
        };
        let Some(node) = nodes.get_mut(node_id) else {
            continue;
        };
        if node.get("iter").and_then(serde_json::Value::as_i64) == Some(iter) {
            node["provisioning"] = rules.clone();
            if let Some(plan) = event
                .payload
                .as_ref()
                .and_then(|payload| payload.get("provisioning_plan"))
            {
                node["provisioning_plan"] = plan.clone();
            }
            if node.get("provisioning_frozen_at").is_none() {
                node["provisioning_frozen_at"] = serde_json::Value::String(event.ts.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_response_uses_the_current_iterations_frozen_node_provisioning() {
        let mut response = serde_json::json!({
            "nodes": {
                "worker": { "iter": 2 }
            }
        });
        let events = vec![
            Event {
                id: None,
                run_id: "run".into(),
                ts: "2026-09-01T12:00:00Z".into(),
                kind: EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(1),
                payload: Some(serde_json::json!({
                    "provisioning": { "copy": ["old"], "hardlink": [], "symlink": [] }
                })),
            },
            Event {
                id: None,
                run_id: "run".into(),
                ts: "2026-09-01T12:01:00Z".into(),
                kind: EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(2),
                payload: Some(serde_json::json!({
                    "provisioning": { "copy": ["frozen"], "hardlink": [], "symlink": [] },
                    "provisioning_plan": {
                        "entries": [{
                            "relative_path": "frozen",
                            "mode": "copy",
                            "origin_scope": "isolated_node",
                            "pattern": "frozen",
                            "provided_by_git": false
                        }],
                        "rules": [],
                        "conflicts": []
                    }
                })),
            },
            Event {
                id: None,
                run_id: "run".into(),
                ts: "2026-09-01T12:02:00Z".into(),
                kind: EventKind::NodeStarted,
                node_id: Some("worker".into()),
                iter: Some(2),
                payload: Some(serde_json::json!({
                    "provisioning": { "copy": ["frozen"], "hardlink": [], "symlink": [] }
                })),
            },
        ];

        inject_frozen_node_provisioning(&mut response, &events);

        assert_eq!(
            response["nodes"]["worker"]["provisioning"]["copy"],
            serde_json::json!(["frozen"])
        );
        assert_eq!(
            response["nodes"]["worker"]["provisioning_frozen_at"],
            serde_json::json!("2026-09-01T12:01:00Z")
        );
        assert_eq!(
            response["nodes"]["worker"]["provisioning_plan"]["entries"][0]["relative_path"],
            serde_json::json!("frozen")
        );
    }
}
