//! Record one successful Node execution: its main session into its Node, its
//! Pipeline and the cohort total, the « By model » axis, and its subagents.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use pdo_sandbox::adapters::staging::stores::HarnessStores;

use super::sessions::{
    main_session_identities, main_session_observation, main_session_steering, source_root,
    subagent_groups,
};
use super::PerformanceError;
use crate::application::ports::SessionTranscripts;
use crate::domain::absorption::identity::{NodeIdentity, RunIdentity};
use crate::domain::performance::acc::{
    DurationReading, HarnessAcc, NodeAcc, NodeKind, Reading, SubagentAcc, SUBAGENT_STEERING_REASON,
};
use crate::domain::performance::model_axis::{
    record_model_observation, ModelAxisModelAcc, ModelEffortKey, ModelIdentity, ModelPairAcc,
};
use crate::domain::performance::waits::duration_millis;

pub struct PendingNode {
    pub started_at: String,
    pub harness: String,
    pub session_id: Option<String>,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    pub isolated: bool,
    /// The genre this attempt was launched with (#810), frozen in `NodeStarted`
    /// (ADR-0007) with the Run's snapshot as the fallback.
    pub kind: NodeKind,
    /// The requested model × effort, frozen in the startup event (ADR-0046) —
    /// the fallback when the source is mute (ADR-0065 §1).
    pub requested_model: Option<String>,
    pub requested_effort: Option<String>,
}

/// A `MergeResolverStarted` awaiting its pairing `MergeResolverCompleted`.
/// `conflicting_node_id`/`iter` resolve the working directory an attributed
/// session is looked for in (module doc, "Infrastructure subagents").
pub struct MergeResolverPending {
    pub started_at: String,
    pub conflicting_node_id: Option<String>,
    pub iter: Option<i64>,
}

/// Fold discovered subagent transcripts into their declared groups (the
/// `by_pipeline` tree) and into the « By model » axis. Shared by a Node's
/// subagents and an Infrastructure role's.
///
/// The duration span alone bypasses the `harness_probes` dispatch: no capability
/// marker exists yet for "start/end bounds from a transcript", so
/// [`pdo_harness::context_peak::claude_transcript_time_span`] stays a direct call —
/// the seam to extend when a second harness grows subagent transcripts.
///
/// `node_pairs` / `model_axis` are `Some` only for a Node's own subagents —
/// the `by_pipeline` drill's couples and the « By model » axis are Node
/// executions; an Infrastructure role's residual subagents stay off both
/// (module doc, « The « By model » axis »).
#[allow(clippy::too_many_arguments)]
pub fn fold_subagents(
    subagents: &mut BTreeMap<String, SubagentAcc>,
    mut node_pairs: Option<&mut BTreeMap<ModelEffortKey, ModelPairAcc>>,
    mut model_axis: Option<&mut BTreeMap<String, ModelAxisModelAcc>>,
    pipeline: Option<&RunIdentity>,
    node: &NodeIdentity,
    node_kind: NodeKind,
    files: Vec<(String, String)>,
    harness: &str,
) {
    for (group, text) in files {
        let sub = subagents.entry(group).or_default();
        let sub_acc = sub.by_harness.entry(harness.to_string()).or_default();
        let peak = pdo_harness::probes::context_peak(harness, &text);
        sub_acc.context.observe(
            peak.map(|p| p as f64),
            (peak.is_none()).then_some("no readable context usage in transcript"),
        );
        let span = pdo_harness::context_peak::claude_transcript_time_span(&text);
        let sub_duration = span.and_then(|(a, b)| duration_millis(&a, &b));
        let sub_duration_reason = sub_duration
            .is_none()
            .then_some("no reliable start/end bounds in subagent transcript");
        // #810/#819: nobody declares a wait inside a subagent — its active
        // duration IS its duration and its Waiting is a measured `0`, so the
        // mode switch leaves a subagent row at the same coverage.
        let sub_durations = DurationReading::unwaited(sub_duration, sub_duration_reason);
        sub_acc.observe_durations(sub_durations);
        // #792: nobody types in a subagent — a declared absence, no expected
        // observation, so a Node's steered rate is never diluted by its
        // subagents.
        sub_acc.steering.declare_absent(SUBAGENT_STEERING_REASON);

        // ADR-0065 §3: the file's peak (and span) follows the file's own model —
        // a subagent on another model lands in its own bucket. A subagent has no
        // startup event, so ONLY the observed identities count: a mute file
        // invents nothing (never a "default of X" bucket).
        if let (Some(node_pairs), Some(model_axis), Some(pipeline)) = (
            node_pairs.as_deref_mut(),
            model_axis.as_deref_mut(),
            pipeline,
        ) {
            let source = pdo_harness::probes::observed_identity_source(harness);
            if source.is_some() {
                for identity in pdo_harness::probes::observed_identities(harness, &text) {
                    // Same source-first rule as a main session (ADR-0065 §1):
                    // `pi`/`copilot` subagents would carry their own observed
                    // effort; a mute effort is « not set », never requested.
                    let (effort, effort_observed) = match source {
                        Some(pdo_harness::probes::ObservedIdentitySource::ModelAndEffort) => {
                            let observed_effort = identity.effort;
                            let observed = observed_effort.clone();
                            (observed_effort, Some(observed.is_some()))
                        }
                        _ => (None, None),
                    };
                    let model_identity = ModelIdentity {
                        model: identity.model,
                        effort,
                        model_observed: true,
                        effort_observed,
                    };
                    record_model_observation(
                        model_axis,
                        node_pairs,
                        pipeline,
                        node,
                        node_kind,
                        harness,
                        &model_identity,
                        (
                            peak.map(|p| p as f64),
                            (peak.is_none()).then_some("no readable context usage in transcript"),
                        ),
                        sub_durations,
                        Reading::NotApplicable(SUBAGENT_STEERING_REASON),
                    );
                }
            }
        }
    }
}

/// Record one successful Node execution into its Node, its Pipeline and the
/// cohort total alike, plus the « By model » axis (#737): the main session's
/// peak + duration goes to the model × effort bucket its own session file
/// names (observed from the source, the startup event's requested identity in
/// fallback). Discovered subagent transcripts go into the Node's own subagent
/// groups and NEVER into any of the three harness accumulators.
#[allow(clippy::too_many_arguments)]
pub fn record_node_success(
    sources: &dyn SessionTranscripts,
    node_acc: &mut NodeAcc,
    pipeline_by_harness: &mut BTreeMap<String, HarnessAcc>,
    total_by_harness: &mut BTreeMap<String, HarnessAcc>,
    model_axis: &mut BTreeMap<String, ModelAxisModelAcc>,
    pipeline: &RunIdentity,
    node: &NodeIdentity,
    claude_root: &Path,
    stores: &HarnessStores,
    working_dir: &Path,
    pending: &PendingNode,
    completed_at: &str,
    // The declared wait this execution accumulated (#810), in milliseconds.
    waited_millis: f64,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    let harness = pending.harness.clone();
    let root = source_root(&harness, claude_root, stores);
    let (context, reason, main_text) = main_session_observation(
        sources,
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        seen_roots,
    )?;
    let duration = duration_millis(&pending.started_at, completed_at);
    let duration_reason = duration.is_none().then_some("unparseable timestamp");
    // #810/#819: the same observation read three ways — wall-clock, declared
    // wait subtracted, declared wait alone. All three absent for exactly the
    // same reason the wall-clock is, so the client's mode switch never changes
    // `n=`.
    let durations = DurationReading::new(duration, waited_millis, duration_reason);
    let (steering, steering_reason) = main_session_steering(
        sources,
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        main_text.as_deref(),
    );
    let steering_reading = Reading::Observed(steering, steering_reason.as_deref());

    for acc in [
        node_acc.by_harness.entry(harness.clone()).or_default(),
        pipeline_by_harness.entry(harness.clone()).or_default(),
        total_by_harness.entry(harness.clone()).or_default(),
    ] {
        acc.context.observe(context, reason);
        acc.observe_durations(durations);
        acc.steering.record(steering_reading);
    }

    // ADR-0065 §1: the main session's observation is attributed to the model(s)
    // its own session file names — observed first, the requested identity in
    // fallback, nothing invented when neither speaks.
    for identity in main_session_identities(
        sources,
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
        pending.requested_model.as_deref(),
        pending.requested_effort.as_deref(),
        main_text.as_deref(),
    ) {
        record_model_observation(
            model_axis,
            &mut node_acc.pairs,
            pipeline,
            node,
            pending.kind,
            &harness,
            &identity,
            (context, reason),
            durations,
            steering_reading,
        );
    }

    let files = subagent_groups(
        sources,
        &harness,
        root,
        working_dir,
        pending.session_id.as_deref(),
    );
    fold_subagents(
        &mut node_acc.subagents,
        Some(&mut node_acc.pairs),
        Some(model_axis),
        Some(pipeline),
        node,
        pending.kind,
        files,
        &harness,
    );

    Ok(())
}
