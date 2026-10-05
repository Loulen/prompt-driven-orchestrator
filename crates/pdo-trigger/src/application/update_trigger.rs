//! Edit a Trigger (`PATCH /triggers/{id}`): `trigger` × `update`. A partial
//! merge: an absent field is left untouched.

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
use crate::domain::trigger::{Trigger, UpdateTrigger};
use crate::Triggers;

/// A partial Trigger edit. Every field is optional; absent fields are left
/// untouched.
#[derive(Debug, Clone, Deserialize)]
pub struct PatchTriggerRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// Repoint the Trigger to a different library pipeline. Validated against the
    /// library/repo before applying; the denormalised display name and a revived
    /// next-fire follow from it.
    #[serde(default)]
    pub pipeline_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub input_template: Option<String>,
    #[serde(default)]
    pub overlap_policy: Option<String>,
    /// Double-wrapped AND custom-deserialized, unlike its plain siblings below: the
    /// point is to make the explicit CLEAR (`"target_repo": null`) *reachable* so it
    /// can be REFUSED. With plain `#[serde(default)]` a present-`null` collapses to
    /// `None` and the clear is a silent no-op, which is what the UI was sending on
    /// every Trigger save.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub target_repo: Option<Option<String>>,
    /// The secondary list, double-wrapped like `target_repo`: absent = leave, present
    /// `null`/`[]` = clear to mono-repo, an array = set. Stored as raw JSON TEXT.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub target_repos: Option<Option<Vec<TargetRepoInput>>>,
    #[serde(default)]
    pub source_branch: Option<Option<String>>,
    #[serde(default)]
    pub guard_command: Option<Option<String>>,
    #[serde(default)]
    pub variables: Option<HashMap<String, serde_yaml::Value>>,
    /// Bounded-`allow` ceiling, double-wrapped: absent = leave, present `null` =
    /// clear to unbounded, `n` = set. `source_branch` / `guard_command` above keep
    /// plain `#[serde(default)]`, where present-`null` collapses and cannot clear.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub max_concurrent: Option<Option<i64>>,
    /// Per-Trigger sandbox mode, double-wrapped like `max_concurrent`: present `null`
    /// clears back to inheriting the instance default, which is how the UI offers
    /// "use the instance default".
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub sandbox: Option<Option<String>>,
    /// Per-Trigger harness, double-wrapped like `sandbox`; present `null` clears back
    /// to inheriting the instance default.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub harness: Option<Option<String>>,
    /// Per-Trigger `AgentChoice`, double-wrapped like `harness`; present `null` clears
    /// back to inheriting the legacy `harness` field.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub agent_choice: Option<Option<AgentChoice>>,
    /// The Run-tier skills (#669), replaced wholesale: absent leaves them, a list
    /// sets them, an empty list clears them. Flat: the empty list IS the clear.
    #[serde(default)]
    pub skills: Option<Vec<SkillRef>>,
    /// Auto-naming toggle — a FLAT `Option<bool>`, deliberately not double-wrapped:
    /// there is no "inherit" state to clear back to, a Trigger's autonomy is a plain
    /// on/off.
    #[serde(default)]
    pub auto_name: Option<bool>,
}

/// Deserialize a double-`Option` PATCH field so a present `null` maps to `Some(None)`
/// ("clear to NULL"), distinct from absent → `None` ("leave untouched"). serde's
/// default for `Option<Option<T>>` collapses both to `None`; this helper is only
/// invoked when the key is present, which is what recovers the three-way distinction.
fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

impl Triggers {
    /// Apply `req` to the Trigger `id`, re-checking the create-time rules against
    /// the *resulting* configuration, then announce and journal both snapshots.
    pub async fn update(
        &self,
        actor: &Actor,
        hint: ActorHint,
        id: &str,
        req: PatchTriggerRequest,
    ) -> Result<Trigger, TriggerError> {
        self.authorize(actor, &operation(Verb::Update, Some(id)))?;
        let existing = match self.store.get(id).await {
            Ok(Some(t)) => t,
            Ok(None) => return Err(TriggerError::NotFound("trigger not found".to_string())),
            Err(e) => {
                error!("failed to load trigger for patch: {e}");
                return Err(TriggerError::Storage("failed to load trigger".to_string()));
            }
        };
        if req
            .name
            .as_deref()
            .is_some_and(|name| !rules::valid_name(name))
        {
            return Err(TriggerError::Invalid(EMPTY_NAME.to_string()));
        }
        let mut edit = self.edit_of(&existing, req).await?;
        // API 0.8: whoever edits a Trigger answers for the Runs it fires next.
        edit.last_editor = Some(actor.clone());
        if let Err(e) = self.store.update(id, edit).await {
            error!("failed to update trigger: {e}");
            return Err(TriggerError::Storage(
                "failed to update trigger".to_string(),
            ));
        }
        let Ok(Some(updated)) = self.store.get(id).await else {
            return Err(TriggerError::Storage(
                "failed to reload trigger".to_string(),
            ));
        };
        self.announce(serde_json::json!({
            "type": "trigger_updated",
            "trigger_id": id,
        }));
        // Persist BOTH snapshots (ADR-0044 §2·D): "a patch happened" cannot tell
        // a manual `enabled:false` from a cron edit unless before→after is on the
        // row.
        self.journal(
            hint,
            "trigger.updated",
            ("trigger", Some(id)),
            serde_json::to_value(&existing).ok(),
            serde_json::to_value(&updated).ok(),
        )
        .await;
        Ok(self.with_suspension(updated))
    }

    /// The validated edit `req` makes to `existing`, with its recomputed next fire.
    async fn edit_of(
        &self,
        existing: &Trigger,
        req: PatchTriggerRequest,
    ) -> Result<UpdateTrigger, TriggerError> {
        // One `now` for the whole request: a combined enable + cron-edit + repoint PATCH
        // must recompute every slot against the same instant.
        let now = chrono::Utc::now();
        let mut next_fire_at: Option<Option<String>> = None;
        if let Some(cron) = &req.cron {
            let schedule = CronSchedule::parse(cron)
                .map_err(|e| TriggerError::Invalid(format!("invalid cron expression: {e}")))?;
            next_fire_at = recompute_next_fire(now, Transition::CronEdit(&schedule));
        }

        // Resolve the target's document to capture the display name (denormalised on
        // the row) and the prompt_required flag the reject rule below needs.
        let mut repoint: Option<(String, String, bool)> = None;
        if let Some(new_pid) = req
            .pipeline_id
            .as_ref()
            .filter(|p| **p != existing.pipeline_id)
        {
            let facts = match self.pipelines.resolve(new_pid) {
                None => {
                    return Err(TriggerError::Invalid(format!(
                        "pipeline not found: {new_pid}"
                    )))
                }
                Some(Err(e)) => {
                    return Err(TriggerError::Invalid(format!("pipeline parse error: {e}")))
                }
                Some(Ok(facts)) => facts,
            };
            repoint = Some((new_pid.clone(), facts.name, facts.prompt_required));
            // Revive the next fire from the existing cron unless a schedule edit
            // already recomputed it, so a Trigger that went dormant on a pipeline-id
            // rename comes back to life on repoint instead of staying dead.
            if next_fire_at.is_none() {
                if let Ok(schedule) = CronSchedule::parse(&existing.cron) {
                    next_fire_at = recompute_next_fire(now, Transition::Repoint(&schedule));
                }
            }
        }

        // Re-activating a disabled Trigger recomputes next_fire_at FORWARD from now:
        // the missed slot is SKIPPED, no catch-up (ADR-0012 amended). Merely flipping
        // the bit leaves next_fire_at frozen in the past and the next tick fires an
        // implicit catch-up. `enabling` is the disabled→enabled front ONLY: an
        // `{enabled:true}` on an already-live Trigger must not shift its schedule. The
        // `next_fire_at.is_none()` guard is load-bearing — a combined enable + cron-edit
        // already recomputed off the *new* schedule, and Enable would overwrite it with
        // the *existing* cron.
        let enabling = !existing.enabled && req.enabled == Some(true);
        if enabling && next_fire_at.is_none() {
            if let Ok(schedule) = CronSchedule::parse(&existing.cron) {
                next_fire_at = recompute_next_fire(now, Transition::Enable(&schedule));
            }
        }

        // PATCH stays a partial merge: an ABSENT `target_repo` leaves the stored value
        // untouched, which is what makes `{"enabled": true}` a legal body. What is
        // refused is an explicit CLEAR (`null` or a blank string).
        let target_repo = match &req.target_repo {
            Some(edit) => Some(Some(
                self.repositories
                    .required(edit.as_deref())
                    .map_err(TriggerError::Invalid)?
                    .to_string_lossy()
                    .into_owned(),
            )),
            None => None,
        };
        if let Some(Some(n)) = req.max_concurrent {
            rules::validate_max_concurrent(Some(n))
                .map_err(|msg| TriggerError::Invalid(msg.to_string()))?;
        }
        // Same shared existence gate as create and `PUT /settings`.
        if let Some(Some(mode)) = &req.sandbox {
            self.sandbox_profiles
                .validate(mode)
                .await
                .map_err(TriggerError::Invalid)?;
        }

        // Re-apply the create-time reject rule against the *resulting* config. When
        // repointing, judge the *new* pipeline's prompt_required, not the old one's.
        let resulting_guard = match &req.guard_command {
            Some(g) => g.clone(),
            None => existing.guard_command.clone(),
        };
        let resulting_input = req
            .input_template
            .clone()
            .unwrap_or_else(|| existing.input_template.clone());
        let prompt_required = match &repoint {
            Some((_, _, required)) => *required,
            None => self.prompt_required(existing),
        };
        if rules::lacks_input(
            prompt_required,
            resulting_guard.as_deref(),
            &resulting_input,
        ) {
            return Err(TriggerError::Invalid(PROMPT_REQUIRED.to_string()));
        }

        Ok(UpdateTrigger {
            name: req.name,
            pipeline_id: repoint.as_ref().map(|(id, _, _)| id.clone()),
            pipeline_name: repoint.map(|(_, name, _)| name),
            // The trimmed, validated path — never the raw field.
            target_repo,
            // Absent → leave; present `null`/`[]` → clear to mono-repo; a non-empty array
            // → the serialized JSON.
            target_repos: req.target_repos.map(|inner| {
                inner
                    .filter(|list| !list.is_empty())
                    .and_then(|list| serde_json::to_string(&list).ok())
            }),
            source_branch: req.source_branch,
            input_template: req.input_template,
            variables: req
                .variables
                .map(|v| serde_json::to_value(v).unwrap_or(serde_json::json!({}))),
            cron: req.cron,
            guard_command: req.guard_command,
            overlap_policy: req.overlap_policy.map(|p| rules::overlap_policy(&p)),
            max_concurrent: req.max_concurrent,
            sandbox: req.sandbox,
            // Normalise a stray `Some(Some(""))` to a clean clear, so a blank never
            // persists as a harness named "".
            harness: req.harness.map(rules::non_blank),
            agent_choice: req.agent_choice,
            skills: req.skills,
            auto_name: req.auto_name,
            next_fire_at,
            // The enable bit and the forward next_fire_at must land in ONE atomic UPDATE,
            // or enabled=1 can persist with a stale (past) next_fire_at.
            enabled: req.enabled,
            // Set by `update`, which knows the Actor.
            last_editor: None,
        })
    }
}
