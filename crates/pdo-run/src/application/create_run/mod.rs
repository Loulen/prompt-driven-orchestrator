//! Creating a Run (`POST /runs`, `pdo run create`): the one create path the
//! JSON, multipart, CLI, retry and Trigger-fire callers converge on.
//!
//! [`Runs::create_run`] is the gesture of an actor: it refuses an unknown
//! extension id, asks the Autorisation d'opération (`run` × `create`), then
//! each extension whose data the request carries (`validate_create`), before
//! any effect. [`Runs::start_run`] is the creation itself, for a caller that
//! has already passed its own gate: a `retry_all` command, a Trigger fire. In
//! order: the session claim (ADR-0064), the repositories ([`target`]), the
//! pipeline, the Projet ([`project`], ADR-0087), the provisioning and the isolation ([`freeze`]), the `RunStarted`
//! payload ([`payload`]), the Run on disk ([`materialize`]), the extensions'
//! attributes, then its start ([`launch`]).

pub mod freeze;
mod launch;
mod materialize;
pub mod naming;
mod payload;
pub(crate) mod project;
pub mod repos;
pub mod request;
pub mod target;

use std::sync::Arc;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdo_kernel::attributes::{self, Attributes, EntityRef, ExtensionData};
use pdo_kernel::extension_host::CreationRefusal;
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
        // API 0.9: an extension id nobody installed is a client mistake named
        // like an unknown field, before anything else.
        let installed = self.extensions().installed();
        if let Some(id) = req.extensions.keys().find(|id| !installed.contains(id)) {
            let refusal = CreationRefusal::UnknownExtension {
                id: id.clone(),
                installed,
            };
            return (StatusCode::BAD_REQUEST, Json(refusal.body())).into_response();
        }
        let operation = Operation::new(ResourceKind::Run, Verb::Create)
            .in_projects(self.projects_of_new_run(&req, session.as_ref()).await);
        if let Err(denied) = self.authorize(actor, &operation).await {
            return denied.into_response();
        }
        // ADR-0087: launched in a Projet, the Run works in one of its
        // repositories.
        if let Err((status, body)) = self.check_primary_in_project(&req).await {
            return (status, Json(body)).into_response();
        }
        // Then each extension checks its value, before any effect.
        if let Err(refusal) =
            self.extensions()
                .validate_create(actor, &ResourceKind::Run, &req.extensions)
        {
            return (StatusCode::BAD_REQUEST, Json(refusal.body())).into_response();
        }
        req.created_by = Some(actor.clone());
        self.create_run_response(req, attachments, session).await
    }

    /// Stamp the extensions' attributes on the Run `run_id`, which `actor` just
    /// created, each extension handed its value of `data`. Returns them.
    async fn stamp_run(&self, actor: &Actor, run_id: &str, data: &ExtensionData) -> Attributes {
        let entity = EntityRef::new(ResourceKind::Run, run_id);
        attributes::stamp_with(self.extensions(), actor, entity, data).await
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
        let project = project::resolve(self, &req, parent.as_ref()).await?;
        let provisioning = freeze::provisioning(self, &req, &target, project.as_deref()).await?;

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
                project: project.as_deref(),
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
        // The extensions stamp the Run (each handed its value of `extensions`)
        // BEFORE its first Node is launched, on every path that launches one
        // (a gesture, a Trigger's fire, a retry): what they stamp is there
        // when the Node reads it. A Run nobody is known to have created (a
        // retry of a Run older than `created_by`) is stamped by nobody.
        if let Some(actor) = &req.created_by {
            self.stamp_run(actor, &run_id, &req.extensions).await;
        }

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
