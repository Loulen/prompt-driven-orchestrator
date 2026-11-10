//! Where a NodeRun works: its sub-worktree (ensured, reused in place on a
//! re-spawn, provisioned when cut) or the Run's worktree, and the skills
//! delivered into it.

use std::path::PathBuf;

use tracing::warn;

use super::interrupts::*;
use super::*;
use crate::domain::merge_action;
use pdo_pipeline::domain::pipeline;
use pdo_workspace::adapters::git::worktrees::{
    ensure_sub_worktree, sub_worktree_branch, sub_worktree_path,
};

use super::guards::Loaded;

/// The tree a NodeRun works in, and what its preparation found.
pub(super) struct Worktree {
    pub(super) working_dir: PathBuf,
    pub(super) has_sub_worktree: bool,
    /// What this spawn created, reaped should it abort (#279). `None` on a reuse.
    pub(super) orphan_to_reap: Option<(PathBuf, String)>,
    pub(super) spawn_base_sha: Option<String>,
    pub(super) reused_sub_worktree: bool,
    pub(super) interrupted_git_ops: Vec<String>,
    pub(super) node_provisioning: pdo_workspace::domain::provisioning::ProvisioningRules,
    pub(super) node_provisioning_plan:
        Option<pdo_workspace::domain::provisioning::ProvisioningPlan>,
}

pub(super) async fn prepare(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
    loaded: &Loaded,
) -> Result<Worktree, SpawnOutcome> {
    let run_id = spawn_ctx.run_id;
    let projected = loaded.as_ref().map(|(_, s)| s);
    // #653 / ADR-0060: where this NodeRun works. The FROZEN value wins over the
    // document — a re-spawn of the same iteration (restart / invalidate) must
    // land back in the directory the interrupted one left, even if the graph was
    // edited in between (ADR-0007). Only a first spawn reads the node's own
    // `isolated_worktree`, and that reading is what gets frozen below.
    let has_sub_worktree = loaded
        .as_ref()
        .and_then(|(events, _)| merge_action::frozen_isolation(events, &node.id, iter))
        .unwrap_or_else(|| node.is_isolated());

    // Track the sub-worktree + branch this spawn creates so an abort in the
    // panic-isolated span below can reap them (#279). `None` for nodes that own
    // no worktree (a non-isolated agent/script, control nodes).
    let mut orphan_to_reap: Option<(PathBuf, String)> = None;
    // #503 / ADR-0036: the commit the sub-worktree is cut from, recorded on
    // `NodeStarted`. It is the sole basis on which a later merge-back conflict may
    // be resolved in the node's favour — no base recorded, no resolution.
    let mut spawn_base_sha: Option<String> = None;
    let mut reused_sub_worktree = false;
    // #516: every interrupted git op left in a reused sub-worktree, in scan order.
    // Routed to both the re-spawned node's preamble and the wire response.
    let mut interrupted_git_ops: Vec<String> = Vec::new();
    let node_provisioning = if has_sub_worktree {
        let frozen = loaded.as_ref().and_then(|(events, _)| {
            crate::application::node::launch::frozen_node_rules(events, &node.id, iter)
        });
        match frozen {
            Some(rules) => rules,
            None => match crate::application::node::launch::node_rules_from_pipeline(
                spawn_ctx.pipeline_path,
                &node.id,
            ) {
                Ok(rules) => rules,
                Err(e) => {
                    return Err(SpawnOutcome::Failed {
                        reason: format!(
                            "provisioning failed for {}: {e:#}; no node was spawned",
                            node.id
                        ),
                    });
                }
            },
        }
    } else {
        pdo_workspace::domain::provisioning::ProvisioningRules::default()
    };
    let mut node_provisioning_plan = loaded.as_ref().and_then(|(events, _)| {
        crate::application::node::launch::frozen_node_plan(events, &node.id, iter)
    });
    let working_dir = if has_sub_worktree {
        let sub_wt_dir = sub_worktree_path(spawn_ctx.repo_root, run_id, &node.id, iter);
        let sub_branch = sub_worktree_branch(run_id, &node.id, iter);
        let pipeline_branch = format!("pdo/run-{run_id}");

        // #503 / ADR-0036 under reuse (ADR-0037 §6): a reuse does not cut anything,
        // so it carries the ORIGINAL base forward rather than deriving a new one.
        let previous_base_sha = loaded
            .as_ref()
            .and_then(|(events, _)| merge_action::spawn_base_sha(events, &node.id, iter));

        // #489-B: `ensure_sub_worktree`, not `create_sub_worktree`. The bare create
        // replayed `git worktree add -b <branch>` on a branch that already existed
        // and failed with exit 255 on EVERY re-spawn of the same iteration — i.e. on
        // every `restart_node` of an isolated node, 100% of the time.
        match ensure_sub_worktree(
            spawn_ctx.repo_root,
            &sub_wt_dir,
            &sub_branch,
            &pipeline_branch,
            previous_base_sha.as_deref(),
        ) {
            Ok(ensured) => {
                spawn_base_sha = ensured.base_sha;
                reused_sub_worktree = !ensured.created;
                interrupted_git_ops = ensured.entry_state.interrupted_git_ops().to_vec();
                if ensured.created {
                    let inherited = projected
                        .map(|state| state.provisioning_rules.as_slice())
                        .unwrap_or_default();
                    let provisioned =
                        pdo_workspace::adapters::fs::provisioner::provision_node_worktree(
                            spawn_ctx.repo_root,
                            &sub_wt_dir,
                            inherited,
                            &node_provisioning,
                            &pipeline_branch,
                        );
                    match provisioned {
                        Ok(plan) => node_provisioning_plan = Some(plan),
                        Err(e) => {
                            let reason = format!(
                                "provisioning failed for {} in copy/link phase: {e:#}; no node was spawned",
                                node.id
                            );
                            let orphan = (sub_wt_dir.clone(), sub_branch.clone());
                            interrupt_spawn_before_start(
                                deps,
                                spawn_ctx.repo_root,
                                run_id,
                                &node.id,
                                iter,
                                Some(&orphan),
                                &reason,
                            )
                            .await;
                            return Err(SpawnOutcome::Failed { reason });
                        }
                    }
                }
                // #489-B: `Some(...)` ONLY when this spawn created the worktree.
                // On a reuse, any later abort in the panic-isolated span would send
                // `interrupt_spawn_before_start` into `reap_orphan_sub_worktree`, and
                // `worktree remove --force` succeeds on a dirty tree — it would
                // destroy exactly the work the restart exists to save. Gated, the
                // abort path appends `NodeInterrupted` and destroys nothing: the Run
                // parks `AwaitingUser` with the work intact (ADR-0049), a reopen/retry
                // re-drives it and the next classification answers `Reusable`. #279's
                // invariant ("an aborted spawn leaves no orphan") is untouched — it
                // only ever covered what the spawn itself created.
                if ensured.created {
                    orphan_to_reap = Some((sub_wt_dir.clone(), sub_branch));
                }
            }
            Err(e) => {
                // #498 / ADR-0050 §1: a surviving `pdo/sub-*` branch (or another
                // git collision) made `worktree add -b` fail. Before résilience
                // this only `error!`-logged and returned `Failed` — no event, so
                // the run stayed frozen `running` and only journalctl knew why.
                // Now it names the node in a `NodeInterrupted`, parking the run
                // `AwaitingUser`; on the next reopen/retry `ensure_sub_worktree`
                // reaps the survivor (Recyclable) and the spawn succeeds (FP #3).
                let reason = format!("failed to ensure sub-worktree for {}: {e:#}", node.id);
                interrupt_spawn_before_start(
                    deps,
                    spawn_ctx.repo_root,
                    run_id,
                    &node.id,
                    iter,
                    None,
                    &reason,
                )
                .await;
                return Err(SpawnOutcome::Failed { reason });
            }
        }
        sub_wt_dir
    } else {
        spawn_ctx.worktree_dir.to_path_buf()
    };

    Ok(Worktree {
        working_dir,
        has_sub_worktree,
        orphan_to_reap,
        spawn_base_sha,
        reused_sub_worktree,
        interrupted_git_ops,
        node_provisioning,
        node_provisioning_plan,
    })
}

pub(super) async fn deliver_skills(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
    working_dir: &std::path::Path,
    resolved_skills: Option<&pdo_skill::domain::selection::ResolvedSkills>,
    orphan_to_reap: Option<&(PathBuf, String)>,
) -> Result<Vec<pdo_skill::domain::delivery::SkippedSkill>, SpawnOutcome> {
    let run_id = spawn_ctx.run_id;
    // #672 / ADR-0062: deliver the skills actifs into the tree this NodeRun works
    // in — its own sub-worktree, or the shared Run worktree (where the Run's frozen
    // base already sits since create; only what is new is written). The content
    // comes from the Run snapshot, extended here (additively) with what this node's
    // own tier adds; `.agents/skills/<name>` + `.claude/skills/<name>` are excluded
    // per skill in the target repo's `info/exclude` and recorded for the completion
    // commit's filter. A versioned homonym is skipped and recorded on `NodeStarted`
    // (visible on the node); an I/O failure refuses the spawn like provisioning.
    let mut skipped_skills: Vec<pdo_skill::domain::delivery::SkippedSkill> = Vec::new();
    if let Some(resolved) = resolved_skills {
        let delivery =
            deps.skills
                .deliver(working_dir, spawn_ctx.repo_root, run_id, &resolved.skills);
        match delivery {
            Ok(skipped) => {
                for s in &skipped {
                    warn!(
                        "run {run_id} node {}: skill '{}' not delivered: {}",
                        node.id, s.name, s.reason
                    );
                }
                skipped_skills = skipped;
            }
            Err(e) => {
                let reason = format!(
                    "skill delivery failed for {}: {e:#}; no node was spawned",
                    node.id
                );
                interrupt_spawn_before_start(
                    deps,
                    spawn_ctx.repo_root,
                    run_id,
                    &node.id,
                    iter,
                    orphan_to_reap,
                    &reason,
                )
                .await;
                return Err(SpawnOutcome::Failed { reason });
            }
        }
    }

    Ok(skipped_skills)
}
