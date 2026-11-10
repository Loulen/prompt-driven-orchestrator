//! The output validation of a completion, with its frontmatter retries.

use tracing::{info, warn};

use crate::application::events::append_event;
use crate::application::node::outputs as outputs_validator;
use crate::domain::completion_refusal;
use crate::domain::{event, state};
use crate::Runs;
use pdo_pipeline::domain::pipeline;

/// The **shared chokepoint** of the completion path (ADR-0035): the one function
/// both `POST …/nodes/:id/done` and the `mark_node_done` command arm reach. It owns
/// the side effects and returns a **typed refusal**, never a pre-projected
/// `Response` — projection belongs to `completion_refusal::refusal_response` alone,
/// which is what makes "a refusal is never a 2xx" a property of the type rather than
/// a list of arms to re-read.
///
/// `Option<CompletionRefusal>` and not `Result<_, Response>`: `Response` is 128
/// bytes, exactly the `clippy::result_large_err` threshold CI treats as an error.
pub async fn check_output_validation_with_retry(
    state: &Runs,
    pipeline_path: &std::path::Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &std::path::Path,
    run_id: &str,
    run_state: &state::RunState,
) -> Option<completion_refusal::CompletionRefusal> {
    let yaml = std::fs::read_to_string(pipeline_path).ok()?;
    let parse_result = pipeline::parse_pipeline(&yaml).ok()?;
    let Err(validation_error) =
        outputs_validator::validate(&parse_result.pipeline, node_id, iter, artifacts_dir)
    else {
        return None;
    };

    // A `script` node has already exited by the time this runs, so there is no live
    // agent to nudge: any validation failure is terminal. The interactive corrective
    // loop would `send-keys` into a dead session and strand the node behind a 409.
    let is_script = parse_result
        .pipeline
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.node_type == pipeline::NodeType::Script)
        .unwrap_or(false);
    if is_script {
        let detail = match &validation_error {
            outputs_validator::ValidationError::MissingOutputs(missing) => {
                serde_json::json!({ "kind": "missing_outputs", "missing": missing })
            }
            outputs_validator::ValidationError::FrontmatterMismatch(violations) => {
                let details: Vec<serde_json::Value> = violations
                    .iter()
                    .map(|v| serde_json::json!({ "port": v.port, "field": v.field, "reason": v.reason }))
                    .collect();
                serde_json::json!({ "kind": "frontmatter_mismatch", "violations": details })
            }
        };
        // An output-validation miss is a runtime give-up, not a business failure:
        // `Interrupted`, never `Failed` (ADR-0049).
        let interrupt_event = event::Event {
            id: None,
            run_id: run_id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            kind: event::EventKind::NodeInterrupted,
            node_id: Some(node_id.to_string()),
            iter: Some(iter),
            payload: Some(serde_json::json!({
                "reason": format!(
                    "script_validation_failed: script node {node_id} failed output \
                     validation at iter {iter}"
                ),
                "reason_code": "script_validation_failed",
                "detail": detail,
            })),
        };
        let _ = append_event(state, &interrupt_event).await;

        warn!("script node {node_id} failed output validation in run {run_id} — interrupted");
        return Some(completion_refusal::CompletionRefusal::ScriptValidationFailed { detail });
    }

    match validation_error {
        outputs_validator::ValidationError::MissingOutputs(missing) => {
            Some(completion_refusal::CompletionRefusal::MissingOutputs { missing })
        }
        outputs_validator::ValidationError::FrontmatterMismatch(violations) => {
            let retries = run_state
                .nodes
                .get(node_id)
                .map(|n| n.frontmatter_retries)
                .unwrap_or(0);

            let violation_details: Vec<serde_json::Value> = violations
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "port": v.port,
                        "field": v.field,
                        "reason": v.reason,
                    })
                })
                .collect();

            if retries >= 1 {
                // The corrective loop is exhausted, but a validation miss is still a
                // runtime give-up: `Interrupted`, never `Failed` (ADR-0049).
                let interrupt_event = event::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event::EventKind::NodeInterrupted,
                    node_id: Some(node_id.to_string()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "reason": format!(
                            "frontmatter_retry_exhausted: node {node_id} failed output \
                             validation after retry at iter {iter}"
                        ),
                        "reason_code": "frontmatter_retry_exhausted",
                        "violations": violation_details,
                    })),
                };
                let _ = append_event(state, &interrupt_event).await;

                warn!("Node {node_id} failed output validation after retry in run {run_id}");
                Some(
                    completion_refusal::CompletionRefusal::FrontmatterRetryExhausted {
                        violations: violation_details,
                    },
                )
            } else {
                let msg = outputs_validator::corrective_message(&violations);
                let session_name =
                    pdo_session::domain::names::node_session_name(run_id, node_id, iter);
                pdo_session::application::messages::send_message(state.tmux(), &session_name, &msg);

                let retry_event = event::Event {
                    id: None,
                    run_id: run_id.to_string(),
                    ts: pdo_kernel::clock::now_iso(),
                    kind: event::EventKind::FrontmatterRetryPending,
                    node_id: Some(node_id.to_string()),
                    iter: Some(iter),
                    payload: Some(serde_json::json!({
                        "violations": violation_details,
                    })),
                };
                let _ = append_event(state, &retry_event).await;

                info!("Frontmatter mismatch for node {node_id} in run {run_id} — corrective message sent, awaiting retry");
                Some(
                    completion_refusal::CompletionRefusal::FrontmatterRetryPending {
                        violations: violation_details,
                    },
                )
            }
        }
    }
}
