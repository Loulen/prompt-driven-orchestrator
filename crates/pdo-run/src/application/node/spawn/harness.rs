//! The harness a node launches on, as a descriptor: resolved against the
//! floor and the disk tier, its binary present (on the host, or in the
//! sandbox image), its staging set filled into a sandboxed Run.

use pdo_harness::adapters::binary_path::{binary_available, harness_probe_path};
use pdo_harness::agent_choice;
use tracing::warn;

use super::interrupts::*;
use super::warnings::*;
use super::*;
use pdo_pipeline::domain::pipeline;

/// The descriptor of the resolved harness and the env of its staging set; a
/// failure interrupts the spawn before it starts.
pub(super) async fn prepare(
    deps: SpawnDeps<'_>,
    spawn_ctx: &SpawnContext<'_>,
    node: &pipeline::NodeDef,
    iter: i64,
    resolved_harness: Option<&agent_choice::ResolvedCombo>,
    run_sandboxed: bool,
) -> Result<
    (
        Option<pdo_harness::registry::HarnessDescriptor>,
        Vec<(String, String)>,
    ),
    SpawnOutcome,
> {
    let run_id = spawn_ctx.run_id;
    let harness_descriptor = match resolved_harness {
        None => None,
        Some(r) => {
            // #553: resolve against the embedded floor MERGED with the user's disk
            // descriptor tier (`~/.pdo/harnesses/`), so a harness declared in data
            // launches without a rebuild. The registry reads a root we hand it — it
            // never touches `$HOME` itself.
            let registry = pdo_harness::adapters::descriptor_file::load(&spawn_home_root(&deps));
            match registry.resolve(&r.harness) {
                Some(d) => {
                    // #553 / ADR-0063: a sandboxed Run whose node runs on a harness
                    // with no staging set holds only by the profile's image and
                    // its `$HOME` exceptions — say it once, visibly. A harness WITH
                    // a set gets it filled below, at this very spawn (#708).
                    if run_sandboxed {
                        warn_missing_staging_set_once(run_id, &r.harness);
                    }
                    Some(d)
                }
                None => {
                    return Err(SpawnOutcome::Failed {
                        reason: format!("node {}: unknown harness '{}'", node.id, r.harness),
                    });
                }
            }
        }
    };
    // #708 / ADR-0063 §3: the env of the resolved harness's staging set, forwarded on
    // the `docker exec` of a sandboxed node. Empty on the host path and for a harness
    // whose set declares none (`claude`).
    let mut set_env: Vec<(String, String)> = Vec::new();
    if let Some(d) = &harness_descriptor {
        if run_sandboxed {
            // ADR-0063 §5: the binary must exist IN THE CONTAINER — the host's PATH says
            // nothing about the profile's image. A `which` at spawn, never a version
            // check; absent ⇒ said once per Run, node `Interrupted` with that reason
            // (ADR-0049), nothing created. A probe that could not answer (docker itself
            // failed) is not an absence: warn and let the spawn fail on its own terms.
            let docker_bin = deps.docker_cmd_override.unwrap_or("docker").to_string();
            let (rid, binary) = (run_id.to_string(), d.binary.clone());
            let uid = pdo_sandbox::adapters::host::host_uid();
            let gid = pdo_sandbox::adapters::host::host_gid();
            let probe = tokio::task::spawn_blocking(move || {
                pdo_sandbox::adapters::local_runtime(Some(&docker_bin))
                    .binary_present(&rid, uid, gid, &binary)
            })
            .await
            .unwrap_or_else(|je| Err(anyhow::anyhow!("binary probe panicked: {je}")));
            match probe {
                Ok(true) => {}
                Ok(false) => {
                    warn_binary_missing_in_image_once(run_id, &d.name, &d.binary);
                    let reason = format!(
                        "harness `{}` binary `{}` is absent from the sandbox image — PDO does \
                         not provide the image; add `{}` to the profile's image (ADR-0063)",
                        d.name, d.binary, d.binary
                    );
                    interrupt_spawn_before_start_with_code(
                        deps,
                        spawn_ctx.repo_root,
                        run_id,
                        &node.id,
                        iter,
                        None,
                        HARNESS_BINARY_MISSING_CODE,
                        &reason,
                    )
                    .await;
                    return Err(SpawnOutcome::Failed {
                        reason: format!("node {}: {reason}", node.id),
                    });
                }
                Err(e) => {
                    warn!(
                        "run {run_id}: node {}: could not probe `{}` in the sandbox container \
                         ({e:#}); spawning anyway",
                        node.id, d.binary
                    );
                }
            }
            // ADR-0063 §3: fill the harness's staging set NOW if this Run has not yet —
            // the first node that resolves `pi` copies `.pi/agent`, the second finds
            // the marker, a claude-only Run never copies it. Trust is pre-granted on
            // `repo_root`, the common ancestor of every worktree (claude's fixup).
            let home_root = spawn_home_root(&deps);
            let sandbox_root = home_root.join(".pdo").join("sandbox");
            match pdo_sandbox::adapters::staging::fill_staging_set(
                &home_root,
                &sandbox_root,
                run_id,
                &d.name,
                Some(spawn_ctx.repo_root),
            ) {
                Ok(fill) => set_env = fill.env(),
                Err(e) => {
                    let reason = format!(
                        "node {}: failed to stage the `{}` set into the sandbox: {e:#}",
                        node.id, d.name
                    );
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
        } else if deps.tmux_cmd_override.is_none() && !binary_available(&d.binary) {
            return Err(SpawnOutcome::Failed {
                reason: format!(
                    "node {}: harness '{}' binary '{}' not found in PATH {} \
                     (ADR-0055: the user's interactive PATH, not the service's)",
                    node.id,
                    d.name,
                    d.binary,
                    harness_probe_path()
                ),
            });
        }
    }

    Ok((harness_descriptor, set_env))
}
