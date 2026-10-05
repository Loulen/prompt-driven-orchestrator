//! Creating a Run (`POST /runs`, `pdo run create`): the one create path the
//! JSON, multipart, CLI, retry and Trigger-fire callers converge on.
//!
//! [`Runs::create_run`] is the gesture of an actor: it asks the Autorisation
//! d'opération (`run` × `create`) first. [`Runs::start_run`] is the creation
//! itself, for a caller that has already passed its own gate: a `retry_all`
//! command, a Trigger fire. In order: the session claim (ADR-0064), the
//! repositories ([`target`]), the pipeline, the provisioning and the isolation
//! ([`freeze`]), the `RunStarted` payload ([`payload`]), the Run on disk
//! ([`materialize`]), then its start ([`launch`]).

pub mod freeze;
mod launch;
mod materialize;
pub mod naming;
mod payload;
pub mod repos;
pub mod request;
pub mod target;

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::attributes::{self, Attributes, EntityRef};
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::{info, warn};

pub use materialize::reap_orphan_run_worktree;
pub use request::{
    reject_unknown_create_run_fields, CreateRunRequest, CreateRunResponse, InputFile,
    PatchRunReposRequest, TargetRepoInput, CREATE_RUN_FIELDS,
};

use crate::application::run_id::generate_run_id;
use crate::application::session_claim::{verify_session_claim, SessionClaim};
use crate::Runs;
use target::Refusal;

impl Runs {
    /// `POST /runs`: create a Run, as `actor`. `session` is the node session
    /// the request claims to come from (ADR-0064): verified, it makes the new
    /// Run a child of that node's Run.
    pub async fn create_run(
        self: &Arc<Self>,
        actor: &Actor,
        mut req: CreateRunRequest,
        attachments: Vec<InputFile>,
        session: Option<SessionClaim>,
    ) -> Response {
        let operation = Operation::new(ResourceKind::Run, Verb::Create)
            .in_projects(self.projects_of_new_run(&req, session.as_ref()).await);
        if let Err(denied) = self.authorize(actor, &operation).await {
            return denied.into_response();
        }
        req.created_by = Some(actor.clone());
        match self.start_run(req, attachments, session).await {
            Ok(run_id) => {
                self.stamp_run(actor, &run_id).await;
                (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response()
            }
            Err((status, body)) => (status, Json(body)).into_response(),
        }
    }

    /// Stamp the extensions' attributes on the Run `run_id`, which `actor` just
    /// created (a gesture's creation, or a Trigger's fire). Returns them.
    pub async fn stamp_run(&self, actor: &Actor, run_id: &str) -> Attributes {
        let entity = EntityRef::new(ResourceKind::Run, run_id);
        attributes::stamp(self.extensions(), actor, entity).await
    }

    /// [`Self::start_run`], answered as `POST /runs` answers it: `201` and the
    /// new id, or the refusal's status and body.
    pub(crate) async fn create_run_response(
        self: &Arc<Self>,
        req: CreateRunRequest,
        attachments: Vec<InputFile>,
        session: Option<SessionClaim>,
    ) -> Response {
        match self.start_run(req, attachments, session).await {
            Ok(run_id) => (StatusCode::CREATED, Json(CreateRunResponse { run_id })).into_response(),
            Err((status, body)) => (status, Json(body)).into_response(),
        }
    }

    /// Create and start a Run, returning its id or the `(status, body)` of the
    /// refusal. A caller that has passed its own gate calls this directly: the
    /// Trigger fire reads the id for its `triggered_by` provenance, with
    /// `session: None` — a fired Run is always a root (a Trigger is not a node
    /// session).
    pub async fn start_run(
        self: &Arc<Self>,
        req: CreateRunRequest,
        attachments: Vec<InputFile>,
        session: Option<SessionClaim>,
    ) -> Result<String, Refusal> {
        // ADR-0064: provenance is mechanical. Verify the session claim FIRST —
        // before any effect (worktree, snapshots, event) — and keep the parent's
        // projected state: the child's project defaults to it when the request
        // names none.
        let parent = match &session {
            Some(claim) => Some(verify_session_claim(self, claim).await?),
            None => None,
        };
        let target = target::resolve(self, &req, parent.as_ref())?;
        let launchable = target::load_pipeline(self, &req)?;
        let provisioning = freeze::provisioning(self, &req, &target).await?;

        let run_id = generate_run_id();
        // Freeze the fork point NOW, from the same local `source_ref`
        // `create_worktree` will cut the run branch from, or a later HEAD move in
        // the shared checkout displaces the LOC/diff base and sweeps in phantom
        // line counts. A resolve failure is non-fatal — it degrades to the
        // `source_branch`/`HEAD` fallback ladder at read time and must never
        // block run creation.
        let fork_sha = match pdo_workspace::adapters::git::worktrees::rev_parse_verified(
            &target.run_repo_root,
            &target.source_ref,
        ) {
            Ok(sha) => Some(sha),
            Err(e) => {
                warn!(
                    "could not resolve fork_sha for run {run_id} from {}: {e}",
                    target.source_ref
                );
                None
            }
        };

        // The instance settings, read FRESH at the create chokepoint so a
        // `PUT /settings` bites on the next create with no restart (ADR-0015).
        let defaults = self.settings.defaults().await.unwrap_or_default();
        let sandbox = freeze::sandbox(self, &req, defaults.default_sandbox.clone()).await?;
        let mut started = payload::run_started(
            self,
            payload::Frozen {
                req: &req,
                target: &target,
                launchable: &launchable,
                provisioning_rules: &provisioning.rules,
                sandbox: &sandbox,
                run_id: &run_id,
                fork_sha: fork_sha.as_deref(),
                defaults: &defaults,
                parent: parent.as_ref(),
                session: session.as_ref(),
                attachments: &attachments,
            },
        )
        .await;
        let worktree_dir = materialize::materialize(
            self,
            &run_id,
            &mut started,
            materialize::Materials {
                target: &target,
                launchable: &launchable,
                provisioning_plan: &provisioning.plan,
                input: &req.input,
                attachments: &attachments,
            },
        )
        .await?;

        // Manager on demand: the automatic spawn is GATED on the instance
        // setting (`stored → env PDO_MANAGER_ENABLED → false`), read FRESH at the
        // edge like every instance knob. It is NOT a permission — the Manager
        // tab's manual start is always available, which is what keeps the
        // setting honest — and it applies to future Runs only: a live Run is
        // unaffected by a mid-flight toggle.
        let manager_enabled = self
            .settings
            .defaults()
            .await
            .is_some_and(|d| d.manager_enabled);
        launch::launch(
            self,
            &run_id,
            &worktree_dir,
            !sandbox.mode.is_off(),
            started.name_hint,
            manager_enabled,
        )
        .await;

        info!(
            "Run {run_id} started for pipeline {}",
            launchable.pipeline.name
        );
        Ok(run_id)
    }
}
