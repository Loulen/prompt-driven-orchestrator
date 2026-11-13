//! `retry_all` (a Pipeline Manager command): archive a terminal Run, then
//! start its twin. A transition the command triggers: the command itself asked
//! the Autorisation d'opération.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::info;

use crate::application::archive::cleanup_run;
use crate::application::create_run::freeze::run_tier;
use crate::application::create_run::request::{CreateRunRequest, TargetRepoInput};
use crate::application::events::load_projected;
use crate::application::paths::effective_repo_root;
use crate::domain::event::EventKind;
use crate::domain::state::RunStatus;
use crate::Runs;

/// `retry_all` (a Pipeline Manager command): archive a terminal Run, then start
/// its twin — same pipeline, input, variables, repositories, isolation,
/// harness, skills and preferences. The command surface forwards this answer
/// verbatim.
pub async fn retry_all_run(state: &Arc<Runs>, run_id: &str) -> Response {
    let run_id = run_id.to_string();
    let (events, run_state) = match load_projected(state, &run_id).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    // Deliberately NOT `RunStatus::is_terminal()`: this set omits `Archived`,
    // whose retry-ability is still an open question.
    let is_terminal = matches!(
        run_state.status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Skipped | RunStatus::Halted
    );
    if !is_terminal {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("retry_all requires a terminal state, run is {:?}", run_state.status)
            })),
        )
            .into_response();
    }

    let run_started_event = events.iter().find(|e| e.kind == EventKind::RunStarted);
    let run_started_payload = run_started_event.and_then(|e| e.payload.as_ref());

    let pipeline_name = run_state.pipeline_name.clone();
    let input = run_state.input.clone().unwrap_or_default();
    let variables: HashMap<String, serde_yaml::Value> = run_started_payload
        .and_then(|p| p.get("variables"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    // Read the RESOLVED repo, not the raw field: an old run carries
    // `target_repo: null`, and forwarding that raw would 400 at the chokepoint
    // AFTER `cleanup_run` archived the original — archived with no replacement.
    // API 0.9: the twin carries the original's extension data, as frozen (its
    // extensions accepted it at the create; they stamp the twin with it).
    let extensions: pdo_kernel::attributes::ExtensionData = run_started_payload
        .and_then(|p| p.get("extensions"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let target_repo = Some(
        effective_repo_root(state, &run_state)
            .to_string_lossy()
            .into_owned(),
    );
    let source_branch = run_state.source_branch.clone();
    // Preserve the original Run's read-only secondaries so the retry runs in
    // the same multi-repo context. The primary must sit at [0]; the create
    // chokepoint re-freezes each secondary's SHA against its recorded base
    // branch.
    let target_repos: Vec<TargetRepoInput> = if run_state.target_repos.is_empty() {
        Vec::new()
    } else {
        let mut v = vec![TargetRepoInput {
            repo: target_repo.clone().unwrap_or_default(),
            base_branch: source_branch.clone(),
            // The primary is always writable (ADR-0047).
            read_only: false,
        }];
        v.extend(run_state.target_repos.iter().map(|p| TargetRepoInput {
            repo: p.repo.clone(),
            base_branch: p.base_branch.clone(),
            // Preserve the read-only opt-in across a retry (ADR-0047).
            read_only: p.read_only,
        }));
        v
    };

    let archive_resp = cleanup_run(state, &run_id).await;
    let archive_status = archive_resp.into_response().status();
    if !archive_status.is_success() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "failed to archive original run" })),
        )
            .into_response();
    }

    let new_run_req = CreateRunRequest {
        pipeline: pipeline_name,
        input,
        variables,
        // Preserve the library pipeline id so the retried run stays in the
        // same "by pipeline" stats bucket rather than falling back to the name.
        pipeline_id: run_state.pipeline_id.clone(),
        target_repo,
        // A retry stays in the Projet the original was launched in (ADR-0087).
        project_id: run_state.project_id.clone(),
        target_repos,
        source_branch,
        // The twin keeps the original's name; an unnamed original is named
        // afresh (`auto_name` below).
        name: run_state.name.clone(),
        triggered_by: None,
        // A retry preserves the original Run's isolation mode. Wrapped in
        // `Some` so the chokepoint treats it as EXPLICIT: a changed instance
        // default must never silently re-sandbox (or un-sandbox) a retried Run.
        //
        // `run_state.sandbox_entries` is deliberately NOT forwarded. A retry is
        // a NEW Run with nothing staged, so there is no coherence to protect,
        // and re-resolving is what ADR-0031 §2 requires (a profile edited since
        // must take effect). Do not "fix" this by threading the frozen list: a
        // profile deleted since makes the retry 400 loudly, which is intended.
        sandbox: Some(run_state.sandbox.clone()),
        // A retry must reproduce the original's harness, or an A/B comparison
        // silently reverts to the instance default. `None` (the Run named no
        // harness) forwards as `None`. Unlike `sandbox` this needs no
        // `Some`-wrapping: the create chokepoint freezes `req.harness` verbatim.
        harness: run_state.harness.clone(),
        // #669: a retry reproduces the original Run tier's skills, same
        // reason as `harness` — the union must not silently shrink.
        skills: run_state.skills.clone(),
        // Reproduce the original Run's `AgentChoice`, like `harness`: `None`
        // forwards as `None`, so the retry re-resolves through the legacy
        // `harness` tier exactly as the original did.
        agent_choice: run_state.agent_choice.clone(),
        // An unnamed original's twin re-derives its name. `Some(true)`
        // reproduces that regardless of the instance default, so a changed
        // `default_auto_name` cannot silently alter how a retried Run is named;
        // a named one keeps its name and derives none.
        auto_name: Some(run_state.name.is_none()),
        // Reproduce the original Run's `auto_fail`, like `harness`: `None`
        // forwards as `None` and re-resolves through project / instance.
        auto_fail: run_state.auto_fail,
        // Preserve the explicit Run tier. Instance and Project are resolved
        // afresh for this new Run, while its per-Run override remains stable.
        provisioning: run_tier(&run_state.provisioning_rules),
        extensions,
        // NOT carried over from the original (#804): the reason a fetch
        // failed minutes or days ago says nothing about this new cut, and
        // a retry is a manual gesture — it fetches no more than any other
        // `POST /runs` does.
        source_fetch_error: None,
        // The twin answers to whoever created the original (API 0.8): the
        // command carries no Actor of its own.
        created_by: run_state.created_by.clone(),
        trigger_editor: run_state.trigger_editor.clone(),
        // The twin comes from where the original came from: a retry does not
        // make an extension's Run look launched by hand.
        provenance: run_state.provenance.clone(),
    };
    // ADR-0064: a retried Run is created by the daemon, not from a node
    // session — it is a root even when the archived original had a parent.
    let new_run_resp = state
        .create_run_response(new_run_req, Vec::new(), None)
        .await;

    info!("retry_all: archived run {run_id}, created new run");
    new_run_resp
}
