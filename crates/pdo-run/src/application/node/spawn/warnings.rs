//! What a spawn says once per Run (a harness with no staging set, a binary
//! missing from the image, a harness with no turn end, a missing profile or
//! skill), and where it reads the home and the collection lap from.

use std::path::PathBuf;

use pdo_harness::agent_choice;
use tracing::warn;

use super::*;
use crate::application::node::prompt as prompt_augmenter;

/// The host home root the disk descriptor tier (#553) is read under: the per-daemon
/// override when set (the layer-3 seam), else the real `$HOME`. Mirrors
/// `run_sandbox::sandbox_home_roots` exactly, but reachable from the narrow
/// [`SpawnDeps`] bundle (which carries no `Runs`). An unresolved `$HOME`
/// degrades to an empty root, i.e. the embedded floor — never a spawn failure.
pub(super) fn spawn_home_root(deps: &SpawnDeps<'_>) -> PathBuf {
    deps.home_override
        .map(PathBuf::from)
        .or_else(|| pdo_sandbox::adapters::host::default_roots_from_env().map(|(home, _)| home))
        .unwrap_or_default()
}

/// #553 / ADR-0063: say ONCE per `(run, harness)` that a sandboxed Run's node runs
/// on a harness with no staging set. The message is the pure
/// [`pdo_harness::probes::staging_set_absence_note`] (so it is unit-tested
/// there, not against a terminal); the process-static dedup keeps a busy scheduler
/// from repeating it on every retry or collection lap. A no-op for `claude`, which
/// declares a set.
pub(super) fn warn_missing_staging_set_once(run_id: &str, harness: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let Some(note) = pdo_harness::probes::staging_set_absence_note(harness) else {
        return;
    };
    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), harness.to_string())) {
        warn!("run {run_id}: {note}");
    }
}

/// #708 / ADR-0063 §5: say ONCE per `(run, harness)` that the resolved harness's
/// binary is absent from the sandbox image. Every node of the Run that resolves
/// that harness then goes `Interrupted` with the same reason (ADR-0049), but the
/// log carries the sentence once — a retry loop must not turn one missing binary
/// into a wall of warnings. PDO does not provide the image (the profile does), so
/// the sentence tells the user what to change: their image.
pub(super) fn warn_binary_missing_in_image_once(run_id: &str, harness: &str, binary: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), harness.to_string())) {
        warn!(
            "run {run_id}: sandbox: harness `{harness}` binary `{binary}` is absent from the \
             sandbox image — PDO does not provide the image; add `{binary}` to the profile's \
             image (ADR-0063). Every node of this Run resolving `{harness}` is Interrupted"
        );
    }
}

/// The `reason_code` of a node interrupted because its harness binary is absent from
/// the sandbox image (#708, ADR-0063 §5 / ADR-0049). Distinct from `spawn_aborted`:
/// nothing broke in PDO — the image lacks the binary, and only the user can fix that.
pub const HARNESS_BINARY_MISSING_CODE: &str = "harness_binary_missing";

/// #613 / ADR-0051 (AC #7): say ONCE per `(run, harness)` that turn-end
/// auto-completion is enabled but the node's harness has no end-of-turn substrate,
/// so the setting cannot be honoured for it. The message is the pure
/// [`pdo_harness::probes::turn_end_absence_note`] (unit-tested there); the
/// process-static dedup keeps a busy scheduler from repeating it. A no-op for
/// `claude` (which has the substrate) and whenever the setting is off. Without this
/// the setting was a **silent** no-op on a substrate-less harness — the very thing
/// this ticket removes.
pub(super) fn warn_turn_end_unsupported_once(run_id: &str, harness: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let Some(note) = pdo_harness::probes::turn_end_absence_note(harness) else {
        return;
    };
    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), harness.to_string())) {
        warn!("run {run_id}: {note}");
    }
}

/// #563 (AC13/AC14): say ONCE per `(run, tier, profile_id)` that a tier named a
/// `Profile` reference absent from the atomic snapshot — the walk warned and
/// behaved as `Inherit` for that tier rather than failing the spawn. Deduped the
/// same way as [`warn_missing_staging_set_once`] so a busy scheduler replaying
/// the same stale reference doesn't spam the log every retry.
pub(super) fn warn_missing_agent_profile_once(
    run_id: &str,
    warning: &agent_choice::MissingProfileWarning,
) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String, String)>>> = Mutex::new(None);

    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    let key = (
        run_id.to_string(),
        format!("{:?}", warning.tier),
        warning.profile_id.clone(),
    );
    if said.insert(key) {
        warn!(
            "run {run_id}: agent profile '{}' referenced at tier {:?} no longer exists — \
             falling through as if that tier stated no choice",
            warning.profile_id, warning.tier
        );
    }
}

/// #669 (ADR-0062): say ONCE per `(run, skill id)` that a tier selected a skill
/// the bank no longer has — the node runs without it, the inspector and the
/// pipeline banner carry the warning, and the Run is never refused. Same dedupe
/// as [`warn_missing_agent_profile_once`].
pub(super) fn warn_missing_skill_once(
    run_id: &str,
    missing: &pdo_skill::domain::selection::MissingSkill,
) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

    let mut guard = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let said = guard.get_or_insert_with(HashSet::new);
    if said.insert((run_id.to_string(), missing.id.clone())) {
        warn!(
            "run {run_id}: skill '{}' ({}) selected at {:?} no longer exists in the bank — \
             the node runs without it",
            missing.name, missing.id, missing.tiers
        );
    }
}

/// A collection-region member (ADR-0011 / #269) reads its OWN deposited item:
/// the fan-out deposits `_item.md` under the entry's artifact dir, one per
/// lap — there is no separate driver node like the retired ForEach.
pub(super) fn find_collection_context(
    spawn_ctx: &SpawnContext<'_>,
    node_id: &str,
    iter: i64,
) -> Option<prompt_augmenter::ForEachContext> {
    crate::domain::loop_region::collection_region_for_member(spawn_ctx.pipeline, node_id)?;
    let item_path = spawn_ctx
        .artifacts_dir
        .join(node_id)
        .join(format!("iter-{iter}"))
        .join("_item.md");
    let item_content = std::fs::read_to_string(&item_path).ok()?;
    let total = std::fs::read_dir(spawn_ctx.artifacts_dir.join(node_id))
        .map(|entries| {
            entries
                .filter(|e| e.as_ref().is_ok_and(|e| e.path().is_dir()))
                .count()
        })
        .unwrap_or(0) as i64;
    let current_item = item_content
        .split("---")
        .nth(2)
        .unwrap_or("")
        .trim()
        .to_string();
    Some(prompt_augmenter::ForEachContext {
        current_item,
        current_iter: iter,
        total,
    })
}
