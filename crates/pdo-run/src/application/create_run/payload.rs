//! The `RunStarted` of a new Run: everything it freezes, written once here,
//! never re-decided by a later edit of the pipeline, the instance, the bank or
//! the Trigger — the graph, the input and variables, the repositories, the
//! isolation, the harness choice, the skills, the preferences, the name, the
//! provenance and the data of the extensions.

use pdo_skill::domain::selection::ActiveSkill;
use pdo_workspace::domain::provisioning::ScopedRules;
use tracing::warn;

use super::freeze::Sandbox;
use super::naming::{placeholder_run_name, run_name_hint};
use super::request::{CreateRunRequest, InputFile};
use super::target::{Launchable, Target};
use crate::application::node::project_tiers::project_skills_for_path;
use crate::application::node::prompt::{default_auto_name_with, RunNameHint};
use crate::application::ports::InstanceDefaults;
use crate::application::session_claim::SessionClaim;
use crate::domain::event::{
    edge_info_from_pipeline, node_def_from_pipeline, EdgeInfo, Event, EventKind, NodeDefInfo,
};
use crate::domain::state::RunState;
use crate::Runs;

/// A new Run's `RunStarted`, and what its creation still needs of it.
pub(super) struct Started {
    pub event: Event,
    /// Whether the manager is told to rename the Run.
    pub name_hint: RunNameHint,
    /// The frozen skills selection, delivered into the Run worktree.
    pub skills: Vec<ActiveSkill>,
}

/// What the payload is built from.
pub(super) struct Frozen<'a> {
    pub req: &'a CreateRunRequest,
    pub target: &'a Target,
    pub launchable: &'a Launchable,
    pub provisioning_rules: &'a [ScopedRules],
    pub sandbox: &'a Sandbox,
    pub run_id: &'a str,
    pub fork_sha: Option<&'a str>,
    pub defaults: &'a InstanceDefaults,
    /// The Run whose node session created this one, and the claim (ADR-0064).
    pub parent: Option<&'a RunState>,
    pub session: Option<&'a SessionClaim>,
    pub attachments: &'a [InputFile],
}

pub(super) async fn run_started(state: &Runs, frozen: Frozen<'_>) -> Started {
    let Frozen {
        req,
        target,
        launchable,
        provisioning_rules,
        sandbox,
        run_id,
        fork_sha,
        defaults,
        parent,
        session,
        attachments,
    } = frozen;
    let pipeline = &launchable.pipeline;
    let edge_infos: Vec<EdgeInfo> = pipeline.edges.iter().map(edge_info_from_pipeline).collect();

    let node_def_infos: Vec<NodeDefInfo> =
        pipeline.nodes.iter().map(node_def_from_pipeline).collect();

    let variables_json = if req.variables.is_empty() {
        None
    } else {
        serde_json::to_value(&req.variables).ok()
    };

    let mut run_payload = serde_json::json!({
        "pipeline_name": pipeline.name,
        "input": req.input,
        "edges": edge_infos,
        "node_defs": node_def_infos,
    });
    if let Some(vars) = variables_json {
        run_payload["variables"] = vars;
    }
    // ADR-0086: written only when set, so a Run without the option keeps the
    // historical payload byte-identical.
    if pipeline.wait_for_started_nodes {
        run_payload["wait_for_started_nodes"] = serde_json::json!(true);
    }
    // Persist the canonical (trimmed, validated) path, never the raw field. Historical
    // events keep their legitimate `null`; the read side resolves those (ADR-0033).
    run_payload["target_repo"] = serde_json::json!(target.run_repo_root.to_string_lossy());
    // Carry the frozen secondary pins ONLY when there is at least one, so a mono-repo
    // Run's payload stays byte-identical to the historical shape.
    if !target.secondary_pins.is_empty() {
        run_payload["target_repos"] = serde_json::json!(target.secondary_pins);
    }
    // Carry the RESOLVED isolation mode only when it is NOT `off`, so `off` payloads
    // stay byte-identical (absence → Off). `sandbox_entries` is a SIBLING key written
    // in the same breath: written together or not at all is what makes the replay
    // table decidable — a `sandbox` with no entries can only mean an older daemon.
    // Don't nest them as `sandbox: {name, entries}`: every historical payload carries
    // `"sandbox": "full"` as a bare string, so both readers would have to accept
    // `String | Object` for ever.
    if !sandbox.mode.is_off() {
        run_payload["sandbox"] = serde_json::json!(sandbox.mode);
        run_payload["sandbox_entries"] = serde_json::json!(sandbox
            .entries
            .as_ref()
            .expect("a non-off sandbox always resolves an entry list at this chokepoint"));
        // Written ONLY when non-empty, and the asymmetry with `sandbox_entries` is
        // deliberate: an empty entry LIST is a legitimate resolution (`minimal`), so
        // absence there had to mean "older daemon". For the env there is nothing to
        // distinguish — an absent key and an empty map describe the same container.
        if !sandbox.env.is_empty() {
            run_payload["sandbox_env"] = serde_json::json!(sandbox.env);
        }
        // Same asymmetry as `sandbox_env`: an absent key means "the profile posed
        // none", indistinguishable from an older daemon precisely because both resolve
        // the same way, to the sandbox context's `DEFAULT_PROFILE_IMAGE`.
        if let Some(image) = &sandbox.image {
            run_payload["sandbox_image"] = serde_json::json!(image);
        }
    }
    if let Some(ref branch) = target.source_branch {
        run_payload["source_branch"] = serde_json::json!(branch);
    }
    // #804 / ADR-0070 §1: the Trigger fire could not refresh the remotes before
    // cutting. The Run is NOT failed by it — it was cut from the last state known
    // locally, and the Run detail says exactly that instead of leaving someone to
    // wonder why the fork point looks old. Written ONLY on failure, so every other
    // Run's payload keeps its historical shape.
    if let Some(ref err) = req.source_fetch_error {
        run_payload["source_fetch_error"] = serde_json::json!(err);
    }
    // FREEZE the resolved fork point into `RunStarted`. A resolve failure leaves it
    // absent, and the read-side `run_diff_base` ladder recovers via `source_branch`,
    // then `HEAD`.
    if let Some(fs) = fork_sha {
        run_payload["fork_sha"] = serde_json::json!(fs);
    }
    // FREEZE the Run's harness choice into `RunStarted`, same immutability posture as
    // `sandbox` above: editing the pipeline or the instance default afterwards must
    // never re-decide a Run in flight. Blank/whitespace is normalised to absent, so
    // `Some("")` can never persist as a harness name. No validation (ADR-0045): an
    // unknown name fails fast at the first node spawn, naming it.
    if let Some(h) = req
        .harness
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        run_payload["harness"] = serde_json::json!(h);
    }
    // FREEZE the Run's `AgentChoice` ATOMICALLY at spawn (ADR-0057 ¶4) — never
    // re-decided by a later profile edit or resume. Written ONLY for an explicit
    // choice with a non-blank payload, so a Run that states none lets `harness` above
    // decide the Run tier.
    let explicit_agent_choice = match &req.agent_choice {
        Some(pdo_harness::agent_choice::AgentChoice::Custom { harness, .. })
            if !harness.trim().is_empty() =>
        {
            req.agent_choice.clone()
        }
        Some(pdo_harness::agent_choice::AgentChoice::Profile { profile_id })
            if !profile_id.trim().is_empty() =>
        {
            req.agent_choice.clone()
        }
        _ => None,
    };
    if let Some(choice) = explicit_agent_choice {
        run_payload["agent_choice"] = serde_json::to_value(choice).unwrap_or_default();
    }
    // FREEZE the Run tier of the skills selection (#669, ADR-0062) — same posture as
    // `harness`/`agent_choice`: set once here, never re-decided by a later edit of
    // the bank or the Trigger. Written ONLY when non-empty, so a Run that selects
    // none keeps the historical payload shape. Not validated against the bank: an
    // unknown id is a spawn-time WARNING, never a refusal to launch (#669 AC).
    let run_skills = pdo_skill::domain::selection::normalise(req.skills.clone());
    if !run_skills.is_empty() {
        run_payload["skills"] = serde_json::to_value(&run_skills).unwrap_or_default();
    }
    // #672 / ADR-0062: FREEZE the resolved instance + Projet + Run selection (ids,
    // bank names, origin tiers) — the base every node spawn of this Run adds its own
    // tier to. The bank's names are read once here; a later rename or edit of the
    // bank never reaches this Run (its content is snapshotted below). Written only
    // when non-empty so a Run selecting nothing keeps the historical payload shape.
    // An id the bank does not know is a warning on the Run, never a refusal.
    let frozen_skills = {
        let instance_skills = defaults.skills.clone();
        let project_skills =
            project_skills_for_path(state, &target.run_repo_root.to_string_lossy())
                .await
                .unwrap_or_default();
        let bank = state.skills.bank_names().await;
        let tiers = pdo_skill::domain::selection::SkillTiers {
            instance: Some(&instance_skills),
            project: Some(&project_skills),
            run: Some(&run_skills),
            node: None,
        };
        let resolved = pdo_skill::domain::selection::resolve(&tiers, &bank);
        for missing in &resolved.missing {
            warn!(
                "run {run_id}: skill '{}' ({}) selected at {:?} is no longer in the bank; \
                 the Run proceeds without it",
                missing.name, missing.id, missing.tiers
            );
        }
        if !resolved.missing.is_empty() {
            run_payload["missing_skills"] =
                serde_json::to_value(&resolved.missing).unwrap_or_default();
        }
        if !resolved.skills.is_empty() {
            run_payload[pdo_skill::domain::delivery::RUN_STARTED_FROZEN_KEY] =
                serde_json::to_value(&resolved.skills).unwrap_or_default();
        }
        resolved.skills
    };
    // FREEZE the Run's `auto_fail` choice (ADR-0049), same immutability posture as
    // `harness`. Absent ⇒ resolve through the project / instance tiers.
    if let Some(af) = req.auto_fail {
        run_payload["auto_fail"] = serde_json::json!(af);
    }
    // Resolved ONCE here, at the same chokepoint as the sandbox default, and read
    // FRESH so a `PUT /settings` bites on the next create. Never cache it at boot
    // (the reaper-TTL trap of ADR-0015).
    //
    // The back-compat ordering is load-bearing: an omitted flag WITH a supplied `name`
    // must keep that name, so a caller passing `name` without the flag is unaffected.
    // Only an omitted flag AND no name consults the instance default.
    let auto_name = match req.auto_name {
        Some(b) => b,
        None => {
            if req.name.as_deref().is_some_and(|n| !n.trim().is_empty()) {
                false
            } else {
                default_auto_name_with(defaults.default_auto_name)
            }
        }
    };

    // Shared between the payload write here and the manager spawn below, so the two
    // can never disagree.
    let name_hint = run_name_hint(auto_name, req.name.as_deref(), &req.input);
    let display_name: Option<String> = match name_hint {
        // With no supplied name, fall back to the stable placeholder so a Trigger with
        // autonomy off does not leave every fire sharing one blank name — WITHOUT
        // re-arming the rename instruction, which lives in the hint.
        RunNameHint::UserProvided => Some(match req.name.as_deref() {
            Some(n) if !n.trim().is_empty() => n.to_string(),
            _ => placeholder_run_name(run_id),
        }),
        RunNameHint::Placeholder => Some(placeholder_run_name(run_id)),
        RunNameHint::DeriveFromInput => None,
    };
    if let Some(name) = &display_name {
        run_payload["name"] = serde_json::json!(name);
    }
    if let Some(ref trigger_id) = req.triggered_by {
        if !trigger_id.is_empty() {
            run_payload["triggered_by"] = serde_json::json!(trigger_id);
        }
    }
    // API 0.8: FREEZE who created the Run, and for a fired Run the Trigger's
    // last editor — the extensions read both at every node launch. Written only
    // when known, so a payload built without an Actor keeps its shape.
    if let Some(actor) = &req.created_by {
        run_payload["created_by"] = serde_json::json!(actor);
    }
    if let Some(editor) = &req.trigger_editor {
        run_payload["trigger_editor"] = serde_json::json!(editor);
    }
    // API 0.9: FREEZE the data the Run carries for the extensions, as checked
    // by them at the create. Written only when non-empty, so a Run carrying
    // none keeps its shape.
    if !req.extensions.is_empty() {
        run_payload["extensions"] = serde_json::json!(req.extensions);
    }
    // ADR-0064: FREEZE the mechanical parent provenance — decided by the daemon
    // from the verified session claim, never read from the body (which refuses
    // any parent key outright). Written ONLY when the create came from a live
    // node session, so an out-of-session create keeps the historical root shape.
    if let (Some(parent), Some(claim)) = (parent, session) {
        run_payload["parent_run_id"] = serde_json::json!(parent.run_id);
        run_payload["parent_node_id"] = serde_json::json!(claim.node_id);
    }
    // Carry the library pipeline id so aggregated "by pipeline" stats survive a
    // pipeline rename. The consumer falls back to `pipeline_name` when absent.
    if let Some(ref pid) = req.pipeline_id {
        if !pid.is_empty() {
            run_payload["pipeline_id"] = serde_json::json!(pid);
        }
    }
    // Upload order is kept: what the user sees on the Start node is what they
    // attached, in that order.
    let image_names: Vec<&str> = attachments
        .iter()
        .filter(|a| a.is_image)
        .map(|a| a.filename.as_str())
        .collect();
    if !image_names.is_empty() {
        run_payload["image_filenames"] = serde_json::json!(image_names);
    }
    let file_names: Vec<&str> = attachments
        .iter()
        .filter(|a| !a.is_image)
        .map(|a| a.filename.as_str())
        .collect();
    if !file_names.is_empty() {
        run_payload["file_filenames"] = serde_json::json!(file_names);
    }
    if provisioning_rules
        .iter()
        .any(|scoped| !scoped.rules.is_empty())
    {
        run_payload["provisioning_rules"] = serde_json::json!(provisioning_rules);
    }

    // `mut`: the skill delivery below (#672) appends `skipped_skills` once the
    // worktree exists, before the event is appended.
    let run_started = Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: EventKind::RunStarted,
        node_id: None,
        iter: None,
        payload: Some(run_payload),
    };
    Started {
        event: run_started,
        name_hint,
        skills: frozen_skills,
    }
}
