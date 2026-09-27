//! What a spawn resolves before any side effect, frozen into `NodeStarted`:
//! the harness (ADR-0046, ADR-0057) and the skills actifs (ADR-0062).

use pdo_harness::agent_choice;
use tracing::warn;

use super::warnings::*;
use super::*;
use crate::application::settings::stored_default_harness;
use crate::application::settings::stored_default_harness_models;
use crate::domain::event;
use pdo_pipeline::domain::pipeline;

use super::guards::Loaded;

/// The agentic combination the node launches on; `None` for a `script` node.
pub(super) async fn harness(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    loaded: &Loaded,
) -> Option<agent_choice::ResolvedCombo> {
    let run_id = spawn_ctx.run_id;
    let projected = loaded.as_ref().map(|(_, s)| s);
    // #550/ADR-0046: resolve the harness ONCE, here — before any side effect — so
    // its result freezes into `NodeStarted` and is re-posed at resume (ADR-0007).
    // A `script` node launches no agent (ADR-0017), so it resolves no harness. The
    // binary fail-fast runs BEFORE the sub-worktree is created and BEFORE the
    // reservation span, so a missing harness leaves no orphan and writes no spawn
    // event (AC #10 / ADR-0037: never a 2xx for a spawn that did not happen).
    // Skipped under the tmux command override — the test seam launches a stand-in,
    // not the real binary.
    let resolved_harness = if node.node_type == pipeline::NodeType::Script {
        None
    } else {
        let default_harness = stored_default_harness(deps.runs).await;
        let default_models = stored_default_harness_models(deps.runs).await;
        // #552/ADR-0046: the Projet tier — the harness carried by the Projet that
        // owns this Run's **primary** repo. The primary is `target_repo` (else the
        // daemon repo root — the same `effective_repo` key the lists group and
        // attach by). A secondary repo (ADR-0042) lives in `target_repos` and is
        // never consulted, so adding or removing one changes neither the Projet nor
        // the resolved harness. A DB error degrades the tier to transparent — a
        // Projet lookup never fails a spawn.
        let primary_repo = projected
            .and_then(|s| s.target_repo.clone())
            .unwrap_or_else(|| spawn_ctx.repo_root.to_string_lossy().into_owned());
        let project_harness =
            match crate::application::node::project_tiers::project_harness_for_path(
                deps.runs,
                &primary_repo,
            )
            .await
            {
                Ok(h) => h,
                Err(e) => {
                    warn!("project harness lookup failed for {primary_repo}: {e}");
                    None
                }
            };
        // #563/ADR-0057: the Projet's `AgentChoice`, read alongside its legacy
        // `harness` above — same primary-repo key, same "a DB error degrades the
        // tier to transparent" posture.
        let project_choice =
            match crate::application::node::project_tiers::project_agent_choice_for_path(
                deps.runs,
                &primary_repo,
            )
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!("project agent_choice lookup failed for {primary_repo}: {e}");
                    None
                }
            };
        // #563: a second fresh `instance_config::get` rather than plumbing the whole
        // config through `stored_default_*`. A DB error degrades the tier to
        // transparent, same posture as every other tier here.
        let instance_choice = deps
            .runs
            .settings
            .defaults()
            .await
            .and_then(|c| c.agent_choice);
        // #563: the atomic profile snapshot — taken ONCE per spawn (ADR-0057 ¶4),
        // so every `Profile` reference this resolution touches (node/run/project/
        // instance) resolves against the exact same point-in-time read. A DB error
        // degrades to an empty snapshot: `agent_choice::resolve` still falls through
        // to its defensive `claude` floor rather than failing the spawn.
        let profiles = deps.runs.settings.agent_profiles().await;
        let tiers = agent_choice::Tiers {
            node_choice: node.agent_choice.as_ref(),
            node_pin: node.pin_harness.as_deref(),
            node_harnesses: Some(&node.harnesses),
            // #551: the Run tier — the harness frozen in this Run's `RunStarted`
            // (`projected` is the fresh projection loaded at the head of this spawn).
            // A pinned node still ignores it; a free node follows it (ADR-0046).
            run_choice: projected.and_then(|s| s.agent_choice.as_ref()),
            run_harness: projected.and_then(|s| s.harness.as_deref()),
            // #552: the Projet tier — resolved just above from this Run's primary
            // repo. Sits below the Run and above the instance default (ADR-0046).
            project_choice: project_choice.as_ref(),
            project_harness: project_harness.as_deref(),
            instance_choice: instance_choice.as_ref(),
            instance_default_harness: default_harness.as_deref(),
            instance_default_models: Some(&default_models),
        };
        let resolved = agent_choice::resolve(&tiers, &profiles, agent_choice::DEFAULT_PROFILE_ID);
        for warning in &resolved.warnings {
            warn_missing_agent_profile_once(run_id, warning);
        }
        Some(resolved.combo)
    };
    resolved_harness
}

/// The skills actifs of this NodeRun; `None` for a `script` node.
pub(super) async fn skills(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    loaded: &Loaded,
) -> Option<pdo_skill::domain::selection::ResolvedSkills> {
    let run_id = spawn_ctx.run_id;
    let projected = loaded.as_ref().map(|(_, s)| s);
    // #669 / ADR-0062: the **skills actifs** of this NodeRun — the strict
    // additive union of the four tiers, resolved HERE at spawn like the harness
    // (ADR-0046 sharing: editing a tier reaches a node not yet launched, never a
    // live one). The Run tier is the list FROZEN on `RunStarted`; the Projet and
    // instance tiers are read fresh, keyed by the same primary repo as the harness
    // above; the Node tier is the document's `skills`. The bank snapshot is taken
    // ONCE so every id resolves against the same point in time. A DB error
    // degrades a tier to empty — a skill lookup never fails a spawn. A `script`
    // node carries none (refused at parse) and launches no agent: `None`.
    //
    // #672: the instance + Projet + Run tiers come from the selection FROZEN on
    // `RunStarted` (ids AND names — the delivered folder is named after the frozen
    // name, so a rename in the bank after the Run started never delivers the same
    // skill twice under two names). Only the Node tier is read from the document
    // here. A Run created before #672 has no frozen base: its outer tiers are read
    // fresh, as #669 did.
    let resolved_skills = if node.node_type == pipeline::NodeType::Script {
        None
    } else {
        let frozen_base = loaded.as_ref().and_then(|(events, _)| {
            events
                .iter()
                .find(|e| e.kind == event::EventKind::RunStarted)
                .and_then(|e| e.payload.as_ref())
                .and_then(pdo_skill::domain::delivery::FrozenRunSkills::from_run_started)
        });
        let mut bank = deps.skills.bank_names().await;
        let (instance_skills, project_skills, run_skills): (
            Vec<pdo_skill::domain::selection::SkillRef>,
            Vec<pdo_skill::domain::selection::SkillRef>,
            Vec<pdo_skill::domain::selection::SkillRef>,
        ) = match &frozen_base {
            Some(base) => {
                let mut by_tier = (Vec::new(), Vec::new(), Vec::new());
                // Known skills pin their frozen name in the bank map; the ids the bank
                // had already lost at create are re-posed in their tiers WITHOUT a
                // bank entry, so `resolve` reports them missing again on this node.
                let frozen_refs = base
                    .skills
                    .iter()
                    .map(|s| {
                        bank.insert(s.id.clone(), s.name.clone());
                        (s.id.clone(), s.name.clone(), s.tiers.clone())
                    })
                    .chain(
                        base.missing
                            .iter()
                            .map(|m| (m.id.clone(), m.name.clone(), m.tiers.clone())),
                    );
                for (id, name, tiers) in frozen_refs {
                    let sref = pdo_skill::domain::selection::SkillRef { id, name };
                    for tier in &tiers {
                        match tier {
                            pdo_skill::domain::selection::SkillTier::Instance => {
                                by_tier.0.push(sref.clone())
                            }
                            pdo_skill::domain::selection::SkillTier::Project => {
                                by_tier.1.push(sref.clone())
                            }
                            pdo_skill::domain::selection::SkillTier::Run => {
                                by_tier.2.push(sref.clone())
                            }
                            pdo_skill::domain::selection::SkillTier::Node => {}
                        }
                    }
                }
                by_tier
            }
            None => {
                let primary_repo = projected
                    .and_then(|s| s.target_repo.clone())
                    .unwrap_or_else(|| spawn_ctx.repo_root.to_string_lossy().into_owned());
                let project_skills =
                    match crate::application::node::project_tiers::project_skills_for_path(
                        deps.runs,
                        &primary_repo,
                    )
                    .await
                    {
                        Ok(skills) => skills,
                        Err(e) => {
                            warn!("project skills lookup failed for {primary_repo}: {e}");
                            Vec::new()
                        }
                    };
                let instance_skills = deps
                    .runs
                    .settings
                    .defaults()
                    .await
                    .map(|c| c.skills)
                    .unwrap_or_default();
                (
                    instance_skills,
                    project_skills,
                    projected.map(|s| s.skills.clone()).unwrap_or_default(),
                )
            }
        };
        let tiers = pdo_skill::domain::selection::SkillTiers {
            instance: Some(&instance_skills),
            project: Some(&project_skills),
            run: Some(&run_skills),
            node: Some(&node.skills),
        };
        let resolved = pdo_skill::domain::selection::resolve(&tiers, &bank);
        for missing in &resolved.missing {
            warn_missing_skill_once(run_id, missing);
        }
        Some(resolved)
    };
    resolved_skills
}
