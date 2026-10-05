//! `PATCH /runs/{run_id}/repos`: add or remove secondary repositories on a
//! live Run (ADR-0042). A gesture of an actor: it asks the Autorisation
//! d'opération (`run` × `edit_repos`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::Actor;
use pdo_workspace::adapters::git::worktrees::{create_secondary_snapshot, secondary_snapshot_path};
use tracing::error;

use crate::adapters::http::verdicts::repo_edit::{repo_edit_refusal_response, RepoEditRefusal};
use crate::application::create_run::repos::{
    canonical_or_self, existing_snapshot_dir_names, resolve_one_secondary_pin,
};
use crate::application::create_run::request::PatchRunReposRequest;
use crate::application::events::{append_event, reload_run_state, run_is_forgotten};
use crate::application::operations::on_run;
use crate::application::paths::effective_repo_root;
use crate::domain::event::{Event, EventKind};
use crate::domain::state::RepoPin;
use crate::Runs;

impl Runs {
    /// `PATCH /runs/{run_id}/repos` — add / remove secondary repos on a **live** Run
    /// (ADR-0042).
    ///
    /// `RunState.target_repos` is frozen at `RunStarted` and now also moves on
    /// `RunReposEdited`. The contract is **spawn-time visibility**: an edit affects nodes
    /// launched AFTER it, and already-live nodes keep the context frozen at their spawn —
    /// preamble and `PDO_SECONDARY_REPOS` are written once at launch and never re-read.
    ///
    /// Order mirrors `run_command`: forgotten (410) → project (404) → terminal (409) →
    /// validate removes+adds → materialise adds → append → reproject.
    pub async fn edit_repos(
        self: &Arc<Self>,
        actor: &Actor,
        run_id: &str,
        req: PatchRunReposRequest,
    ) -> Response {
        if let Err(denied) = self.authorize(actor, &on_run("edit_repos", run_id)).await {
            return denied.into_response();
        }
        let state = self;
        let run_id = run_id.to_string();

        // (1) A forgotten Run is a tombstone — 410 before any side effect.
        match run_is_forgotten(state, &run_id).await {
            Ok(true) => {
                return repo_edit_refusal_response(&RepoEditRefusal::RunForgotten { run_id });
            }
            Ok(false) => {}
            Err(e) => {
                return repo_edit_refusal_response(&RepoEditRefusal::Internal {
                    error: format!("forgotten-run check failed: {e}"),
                });
            }
        }

        let Some((_, run_state)) = reload_run_state(state, &run_id).await else {
            return repo_edit_refusal_response(&RepoEditRefusal::RunNotFound);
        };

        // (3) A terminal Run's list is frozen for good.
        if run_state.status.is_terminal() {
            return repo_edit_refusal_response(&RepoEditRefusal::RunTerminal {
                status: run_state.status,
            });
        }

        // An empty edit is a no-op: return the current projection without touching the log.
        if req.add.is_empty() && req.remove.is_empty() {
            return (StatusCode::OK, Json(run_state)).into_response();
        }

        let repo_root = effective_repo_root(state, &run_state);
        let primary_canon = canonical_or_self(&repo_root);

        // (4) Removals first, by alias. An absent alias is a silent no-op: the UI removes
        //     by displayed alias, and a double-click must not 404.
        let remove_set: HashSet<&str> = req
            .remove
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let mut active: Vec<RepoPin> = run_state
            .target_repos
            .iter()
            .filter(|p| !remove_set.contains(p.alias.as_str()))
            .cloned()
            .collect();

        // (5) Seed the disambiguation sets from the SURVIVING pins AND from the folders
        //     present on disk, so a removed-but-persistent snapshot's alias is not reused.
        //     `seen_canon` is seeded ONLY from survivors, so a repo removed in this same
        //     request may be re-added.
        let mut seen_canon: HashSet<PathBuf> = active
            .iter()
            .map(|p| canonical_or_self(Path::new(&p.repo)))
            .collect();
        let mut used_aliases: HashSet<String> = active.iter().map(|p| p.alias.clone()).collect();
        for name in existing_snapshot_dir_names(&repo_root, &run_id) {
            used_aliases.insert(name);
        }

        // (6) Validate every add BEFORE writing anything to disk, so a bad repo yields no
        //     snapshot and no event.
        let mut new_pins: Vec<RepoPin> = Vec::new();
        for input in &req.add {
            match resolve_one_secondary_pin(
                input,
                &primary_canon,
                &mut seen_canon,
                &mut used_aliases,
            ) {
                Ok(pin) => new_pins.push(pin),
                Err(r) => return repo_edit_refusal_response(&r),
            }
        }

        // (7) Materialise each add BEFORE any append, so the log can never carry a pin
        //     whose snapshot is missing. A snapshot orphaned by a later add's failure is
        //     reaped by the disk-driven `cleanup_run` and invisible to the projection.
        for pin in &new_pins {
            let dest = secondary_snapshot_path(&repo_root, &run_id, &pin.alias);
            if let Err(e) = create_secondary_snapshot(Path::new(&pin.repo), &dest, &pin.sha) {
                error!("patch_run_repos: snapshot failed for {}: {e:#}", pin.repo);
                return repo_edit_refusal_response(&RepoEditRefusal::SnapshotMaterializeFailed {
                    repo: pin.repo.clone(),
                    error: format!("{e:#}"),
                });
            }
        }

        active.extend(new_pins);

        // Skip the append when nothing actually changed. Adds always change the list
        // (each gets a fresh alias), so this only short-circuits genuine no-ops.
        let before: Vec<&str> = run_state
            .target_repos
            .iter()
            .map(|p| p.alias.as_str())
            .collect();
        let after: Vec<&str> = active.iter().map(|p| p.alias.as_str()).collect();
        if before == after {
            return (StatusCode::OK, Json(run_state)).into_response();
        }

        let event = Event {
            id: None,
            run_id: run_id.clone(),
            ts: pdo_kernel::clock::now_iso(),
            kind: EventKind::RunReposEdited,
            node_id: None,
            iter: None,
            payload: Some(serde_json::json!({ "target_repos": active })),
        };
        if let Err(e) = append_event(state, &event).await {
            error!("patch_run_repos: append failed for run {run_id}: {e:#}");
            return repo_edit_refusal_response(&RepoEditRefusal::AppendFailed {
                error: format!("{e:#}"),
            });
        }

        match reload_run_state(state, &run_id).await {
            Some((_, refreshed)) => (StatusCode::OK, Json(refreshed)).into_response(),
            None => repo_edit_refusal_response(&RepoEditRefusal::Internal {
                error: "run vanished immediately after a repos edit".into(),
            }),
        }
    }
}
