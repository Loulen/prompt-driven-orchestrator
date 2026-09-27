//! The node session a request claims to come from (ADR-0064), and its check.

use std::sync::Arc;

use anyhow::Result;
use axum::http::{HeaderMap, StatusCode};

use crate::application::events::reload_run_state_with;
use crate::domain::state;
use crate::Runs;
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

/// The session identity a `POST /runs` caller carries via the
/// `X-PDO-Session-Run-Id` / `X-PDO-Session-Node-Id` headers — the exact values
/// of the `PDO_RUN_ID` / `PDO_NODE_ID` env vars every node session is wrapped
/// with (`pdo_session::domain::launch::wrap_with_env`). Never a body field: provenance
/// is mechanical (ADR-0064), so the daemon — not the caller — decides the
/// parent, and any parent key in the body is refused as unknown.
#[derive(Debug, Clone)]
pub struct SessionClaim {
    pub run_id: String,
    pub node_id: String,
}

/// Read the session claim from the request headers. Only a pair of non-empty
/// values is a claim — a lone header is not (the caller is out of session).
pub fn session_claim_from_headers(headers: &HeaderMap) -> Option<SessionClaim> {
    let run_id = headers
        .get(SESSION_RUN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let node_id = headers
        .get(SESSION_NODE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(SessionClaim {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
    })
}

/// Verify a session claim for mechanical provenance (ADR-0064) and hand back the
/// claimed parent Run's projected state — the child's project defaults to it.
///
/// The session is the only witness the provenance trusts, and the daemon knows
/// it already: a claim is accepted only when the named Run projects a node that
/// **currently holds a live session** ([`state::NodeStatus::holds_session`]).
/// A stale or invented claim is refused before any effect — no worktree, no
/// event. A claim on a node that is `Waiting` (admission-throttled) is refused
/// too: no session exists there to speak.
pub async fn verify_session_claim(
    state: &Arc<Runs>,
    claim: &SessionClaim,
) -> Result<state::RunState, (StatusCode, serde_json::Value)> {
    let refuse = |msg: String| (StatusCode::FORBIDDEN, serde_json::json!({ "error": msg }));
    let Some((_, parent)) = reload_run_state_with(state, &claim.run_id).await else {
        return Err(refuse(format!(
            "session claim refused: run `{}` does not exist (or has no run log)",
            claim.run_id
        )));
    };
    if !parent.status.is_live() {
        return Err(refuse(format!(
            "session claim refused: run `{}` is not live (status: {:?})",
            claim.run_id, parent.status
        )));
    }
    let Some(node) = parent.nodes.get(&claim.node_id) else {
        return Err(refuse(format!(
            "session claim refused: run `{}` has no node `{}`",
            claim.run_id, claim.node_id
        )));
    };
    if !node.status.holds_session() {
        return Err(refuse(format!(
            "session claim refused: node `{}` of run `{}` holds no live session \
             (status: {:?}); provenance follows a live node session",
            claim.node_id, claim.run_id, node.status
        )));
    }
    Ok(parent)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::adapters::cli::session_env_claim;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    #[test]
    fn run_create_session_env_claim_needs_a_pair() {
        // Mirror of the daemon's `session_claim_from_headers`: only a non-empty
        // PAIR is a claim. Gated on the vars being unset, so this can't race a
        // real session's env (same guard pattern as the nested-context tests).
        if std::env::var("PDO_RUN_ID").is_ok() || std::env::var("PDO_NODE_ID").is_ok() {
            return;
        }
        assert!(
            session_env_claim().is_none(),
            "no env vars ⇒ out of session"
        );
    }
}
