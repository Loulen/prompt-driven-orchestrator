//! What one session's transcript says, read through [`SessionTranscripts`]:
//! a main session's context peak, its steering count and its model × effort
//! identities, its subagents.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pdo_sandbox::adapters::staging::stores::HarnessStores;

use super::PerformanceError;
use crate::application::ports::SessionTranscripts;
use crate::domain::performance::model_axis::ModelIdentity;
use crate::domain::performance::node_defs::claude_subagent_group;

/// Confirm a harness's whole transcript source is listable, or name it in a
/// visible error. `NotFound` is not an error — it means "no session written
/// yet". Checked once per distinct root, never per session: a single unreadable
/// session must degrade to a local absence, not fail the whole request.
pub fn ensure_source_readable(
    files: &dyn SessionTranscripts,
    root: &Path,
    harness_label: &str,
) -> Result<(), PerformanceError> {
    files.source_listable(root).map_err(|error| {
        PerformanceError::SourceUnreadable(format!(
            "{harness_label} transcript source at {} is unreadable: {error}",
            root.display()
        ))
    })
}

/// Pick the store root a harness's transcript is read from: the per-Run,
/// sandbox-aware Claude root, or one of the host-home stores (`copilot`'s journal
/// store, `pi`'s sessions store, #707). Picks between independently computed
/// roots, not between behaviours — behaviour dispatch goes through
/// `harness_probes` (ADR-0051); the name match lives once, in
/// [`HarnessStores::root_for`].
pub fn source_root<'a>(
    harness: &str,
    claude_root: &'a Path,
    stores: &'a HarnessStores,
) -> &'a Path {
    stores.root_for(harness, claude_root)
}

/// One main session's observation: `(context peak, absence reason, file text)`.
pub type MainObservation = (Option<f64>, Option<&'static str>, Option<String>);

/// One main session's Steering reading (#792): `Some(n)` typed human turns after
/// the launch prompt, or the named reason the count is unavailable — a harness
/// with no steering source (`opencode`: its session is not resolvable by
/// identity), no session identity, no transcript, or a transcript with no
/// readable human turn. Never `0` for an unreadable one. Reads the text the
/// context peak already loaded when there is one, so the file is read once.
pub fn main_session_steering(
    files: &dyn SessionTranscripts,
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    already_read: Option<&str>,
) -> (Option<f64>, Option<String>) {
    if !pdo_harness::probes::can_count_steering(harness) {
        return (None, Some(format!("session not resolvable on {harness}")));
    }
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return (None, Some("no session identity".to_string()));
    };
    let text = match already_read {
        Some(text) => Some(text.to_string()),
        None => session_file_text(files, harness, root, working_dir, Some(session_id)),
    };
    let Some(text) = text else {
        return (None, Some("no attributable transcript".to_string()));
    };
    match pdo_harness::probes::steering_count(harness, &text) {
        Some(count) => (Some(f64::from(count)), None),
        None => (
            None,
            Some("no readable human turn in transcript".to_string()),
        ),
    }
}

/// One harness's context peak for one main session, or the absence reason,
/// plus the session file's text — the same read feeds the peak and the
/// observed model identity (#737), so the file is read once. The root
/// readability check sits *after* the `can_measure_context` gate, so a
/// harness with no context source (e.g. `opencode`) never triggers a
/// Claude/Copilot root check it does not need.
pub fn main_session_observation(
    files: &dyn SessionTranscripts,
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    seen_roots: &mut HashSet<PathBuf>,
) -> Result<MainObservation, PerformanceError> {
    if !pdo_harness::probes::can_measure_context(harness) {
        return Ok((None, Some("harness has no context-usage source"), None));
    }
    check_root_once(files, root, &harness_display_label(harness), seen_roots)?;
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Ok((None, Some("no session identity"), None));
    };
    Ok(
        match files.main_session(harness, root, working_dir, Some(session_id)) {
            Some(text) => match pdo_harness::probes::context_peak(harness, &text) {
                Some(peak) => (Some(peak as f64), None, Some(text)),
                None => (
                    None,
                    Some("no readable context usage in transcript"),
                    Some(text),
                ),
            },
            None => (None, Some("no attributable transcript"), None),
        },
    )
}

/// One session file's transcript text, by the harness's own resolution — the
/// identity-only read for a harness whose context gate answered before the
/// file was read (`copilot`'s journal is still worth reading for its observed
/// model, #737).
pub fn session_file_text(
    files: &dyn SessionTranscripts,
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Option<String> {
    files.main_session(harness, root, working_dir, session_id)
}

/// The model × effort identities one MAIN session's observation is attributed
/// to (ADR-0065 §1): the observed identities the harness's source reports when
/// it speaks — `effort_observed` only where the source carries the effort
/// (`pi`, `copilot`); `claude`'s effort stays the requested one. A mute source
/// (or no readable file) falls back to the startup event's requested identity,
/// marked `requested`; a mute source with **no** requested model invents
/// nothing and yields no bucket.
#[allow(clippy::too_many_arguments)]
pub fn main_session_identities(
    files: &dyn SessionTranscripts,
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
    requested_model: Option<&str>,
    requested_effort: Option<&str>,
    already_read: Option<&str>,
) -> Vec<ModelIdentity> {
    let source = pdo_harness::probes::observed_identity_source(harness);
    let mut observed = Vec::new();
    if source.is_some() {
        let text = match already_read {
            Some(text) => Some(text.to_string()),
            None => session_file_text(files, harness, root, working_dir, session_id),
        };
        if let Some(text) = text {
            observed = pdo_harness::probes::observed_identities(harness, &text)
                .into_iter()
                .map(|identity| {
                    // The source carries the effort (`pi`, `copilot`) — an
                    // absent one is « not set », never refilled from the
                    // startup event (source-first, ADR-0065 §1). `claude`'s
                    // source never writes one: the requested effort stands.
                    let (effort, effort_observed) = match source {
                        Some(pdo_harness::probes::ObservedIdentitySource::ModelAndEffort) => {
                            let observed_effort = identity.effort;
                            let observed = observed_effort.clone();
                            (observed_effort, Some(observed.is_some()))
                        }
                        _ => (
                            requested_effort.map(str::to_string),
                            requested_effort.map(|_| false),
                        ),
                    };
                    ModelIdentity {
                        model: identity.model,
                        effort,
                        model_observed: true,
                        effort_observed,
                    }
                })
                .collect();
        }
    }
    if !observed.is_empty() {
        return observed;
    }
    requested_model
        .map(|model| {
            vec![ModelIdentity {
                model: model.to_string(),
                effort: requested_effort.map(str::to_string),
                model_observed: false,
                effort_observed: requested_effort.map(|_| false),
            }]
        })
        .unwrap_or_default()
}

/// The subagent transcript files discovered for one main session, grouped by
/// declared label. A harness with no nested-subagent convention (every one but
/// `claude` today) answers an empty `Vec` from the `harness_probes` dispatch
/// itself — never from an `if harness == ..` written here (ADR-0051).
pub fn subagent_groups(
    files: &dyn SessionTranscripts,
    harness: &str,
    root: &Path,
    working_dir: &Path,
    session_id: Option<&str>,
) -> Vec<(String, String)> {
    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    files
        .subagents(harness, root, working_dir, session_id)
        .into_iter()
        .map(|(stem, text)| (claude_subagent_group(&stem), text))
        .collect()
}

/// Display prose, not a capability decision — so no `harness_probes` dispatch.
pub fn harness_display_label(harness: &str) -> String {
    let mut chars = harness.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn check_root_once(
    files: &dyn SessionTranscripts,
    root: &Path,
    label: &str,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), PerformanceError> {
    if seen.insert(root.to_path_buf()) {
        ensure_source_readable(files, root, label)?;
    }
    Ok(())
}
