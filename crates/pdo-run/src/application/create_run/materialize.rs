//! A new Run on disk: its worktree cut from the source ref, provisioned, its
//! skills delivered, its secondary snapshots, its `RunStarted` appended, its
//! pipeline copied beside it and its `_input/`. Each failure before the append
//! takes back what was already made: a refused Run leaves nothing behind.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::http::StatusCode;
use pdo_workspace::adapters::git::worktrees::{
    create_secondary_snapshot, create_worktree, remove_secondary_snapshot, secondary_snapshot_path,
    worktree_dir_for_run,
};
use pdo_workspace::domain::provisioning::ProvisioningPlan;
use tracing::{error, warn};

use super::payload::Started;
use super::request::InputFile;
use super::target::{Launchable, Refusal, Target};
use crate::application::events::append_event;
use crate::application::paths::{run_scoped_pipeline_path, run_scoped_prompts_dir};
use crate::application::run_pipeline::record_launch_source;
use crate::Runs;

/// What the Run is made of on disk.
pub(super) struct Materials<'a> {
    pub target: &'a Target,
    pub launchable: &'a Launchable,
    pub provisioning_plan: &'a ProvisioningPlan,
    /// The prompt, written to `_input/output.md`.
    pub input: &'a str,
    pub attachments: &'a [InputFile],
}

/// Make the Run exist on disk and in its log. Returns its worktree.
pub(super) async fn materialize(
    state: &Runs,
    run_id: &str,
    started: &mut Started,
    materials: Materials<'_>,
) -> Result<PathBuf, Refusal> {
    let Materials {
        target,
        launchable,
        provisioning_plan,
        input,
        attachments,
    } = materials;
    // Artifacts live under <target_repo>/.pdo/runs/<run-id>/.
    let run_repo_root = &target.run_repo_root;
    let worktree_dir = worktree_dir_for_run(run_repo_root, run_id);
    let branch_name = format!("pdo/run-{run_id}");

    if let Err(e) = create_worktree(
        run_repo_root,
        &worktree_dir,
        &branch_name,
        &target.source_ref,
    ) {
        error!("failed to create worktree: {e}");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": format!("worktree creation failed: {e}") }),
        ));
    }

    if let Err(e) = pdo_workspace::adapters::fs::provisioner::provision_missing(
        run_repo_root,
        &worktree_dir,
        provisioning_plan,
    ) {
        reap_orphan_run_worktree(run_repo_root, &worktree_dir, &branch_name);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "error": format!(
                    "Run not created. Provisioning failed: {e:#}. No node was spawned."
                )
            }),
        ));
    }

    // #672 / ADR-0062: snapshot the frozen selection's content under the Run dir
    // (outside the worktree), then deliver it into the Run worktree — the tree the
    // non-isolated nodes, the Pipeline Manager and the run shell work in. Paths are
    // excluded per skill in the target repo's `info/exclude` and recorded in the
    // worktree's provisioning manifest, so no skill ever reaches the Run branch. A
    // versioned homonym is skipped and reported on the Run; an I/O failure refuses
    // the Run like a provisioning failure (never a partial environment).
    let skill_delivery =
        state
            .skills
            .deliver(&worktree_dir, run_repo_root, run_id, &started.skills);
    match skill_delivery {
        Ok(skipped) => {
            for skipped in &skipped {
                warn!(
                    "run {run_id}: skill '{}' not delivered: {}",
                    skipped.name, skipped.reason
                );
            }
            if !skipped.is_empty() {
                if let Some(payload) = started.event.payload.as_mut() {
                    payload["skipped_skills"] = serde_json::to_value(&skipped).unwrap_or_default();
                }
            }
        }
        Err(e) => {
            let _ = state.skills.withdraw(run_repo_root, run_id);
            reap_orphan_run_worktree(run_repo_root, &worktree_dir, &branch_name);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!(
                        "Run not created. Skill delivery failed: {e:#}. No node was spawned."
                    )
                }),
            ));
        }
    }

    // Materialise each secondary as a detached, pinned snapshot beside
    // `worktree/`/`nodes/`. At Run start, NOT per-node spawn, so the reap surface
    // stays minimal; under `run_repo_root`, so the sandbox sees them at an identical
    // path with no new mount. A failure fails the Run loud before `RunStarted`;
    // otherwise a missing snapshot would silently starve every node reading it.
    let mut created_secondary_snapshots: Vec<(String, PathBuf)> = Vec::new();
    for pin in &target.secondary_pins {
        let dest = secondary_snapshot_path(run_repo_root, run_id, &pin.alias);
        if let Err(e) = create_secondary_snapshot(Path::new(&pin.repo), &dest, &pin.sha) {
            error!("failed to create secondary snapshot for {}: {e}", pin.repo);
            for (repo, snapshot) in &created_secondary_snapshots {
                remove_secondary_snapshot(Path::new(repo), snapshot);
            }
            reap_orphan_run_worktree(run_repo_root, &worktree_dir, &branch_name);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!("secondary snapshot creation failed for {}: {e}", pin.repo)
                }),
            ));
        }
        created_secondary_snapshots.push((pin.repo.clone(), dest));
    }

    if let Err(e) = append_event(state, &started.event).await {
        error!("failed to append run_started: {e}");
        for (repo, snapshot) in &created_secondary_snapshots {
            remove_secondary_snapshot(Path::new(repo), snapshot);
        }
        reap_orphan_run_worktree(run_repo_root, &worktree_dir, &branch_name);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "event log error" }),
        ));
    }

    if let Err(e) = copy_pipeline_to_run(run_repo_root, &launchable.pipeline_path, run_id) {
        error!("failed to copy pipeline to run dir: {e}");
    } else {
        // ADR-0085: remember which shared pipeline the Run came from and what
        // it looked like, so the overwrite warning can say whether it changed.
        record_launch_source(run_repo_root, run_id, &launchable.pipeline_path);
        if *run_repo_root == state.repo_root {
            // Run dirs are watched individually and non-recursively, so each new one must
            // be registered. Runs in another target repo stay unwatched — the watcher's
            // run-id detection is rooted at `state.repo_root`.
            if let Some(run_dir) = run_scoped_pipeline_path(run_repo_root, run_id).parent() {
                state.run_dirs.watch(run_dir);
            }
        }
    }

    // Directory-based artifact (ADR-0010).
    let artifacts_dir = worktree_dir.join(".pdo").join("artifacts");
    let input_dir = artifacts_dir.join("_input");
    if let Err(e) = std::fs::create_dir_all(&input_dir) {
        error!("failed to create _input artifact dir: {e}");
    }
    let input_path = input_dir.join("output.md");
    if let Err(e) = std::fs::write(&input_path, input) {
        error!("failed to write _input/output.md: {e}");
    }

    for attachment in attachments {
        let path = input_dir.join(&attachment.filename);
        if let Err(e) = std::fs::write(&path, &attachment.data) {
            error!("failed to write attachment {}: {e}", attachment.filename);
        }
    }
    Ok(worktree_dir)
}

pub(crate) fn copy_pipeline_to_run(
    repo_root: &std::path::Path,
    pipeline_path: &std::path::Path,
    run_id: &str,
) -> Result<()> {
    let dest_yaml = run_scoped_pipeline_path(repo_root, run_id);
    if let Some(parent) = dest_yaml.parent() {
        std::fs::create_dir_all(parent).context("create run dir")?;
    }
    std::fs::copy(pipeline_path, &dest_yaml).context("copy pipeline yaml")?;

    let stem = pipeline_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pipeline");
    let src_prompts = pipeline_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(format!("{stem}.prompts"));
    // Always create the prompts dir, even when the template has none, so the file
    // watcher can attach at run creation: it watches non-recursively and there is no
    // later hook to pick the dir up.
    let dest_prompts = run_scoped_prompts_dir(repo_root, run_id);
    std::fs::create_dir_all(&dest_prompts).context("create prompts dir")?;
    if src_prompts.is_dir() {
        for entry in std::fs::read_dir(&src_prompts)
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = entry.path();
            if p.is_file() {
                if let Some(name) = p.file_name() {
                    std::fs::copy(&p, dest_prompts.join(name))?;
                }
            }
        }
    }
    Ok(())
}

/// Reap the worktree of a Run aborted at create, and what it left beside it.
///
/// #672: a Run aborted at create may already have snapshotted skills beside the
/// worktree and excluded their paths in the repo's `info/exclude`; take both back
/// before the (then empty) Run directory goes.
pub fn reap_orphan_run_worktree(repo_root: &Path, worktree_dir: &Path, branch: &str) {
    pdo_workspace::adapters::git::worktrees::reap_orphan_sub_worktree(
        repo_root,
        worktree_dir,
        branch,
    );
    if let Some(run_id) = branch.strip_prefix("pdo/run-") {
        let _ = pdo_skill::adapters::delivery::remove_exclusions(repo_root, run_id);
        let _ = std::fs::remove_dir_all(pdo_skill::domain::delivery::snapshot_root(
            repo_root, run_id,
        ));
    }
    if let Some(run_dir) = worktree_dir.parent() {
        let _ = std::fs::remove_dir(run_dir);
    }
}
