//! A node's pane: what the UI shows of a node session, live, resumed, or from
//! the snapshot persisted when its iteration ended (#205).

use std::path::{Path, PathBuf};

/// Where a node iteration stands, as the Run's projection says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneTarget {
    /// The iteration asked for is the node's latest.
    pub latest_iter: bool,
    /// That iteration reached a terminal status (completed, skipped, failed,
    /// stopped, stale).
    pub terminal: bool,
    /// The node is still `pending`: it never had a session.
    pub pending: bool,
    /// The node is `Interrupted` (ADR-0049): its session died with an infra
    /// incident and only a human gesture (`recover_node`, a restart) brings it
    /// back. Reading its pane is not that gesture.
    pub interrupted: bool,
    /// The repository the Run's files live under.
    pub repo_root: PathBuf,
}

/// The outcome of an in-place session **re-attach** (ADR-0049 §3a): resume the SAME
/// agent session in the node's existing sub-worktree on the harness's resume tail,
/// without re-driving the run. Shared by the pane and `recover_node`: the two
/// renderings differ, but the resume mechanics — script exclusion, the
/// `can_resume()` gate, the admission reservation, the resurrection `NodeStarted`
/// trace — must not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReattachOutcome {
    /// A `script` node is deterministic bash, never an LLM session — not
    /// re-attachable (ADR-0017).
    ScriptNode,
    /// The node's working directory is gone; nothing to re-enter.
    WorkingDirMissing,
    /// The frozen harness declares no resume tail (`can_resume() == false`,
    /// ADR-0045). The optimal path is unavailable — the caller falls back
    /// (`recover_node` → restart-with-artifacts, AC #9 / the pane → snapshot).
    CannotResume,
    /// The harness FROZEN at spawn no longer resolves. PDO **refuses** rather than
    /// relaunch `claude` in this node's worktree: a silent fallback would run a
    /// different agent than the node was started on, corrupting its transcript and
    /// its cost attribution.
    FrozenHarnessGone { harness: String },
    /// The session cap refused the re-attach — a re-attach IS a (re)spawn.
    CapReached { live: usize, cap: usize },
    /// The `NodeStarted` resurrection trace was refused (e.g. a terminal Run) — do
    /// NOT launch a session the event log has no room for.
    TraceRefused { error: String },
    /// The resume launch itself failed.
    ResumeFailed { error: String },
    /// The session was re-launched on its resume tail. The caller captures/reports.
    Resumed,
}

/// Provenance of a pane's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSource {
    /// Captured from the live session.
    Live,
    /// A dead latest-iteration session was re-attached, then captured.
    Resumed,
    /// The persisted post-mortem pane.
    Snapshot,
    /// Nothing to show: the content is a sentence saying why.
    Unavailable,
}

impl PaneSource {
    pub fn as_str(self) -> &'static str {
        match self {
            PaneSource::Live => "live",
            PaneSource::Resumed => "resumed",
            PaneSource::Snapshot => "snapshot",
            PaneSource::Unavailable => "unavailable",
        }
    }
}

/// What the pane shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub content: String,
    pub resumed: bool,
    /// The iteration shown is not the node's latest.
    pub stale: bool,
    pub source: PaneSource,
}

impl Pane {
    pub fn new(content: impl Into<String>, source: PaneSource, stale: bool) -> Self {
        Self {
            content: content.into(),
            resumed: source == PaneSource::Resumed,
            stale,
            source,
        }
    }
}

/// Where the pane of a terminal iteration is persisted: in the node's dir,
/// NOT inside the per-iter sub-worktree, so it survives the worktree's removal
/// (it is the post-mortem pane of a reaped terminal node, #205).
pub fn pane_snapshot_path(repo_root: &Path, run_id: &str, node_id: &str, iter: i64) -> PathBuf {
    repo_root
        .join(".pdo")
        .join("runs")
        .join(run_id)
        .join("nodes")
        .join(node_id)
        .join(format!("pane-iter-{iter}.snapshot"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_snapshot_path_lives_outside_the_iter_worktree() {
        let path = pane_snapshot_path(Path::new("/repo"), "20260101-120000-abc", "impl-1", 2);
        assert_eq!(
            path,
            PathBuf::from("/repo/.pdo/runs/20260101-120000-abc/nodes/impl-1/pane-iter-2.snapshot")
        );
        assert!(!path.to_string_lossy().contains("/iter-2/"));
    }

    #[test]
    fn only_a_resumed_pane_says_resumed() {
        assert!(Pane::new("x", PaneSource::Resumed, false).resumed);
        for source in [
            PaneSource::Live,
            PaneSource::Snapshot,
            PaneSource::Unavailable,
        ] {
            assert!(!Pane::new("x", source, false).resumed);
        }
        assert_eq!(PaneSource::Unavailable.as_str(), "unavailable");
    }
}
