//! What a new Run freezes before it exists: its provisioning (the rules and the
//! plan resolved at the source ref) and its isolation (the sandbox mode with
//! its staging profile's entries, env and image).

use std::collections::BTreeMap;

use axum::http::StatusCode;
use pdo_sandbox::domain::image::ProfileImage;
use pdo_workspace::adapters::fs::provisioner;
use pdo_workspace::domain::provisioning::{ProvisioningPlan, ProvisioningRules, ScopedRules};
use tracing::error;

use super::request::CreateRunRequest;
use super::target::{bad_request, Refusal, Target};
use crate::domain::state::SandboxMode;
use crate::Runs;

/// The provisioning a Run freezes: the rules of every tier, and the plan they
/// resolve to at the Run's source ref.
pub(super) struct Provisioning {
    pub rules: Vec<ScopedRules>,
    pub plan: ProvisioningPlan,
}

pub(super) async fn provisioning(
    state: &Runs,
    req: &CreateRunRequest,
    target: &Target,
) -> Result<Provisioning, Refusal> {
    let rules = match state
        .projects
        .provisioning_rules(
            &target.run_repo_root.to_string_lossy(),
            req.provisioning.clone(),
        )
        .await
    {
        Ok(rules) => rules,
        Err(e) => {
            error!("failed to load provisioning recipe: {e}");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "provisioning store unavailable" }),
            ));
        }
    };
    let plan =
        match provisioner::resolve_at_git_ref(&target.run_repo_root, &rules, &target.source_ref) {
            Ok(plan) if plan.conflicts.is_empty() => plan,
            Ok(plan) => {
                let conflict = &plan.conflicts[0];
                return Err(bad_request(format!(
                    "mode conflict in {:?}: `{}` is declared in multiple modes",
                    conflict.scope, conflict.relative_path
                )));
            }
            Err(e) => return Err(bad_request(format!("invalid provisioning recipe: {e:#}"))),
        };
    Ok(Provisioning { rules, plan })
}

/// The isolation a Run freezes: the effective mode and, for a staging profile,
/// what it stages, its env and its image, resolved together.
pub(super) struct Sandbox {
    pub mode: SandboxMode,
    pub entries: Option<Vec<String>>,
    pub env: BTreeMap<String, String>,
    pub image: Option<ProfileImage>,
}

/// CHOKEPOINT: resolve the effective sandbox mode ONCE, here, where the JSON,
/// multipart and trigger-fire paths converge, just before the mode is frozen
/// into `RunStarted`. `req.sandbox` already carries either the explicit
/// run-level choice or the trigger's mode (a Run has a single origin), so the
/// trigger arm stays `None` in production. `stored_default` is read FRESH so a
/// `PUT /settings` bites on the next create with no restart.
pub(super) async fn sandbox(
    state: &Runs,
    req: &CreateRunRequest,
    stored_default: Option<String>,
) -> Result<Sandbox, Refusal> {
    let explicit_tier = req.sandbox.is_some();
    let mode = pdo_sandbox::domain::mode::effective_sandbox(
        req.sandbox.clone(),
        None,
        pdo_sandbox::adapters::env::default_sandbox_with(stored_default),
    );

    // #432 (ADR-0031 §6 + §7): resolve the winning tier's staging profile to its
    // entry list HERE, at the same chokepoint, so the name AND the list are frozen
    // together into `RunStarted` — editing (or deleting) the profile afterwards can
    // never rewrite what this Run stages. Only the WINNER is resolved: an unknown
    // name sitting in the instance default is not an error for a Run that
    // explicitly picks another profile, because nothing ever consults it.
    //
    // An unknown name **fails hard**, never falls back to the instance default or
    // to `off`. This is before `append_event(run_started)` and before
    // `create_worktree`, so "no Run is created" holds in the strict sense. A
    // Trigger fire turns this 400's message into a `FireRecord { outcome: "error",
    // reason }` that the history renders red — which is why the wording below is
    // the deliverable, not an afterthought.
    //
    // The profile's env and its image SOURCE are frozen at the SAME resolve, for
    // the same reason: editing it afterwards cannot rewrite what this Run's
    // container was created with, and two of its nodes never land in two
    // different images (ADR-0031 §6/§9). One `resolve` feeds every key; two reads
    // could straddle a concurrent PUT.
    let mut frozen = Sandbox {
        mode: mode.clone(),
        entries: None,
        env: BTreeMap::new(),
        image: None,
    };
    let Some(name) = mode.profile() else {
        return Ok(frozen);
    };
    match pdo_sandbox::application::profiles::resolve(state.sandbox_profiles.as_ref(), name).await
    {
        Ok(Some(resolved)) => {
            frozen.env = resolved.env;
            frozen.image = resolved.image;
            frozen.entries = Some(resolved.resolved.entries);
            Ok(frozen)
        }
        Ok(None) => Err(bad_request(unknown_profile(
            name,
            explicit_tier,
            req.triggered_by.is_some(),
        ))),
        Err(e) => {
            error!("failed to resolve sandbox profile `{name}`: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "sandbox profile store error" }),
            ))
        }
    }
}

/// The refusal of an unknown staging profile, naming the tier that chose it.
fn unknown_profile(name: &str, explicit_tier: bool, triggered: bool) -> String {
    if !explicit_tier {
        return format!(
            "unknown sandbox profile `{name}`: the instance default sandbox names \
             a staging profile that does not exist. Fix it under \
             Settings → Default sandbox."
        );
    }
    if triggered {
        format!(
            "unknown sandbox profile `{name}`: the Trigger's sandbox setting \
             names a staging profile that does not exist. Recreate it under \
             Settings → Staging profiles, or point the Trigger at `off`."
        )
    } else {
        format!(
            "unknown sandbox profile `{name}`: no such staging profile. \
             Create it under Settings → Staging profiles, or use `off`."
        )
    }
}

/// The Run tier of the provisioning rules, as `retry_all` reproduces it.
pub(crate) fn run_tier(rules: &[ScopedRules]) -> ProvisioningRules {
    rules
        .iter()
        .find(|scoped| scoped.scope == pdo_workspace::domain::provisioning::ProvisioningScope::Run)
        .map(|scoped| scoped.rules.clone())
        .unwrap_or_default()
}
