//! The steps of the start primitive that can refuse it: where the NodeRun
//! works, which harness it runs on, the staging set it carries.

use std::path::PathBuf;

use pdo_harness::{adapters::binary_path, registry, resolver};
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::{
    ensure_sub_worktree, sub_worktree_branch, sub_worktree_path,
};
use pdo_workspace::domain::provisioning::ProvisioningRules;

use super::StartNodeParams;

/// Where the NodeRun works: its sub-worktree, ensured and provisioned (with the
/// commit it was cut from), or the Run's worktree; the reason otherwise.
pub(super) fn prepare_working_dir(
    params: &StartNodeParams<'_>,
    node: &pipeline::NodeDef,
    has_sub_worktree: bool,
) -> Result<(PathBuf, Option<String>, ProvisioningRules), String> {
    // ADR-0036: the commit the sub-worktree was cut from, recorded on
    // `NodeStarted` below. Without it a merge-back conflict on this iteration can
    // never be resolved in the node's favour. Record it on this path too, not just
    // in `node_spawn`.
    let mut spawn_base_sha: Option<String> = None;
    let node_provisioning = if has_sub_worktree {
        match crate::application::node::launch::node_rules_from_pipeline(
            params.pipeline_path,
            &node.id,
        ) {
            Ok(rules) => rules,
            Err(e) => {
                return Err(format!(
                    "provisioning failed for {}: {e:#}; no node was spawned",
                    node.id
                ));
            }
        }
    } else {
        pdo_workspace::domain::provisioning::ProvisioningRules::default()
    };
    let working_dir = if has_sub_worktree {
        let sub_wt_dir =
            sub_worktree_path(params.repo_root, params.run_id, params.node_id, params.iter);
        let sub_branch = sub_worktree_branch(params.run_id, params.node_id, params.iter);
        let pipeline_branch = format!("pdo/run-{}", params.run_id);

        // No `previous_base_sha` to carry: `has_node_started_event` above already
        // returned `AlreadyDone` for any started iteration, so the `Reusable` arm
        // is unreachable here — this site only creates or recycles.
        match ensure_sub_worktree(
            params.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
            None,
        ) {
            Ok(ensured) => {
                spawn_base_sha = ensured.base_sha;
                if ensured.created {
                    if let Err(e) =
                        pdo_workspace::adapters::fs::provisioner::provision_node_worktree(
                            params.repo_root,
                            &sub_wt_dir,
                            &params.run_state.provisioning_rules,
                            &node_provisioning,
                            &pipeline_branch,
                        )
                    {
                        pdo_workspace::adapters::git::worktrees::reap_orphan_sub_worktree(
                            params.repo_root,
                            &sub_wt_dir,
                            &sub_branch,
                        );
                        return Err(format!(
                            "provisioning failed for {}: {e:#}; no node was spawned",
                            node.id
                        ));
                    }
                }
            }
            Err(e) => {
                return Err(format!("failed to ensure sub-worktree: {e:#}"));
            }
        }
        sub_wt_dir
    } else {
        params.worktree_dir.to_path_buf()
    };
    Ok((working_dir, spawn_base_sha, node_provisioning))
}

/// The harness a non-script node runs on, resolved like `node_spawn`'s
/// (ADR-0046); the reason when it cannot be.
pub(super) fn resolve_harness(
    params: &StartNodeParams<'_>,
    node: &pipeline::NodeDef,
    is_script: bool,
) -> Result<
    (
        Option<resolver::ResolvedHarness>,
        Option<registry::HarnessDescriptor>,
    ),
    String,
> {
    // ADR-0046 harness resolution; mirrors `node_spawn`. A `script` node resolves
    // no harness.
    let resolved_harness = if is_script {
        None
    } else {
        // Back-compat fold, mirroring `stored_default_harness_models`: keep it here
        // so this primitive stays self-contained given its inputs.
        let mut default_models = params.default_harness_models.clone();
        if !default_models.contains_key(registry::CLAUDE) {
            if let Some(m) = params.default_model.as_deref().filter(|s| !s.is_empty()) {
                default_models.insert(registry::CLAUDE.to_string(), m.to_string());
            }
        }
        let tiers = resolver::HarnessTiers {
            node_pin: node.pin_harness.as_deref(),
            // The Run tier: the harness frozen in this Run's `RunStarted`.
            run: params.run_state.harness.as_deref(),
            // An empty string must never win a tier (the `Some("")` trap).
            project: params.project_harness.as_deref().filter(|s| !s.is_empty()),
            instance_default: params.default_harness.as_deref(),
        };
        Some(resolver::resolve(&tiers, &node.harnesses, &default_models))
    };
    let harness_descriptor = match &resolved_harness {
        None => None,
        // An unresolvable name must be a rejection that names it — never a silent
        // claude fallback.
        Some(r) => {
            let resolved = match params.harness_registry {
                Some(reg) => reg.resolve(&r.harness),
                None => registry::resolve(&r.harness),
            };
            match resolved {
                Some(d) => Some(d),
                None => {
                    return Err(format!(
                        "node '{}': unknown harness '{}'",
                        params.node_id, r.harness
                    ));
                }
            }
        }
    };
    // A missing harness binary is a spawn that cannot happen: reject, never a 2xx.
    if let Some(d) = &harness_descriptor {
        if params.tmux_cmd_override.is_none() && !binary_path::binary_available(&d.binary) {
            return Err(format!(
                "node '{}': harness '{}' binary '{}' not found in PATH {} \
                         (ADR-0055: the user's interactive PATH, not the service's) — \
                         install it or check the name; this is not \"not installed\" unless \
                         the binary is truly absent from that PATH",
                params.node_id,
                d.name,
                d.binary,
                binary_path::harness_probe_path()
            ));
        }
    }
    Ok((resolved_harness, harness_descriptor))
}

/// The staging-set env a sandboxed force-spawn carries (#708 / ADR-0063 §3).
pub(super) fn staging_set_env(
    params: &StartNodeParams<'_>,
    sandboxed: bool,
    harness: Option<&registry::HarnessDescriptor>,
) -> Vec<(String, String)> {
    match (sandboxed, params.sandbox_home_root, harness) {
        (true, Some(home), Some(d)) => {
            let sandbox_root = home.join(".pdo").join("sandbox");
            match pdo_sandbox::adapters::staging::fill_staging_set(
                home,
                &sandbox_root,
                params.run_id,
                &d.name,
                Some(params.repo_root),
            ) {
                Ok(fill) => fill.env(),
                Err(e) => {
                    tracing::warn!(
                        "force-spawn of node {}: staging `{}` set failed (best-effort): {e:#}",
                        params.node_id,
                        d.name
                    );
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    }
}
