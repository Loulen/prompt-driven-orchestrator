//! Create a Trigger (`POST /triggers`): `trigger` × `create`.

use std::collections::HashMap;

use pdo_harness::agent_choice::AgentChoice;
use pdo_kernel::{Actor, Verb};
use pdo_platform::domain::audit::ActorHint;
use pdo_run::application::create_run::TargetRepoInput;
use pdo_skill::domain::selection::SkillRef;
use serde::Deserialize;
use tracing::error;

use super::{operation, TriggerError};
use crate::domain::cron_schedule::CronSchedule;
use crate::domain::rules::{self, EMPTY_NAME, PROMPT_REQUIRED};
use crate::domain::schedule::{recompute_next_fire, Transition};
use crate::domain::trigger::{NewTrigger, Trigger};
use crate::Triggers;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTriggerRequest {
    pub name: String,
    /// Library pipeline id the Trigger fires.
    pub pipeline_id: String,
    #[serde(default)]
    pub target_repo: Option<String>,
    /// Secondary repos to associate with fired Runs (ADR-0042). Stored as raw JSON
    /// TEXT and forwarded at fire time. Absent → mono-repo fire.
    #[serde(default)]
    pub target_repos: Vec<TargetRepoInput>,
    #[serde(default)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub input_template: String,
    #[serde(default)]
    pub variables: HashMap<String, serde_yaml::Value>,
    /// 5-field cron expression.
    pub cron: String,
    #[serde(default)]
    pub guard_command: Option<String>,
    #[serde(default = "default_overlap_policy")]
    pub overlap_policy: String,
    /// Bounded-`allow` ceiling: max simultaneous live Runs. `None` = unbounded.
    /// Inert unless `overlap_policy == "allow"`.
    #[serde(default)]
    pub max_concurrent: Option<i64>,
    /// Per-Trigger sandbox mode: `"off"` | `"full"` | `"minimal"`, or absent/`null` to
    /// inherit the instance default. Read at fire time and folded into the create
    /// request's explicit tier.
    #[serde(default)]
    pub sandbox: Option<String>,
    /// Per-Trigger harness (ADR-0046): a harness name, or absent/`null`/blank to
    /// inherit the instance default. Folded at fire time into the fired Run's harness —
    /// there is no separate Trigger tier. Free text, no validation (ADR-0045).
    #[serde(default)]
    pub harness: Option<String>,
    /// Per-Trigger `AgentChoice` (ADR-0057): `Inherit`/absent falls through to the
    /// legacy `harness` field above; `Profile`/`Custom` is folded into the fired Run's
    /// explicit tier at fire time.
    #[serde(default)]
    pub agent_choice: Option<AgentChoice>,
    /// The Run-tier skills every fired Run carries (#669, ADR-0062). Folded into
    /// the fired Run's `skills` at fire time, like `harness`.
    #[serde(default)]
    pub skills: Vec<SkillRef>,
    /// Whether Runs this Trigger fires are auto-named. Seeded from the instance
    /// default in the create modal and frozen on the row; defaults to `true` so an
    /// older client that omits the field keeps auto-naming.
    #[serde(default = "default_true")]
    pub auto_name: bool,
}

fn default_overlap_policy() -> String {
    "skip".to_string()
}

/// Matches the `NOT NULL DEFAULT 1` column and the built-in resolver default.
fn default_true() -> bool {
    true
}

impl Triggers {
    /// Validate, store, announce and journal a new Trigger. Refused before any
    /// check when an extension says no.
    pub async fn create(
        &self,
        actor: &Actor,
        hint: ActorHint,
        req: CreateTriggerRequest,
    ) -> Result<Trigger, TriggerError> {
        self.authorize(actor, &operation(Verb::Create, None))?;
        if !rules::valid_name(&req.name) {
            return Err(TriggerError::Invalid(EMPTY_NAME.to_string()));
        }
        // Fail loud at config time, not at 3am (ADR-0001).
        let schedule = CronSchedule::parse(&req.cron)
            .map_err(|e| TriggerError::Invalid(format!("invalid cron expression: {e}")))?;
        rules::validate_max_concurrent(req.max_concurrent)
            .map_err(|msg| TriggerError::Invalid(msg.to_string()))?;
        if let Some(raw) = req.sandbox.as_deref() {
            self.sandbox_profiles
                .validate(raw)
                .await
                .map_err(TriggerError::Invalid)?;
        }
        let pipeline = match self.pipelines.resolve(&req.pipeline_id) {
            None => {
                return Err(TriggerError::Invalid(format!(
                    "pipeline not found: {}",
                    req.pipeline_id
                )))
            }
            Some(Err(e)) => {
                return Err(TriggerError::Invalid(format!("pipeline parse error: {e}")))
            }
            Some(Ok(facts)) => facts,
        };
        // A cron-only Trigger has no guard, so the input template is the only input
        // source: empty + prompt-required is a misconfiguration. Refuse at creation.
        if rules::lacks_input(
            pipeline.prompt_required,
            req.guard_command.as_deref(),
            &req.input_template,
        ) {
            return Err(TriggerError::Invalid(PROMPT_REQUIRED.to_string()));
        }
        // A Trigger is a Run template (ADR-0012b), so it cannot be valid without a
        // target repo.
        let target_repo = self
            .repositories
            .required(req.target_repo.as_deref())
            .map_err(TriggerError::Invalid)?
            .to_string_lossy()
            .into_owned();

        // The first fire, via the single recompute seam. `NewTrigger.next_fire_at` is a
        // flat `Option<String>`, so flatten what `Create` returns; `Some(None)` = an
        // impossible cron, which stays dormant.
        let next_fire_at =
            recompute_next_fire(chrono::Utc::now(), Transition::Create(&schedule)).flatten();
        let new = NewTrigger {
            name: req.name,
            pipeline_id: req.pipeline_id,
            pipeline_name: pipeline.name,
            target_repo: Some(target_repo),
            target_repos: if req.target_repos.is_empty() {
                None
            } else {
                serde_json::to_string(&req.target_repos).ok()
            },
            source_branch: req.source_branch,
            input_template: req.input_template,
            variables: serde_json::to_value(&req.variables).unwrap_or(serde_json::json!({})),
            cron: req.cron,
            guard_command: rules::non_blank(req.guard_command),
            overlap_policy: rules::overlap_policy(&req.overlap_policy),
            max_concurrent: req.max_concurrent,
            // Normalise `Some("")` to `None` (inherit the instance default), so an empty
            // selector value never persists as a bogus stored mode.
            sandbox: rules::non_blank(req.sandbox),
            // Same normalisation for the harness: a blank value must inherit at fire time
            // rather than persist as a harness named "".
            harness: rules::non_blank(req.harness),
            // No normalisation needed: `None`/`Inherit`/blank payloads are already
            // transparent inside `pdo_harness::agent_choice::resolve`.
            agent_choice: req.agent_choice,
            // #669: the store normalises (trim, dedupe) and stores NULL for empty.
            skills: req.skills,
            // Frozen on the row, never re-resolved at fire time — the runtime never
            // decides autonomy (ADR-0012 frontier).
            auto_name: req.auto_name,
            next_fire_at,
        };

        let trigger = self.store.create(new).await.map_err(|e| {
            error!("failed to create trigger: {e}");
            TriggerError::Storage("failed to persist trigger".to_string())
        })?;
        self.announce(serde_json::json!({
            "type": "trigger_created",
            "trigger_id": trigger.id,
        }));
        self.journal(
            hint,
            "trigger.created",
            ("trigger", Some(&trigger.id)),
            None,
            serde_json::to_value(&trigger).ok(),
        )
        .await;
        Ok(trigger)
    }
}
