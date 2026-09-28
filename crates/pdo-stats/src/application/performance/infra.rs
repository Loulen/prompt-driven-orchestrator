//! The Infrastructure roles (the Pipeline Manager, a Merge resolver): their
//! session resolved by exclusion in the one directory their turns land in,
//! never guessed by path or timing alone.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::record::fold_subagents;
use super::sessions::{check_root_once, harness_display_label, subagent_groups};
use super::PerformanceError;
use crate::application::ports::SessionTranscripts;
use crate::domain::absorption::identity::NodeIdentity;
use crate::domain::performance::acc::{
    DurationReading, HarnessAcc, NodeAcc, NodeKind, INFRA_STEERING_REASON,
};

/// The exclusion list an Infrastructure role's own session is resolved against.
/// Populated for EVERY `NodeStarted`, whatever its type or outcome: even a
/// failed attempt's session file still occupies the shared directory.
#[derive(Debug, Clone)]
pub struct NodeStartRecord {
    pub node_type: String,
    /// Where the NodeRun works (#653) — its own sub-worktree, or the Run's.
    pub isolated: bool,
    pub harness: String,
    pub session_id: Option<String>,
}

/// One directory's Claude sessions resolved down to the ONE not already known
/// to belong to a Node — or why that could not be done. An ambiguous directory
/// is never attributed by path alone.
pub enum InfraSession {
    /// Exactly one unattributed `.jsonl` file: `(project_dir, session_id)`.
    One(PathBuf, String),
    /// The directory doesn't exist, or every file in it is already a known
    /// Node's own session — an ordinary, non-ambiguous absence.
    None,
    /// More than one unattributed file, or a Claude Node shares this directory
    /// with no recorded `session_id` at all (so it can't be excluded by name)
    /// — cannot be safely attributed to anything.
    Ambiguous,
}

pub fn resolve_infra_session(
    sources: &dyn SessionTranscripts,
    claude_root: &Path,
    working_dir: &Path,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
) -> InfraSession {
    if directory_ambiguous {
        return InfraSession::Ambiguous;
    }
    let Some((project_dir, sessions)) = sources.claude_sessions_in(claude_root, working_dir) else {
        return InfraSession::None;
    };
    let mut candidates: Vec<String> = sessions
        .into_iter()
        .filter(|stem| !excluded_session_ids.contains(stem))
        .collect();
    match candidates.len() {
        0 => InfraSession::None,
        1 => InfraSession::One(project_dir, candidates.remove(0)),
        _ => InfraSession::Ambiguous,
    }
}

/// Context peak + subagent transcripts for one resolved Infrastructure-role
/// session, or the motivated absence. A subagent still counts even if the
/// role's own attempt around it later failed (same as a Node's subagents).
/// `harness` is threaded through though only `claude` reaches here today — the
/// resolve-by-exclusion scan above is structurally Claude-only.
pub fn infra_claude_observation(
    sources: &dyn SessionTranscripts,
    harness: &str,
    claude_root: &Path,
    working_dir: &Path,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
) -> (Option<f64>, Option<&'static str>, Vec<(String, String)>) {
    match resolve_infra_session(
        sources,
        claude_root,
        working_dir,
        excluded_session_ids,
        directory_ambiguous,
    ) {
        InfraSession::Ambiguous => (
            None,
            Some("ambiguous session identity in a directory shared with a Node"),
            Vec::new(),
        ),
        InfraSession::None => (
            None,
            Some("no attributable session for this infrastructure role"),
            Vec::new(),
        ),
        InfraSession::One(project_dir, session_id) => {
            let path = project_dir.join(format!("{session_id}.jsonl"));
            match sources.read(&path) {
                Some(text) => {
                    let peak = pdo_harness::probes::context_peak(harness, &text);
                    let subs = subagent_groups(
                        sources,
                        harness,
                        claude_root,
                        working_dir,
                        Some(&session_id),
                    );
                    (
                        peak.map(|p| p as f64),
                        peak.is_none()
                            .then_some("no readable context usage in transcript"),
                        subs,
                    )
                }
                None => (None, Some("no attributable transcript"), Vec::new()),
            }
        }
    }
}

/// Fold one Infrastructure-role occurrence into its own role and the raw infra
/// total alike. Shared by the Pipeline Manager and Merge resolver arms, which
/// differ only in which directory and exclusion set apply.
#[allow(clippy::too_many_arguments)]
pub fn record_infra_success(
    sources: &dyn SessionTranscripts,
    acc: &mut NodeAcc,
    infra_total_by_harness: &mut BTreeMap<String, HarnessAcc>,
    run_harness: &str,
    claude_root: &Path,
    working_dir: Option<&Path>,
    excluded_session_ids: &HashSet<String>,
    directory_ambiguous: bool,
    duration: Option<f64>,
    // #810/#819 — the wall-clock, and the declared wait this role's span
    // covered: the sum of every Node wait of the Run for the Pipeline Manager,
    // nothing at all for a Merge resolver (no wait is possible on one).
    waited_millis: f64,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    let (context, reason, subs) = if run_harness != pdo_harness::registry::CLAUDE {
        (
            None,
            Some("harness has no context-usage source"),
            Vec::new(),
        )
    } else if let Some(working_dir) = working_dir {
        check_root_once(
            sources,
            claude_root,
            &harness_display_label(run_harness),
            seen_roots,
        )?;
        infra_claude_observation(
            sources,
            run_harness,
            claude_root,
            working_dir,
            excluded_session_ids,
            directory_ambiguous,
        )
    } else {
        (
            None,
            Some("no attributable session for this infrastructure role"),
            Vec::new(),
        )
    };

    let duration_reason = duration.is_none().then_some("unparseable timestamp");
    let durations = DurationReading::new(duration, waited_millis, duration_reason);
    for target in [
        acc.by_harness.entry(run_harness.to_string()).or_default(),
        infra_total_by_harness
            .entry(run_harness.to_string())
            .or_default(),
    ] {
        target.context.observe(context, reason);
        target.observe_durations(durations);
        // #792: what reaches an Infrastructure role's session is the daemon's
        // own text (review batches), never human piloting — unavailable, with
        // the reason.
        target.steering.observe(None, Some(INFRA_STEERING_REASON));
    }

    if run_harness == pdo_harness::registry::CLAUDE {
        // Infrastructure stays off the « By model » axis and its Node couples
        // (module doc): the role's residual sessions carry no model identity of
        // their own, and the axis compares Node executions.
        fold_subagents(
            &mut acc.subagents,
            None,
            None,
            None,
            &NodeIdentity::default(),
            NodeKind::default(),
            subs,
            run_harness,
        );
    }

    Ok(())
}
