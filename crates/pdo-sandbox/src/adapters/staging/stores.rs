//! Where a Run's harness transcripts are read from (#408, #707, #708): the staged
//! sinks while a sandboxed Run's staging exists, the host stores otherwise. Keyed on
//! the staging directory's existence, hence an adapter.

use std::path::{Path, PathBuf};

use crate::domain::staging::{staged_claude_home, staged_path, staging_dir_for_run};

/// The `projects/` root where cost (`run_cost`) and stale-detection
/// (`stale_detector`) read a Run's Claude Code transcripts (#408). The
/// SINGLE seam shared by both consumers.
///
/// - A sandboxed Run whose **staging still exists** (live / reapable / resumed)
///   → its staged home (`<sandbox_root>/<run_id>/claude-home/projects/`), where
///   `claude` writes in real time through the identity mount.
/// - `off`, OR a sandboxed Run whose staging was purged by `cleanup_run` → the
///   host `~/.claude/projects/`, where `merge_back` flushed the transcripts.
///
/// Dispatch is keyed on the **existence of the staging dir** (not the Run's
/// terminal status): it stays correct even if the best-effort terminal
/// `merge_back` failed, as long as the staging lives (cf. plan #408 D-1). The cwd
/// encoding is unchanged — the caller still resolves the encoded dirname from
/// this base via [`pdo_harness::claude_transcript::encode_working_dir`], the single source
/// of truth (#373); the seam only swaps the base `projects/` root.
/// `sandboxed` is a plain `bool`, not a [`crate::domain::mode::SandboxMode`] (#432 D2): this seam only ever
/// asked about off-ness, and since the mode owns a `String` a by-value parameter would
/// force a clone at each of the call sites — several of which capture it into a detached
/// `async move`, where a reference cannot live.
pub fn transcripts_root(
    sandboxed: bool,
    run_id: &str,
    home_root: &Path,
    sandbox_root: &Path,
) -> PathBuf {
    if sandboxed && staging_dir_for_run(sandbox_root, run_id).exists() {
        staged_claude_home(sandbox_root, run_id).join("projects")
    } else {
        home_root.join(".claude").join("projects")
    }
}

/// The `copilot` session-state store root — `<home_root>/.copilot/session-state/`,
/// where each session's event journal lives at `<session-id>/events.jsonl` (#615).
///
/// Always the **host** home, unlike [`transcripts_root`]: `copilot` declares **no
/// staging set** (ADR-0063 / #615), so a sandboxed Run has no staged copilot home
/// to mirror — the journal is read where the harness wrote it. Path math only; this
/// module never reads `$HOME` (the caller injects `home_root`).
pub fn copilot_store_root(home_root: &Path) -> PathBuf {
    home_root.join(".copilot").join("session-state")
}

/// The `pi` sessions store root on the **host** — `<home_root>/.pi/agent/sessions/`,
/// where each working directory's folder holds `<timestamp>_<session-id>.jsonl`
/// files (#707, [`pdo_harness::pi_session`]). Path math only; the per-Run seam that moves
/// it while a sandboxed Run lives is [`HarnessStores::for_run`].
pub fn pi_store_root(home_root: &Path) -> PathBuf {
    home_root.join(".pi").join("agent").join("sessions")
}

/// Where a Run's `pi` sessions are read (#708, ADR-0063): the **staged** sink
/// (`<staging>/home/.pi/agent/sessions/`) while a sandboxed Run's staging exists —
/// pi writes there in real time through the rw mount of its home root — else the
/// host store where `merge_back` flushed them. The same dispatch rule as
/// [`transcripts_root`] (keyed on the staging dir's existence, never on the Run's
/// status), so the cost of a sandboxed pi Run reads while the Run lives AND after
/// its merge-back. The cwd-encoded folder name is identical on both sides because
/// the worktree is mounted at its host path (ADR-0030).
pub fn pi_sessions_root(
    sandboxed: bool,
    run_id: &str,
    home_root: &Path,
    sandbox_root: &Path,
) -> PathBuf {
    if sandboxed && staging_dir_for_run(sandbox_root, run_id).exists() {
        staged_path(sandbox_root, run_id, ".pi/agent/sessions")
    } else {
        pi_store_root(home_root)
    }
}

/// The store roots of the reported-cost harnesses (#707): one field per harness, so a
/// consumer that reads several harnesses' stores threads one value instead of one
/// root per harness. `copilot` (no staging set) is always the host store; `pi` moves
/// per Run like `claude` does ([`pi_sessions_root`], #708) — build the struct with
/// [`HarnessStores::for_run`] for a Run, [`HarnessStores::from_home`] for the host
/// only. `claude`'s root is not here: it stays the caller's separate `claude_root`
/// ([`transcripts_root`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessStores {
    /// [`copilot_store_root`].
    pub copilot: PathBuf,
    /// [`pi_store_root`].
    pub pi: PathBuf,
}

impl HarnessStores {
    /// Both **host**-home roots derived from `home_root` — the `off` Run's view, and
    /// the view of a sandboxed Run whose staging is gone. Every production caller
    /// threads the per-Run [`Self::for_run`] (pi's store moves with the sandbox); the
    /// cost/perf unit tests exercise this host-only view.
    pub fn from_home(home_root: &Path) -> Self {
        HarnessStores {
            copilot: copilot_store_root(home_root),
            pi: pi_store_root(home_root),
        }
    }

    /// The roots **for one Run** (#708): `copilot` on the host, `pi` on the staged
    /// sink while a sandboxed Run's staging exists ([`pi_sessions_root`]).
    pub fn for_run(sandboxed: bool, run_id: &str, home_root: &Path, sandbox_root: &Path) -> Self {
        HarnessStores {
            copilot: copilot_store_root(home_root),
            pi: pi_sessions_root(sandboxed, run_id, home_root, sandbox_root),
        }
    }

    /// The store root `harness`'s transcript resolution reads from: this struct's
    /// own for `copilot` / `pi`, `claude_root` for `claude` **and for any other
    /// name** — a data-declared harness resolves no transcript at all
    /// (`pdo_harness::adapters::transcripts::resolve_transcript` answers `None` before the root is ever
    /// joined), so handing it the claude root is inert. Picks between independently
    /// computed roots, not between behaviours (ADR-0051): behaviour dispatch stays in
    /// `harness_probes`.
    pub fn root_for<'a>(&'a self, harness: &str, claude_root: &'a Path) -> &'a Path {
        match harness {
            pdo_harness::registry::COPILOT => &self.copilot,
            pdo_harness::registry::PI => &self.pi,
            _ => claude_root,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcripts_root_off_is_always_host() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Even if a staging dir happens to exist, `off` reads the host.
        std::fs::create_dir_all(staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        assert_eq!(
            transcripts_root(false, "r1", &home, &sandbox_root),
            home.join(".claude").join("projects")
        );
    }

    #[test]
    fn transcripts_root_sandboxed_live_is_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // Staging present → live/reapable Run → read the staged home.
        std::fs::create_dir_all(staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        assert_eq!(
            transcripts_root(true, "r1", &home, &sandbox_root),
            staged_claude_home(&sandbox_root, "r1").join("projects")
        );
    }

    #[test]
    fn transcripts_root_sandboxed_after_cleanup_is_host() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        // No staging dir (post-`cleanup_run`) → read the host, where merge_back
        // flushed. Keyed on staging EXISTENCE, not the Run's terminal status.
        assert!(!staging_dir_for_run(&sandbox_root, "r1").exists());
        assert_eq!(
            transcripts_root(true, "r1", &home, &sandbox_root),
            home.join(".claude").join("projects")
        );
    }

    #[test]
    fn transcripts_root_encoding_is_root_invariant() {
        // AC5: the seam only swaps the base `projects/` root — the per-cwd encoded
        // segment stays the single source of truth (`encode_working_dir`, #373),
        // never re-derived by the seam. So for a given cwd, appending the encoded
        // segment to the seam-resolved root yields the same dirname regardless of
        // which base (host vs staging) the seam picked.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox_root = tmp.path().join("sandbox");
        let cwd = std::path::Path::new("/home/u/.pdo/runs/r1/worktree");
        let enc = pdo_harness::claude_transcript::encode_working_dir(cwd);

        // Host arm (no staging).
        let host = transcripts_root(true, "r1", &home, &sandbox_root);
        assert_eq!(host.join(&enc), home.join(".claude/projects").join(&enc));

        // Staging arm (staging materialised) — same encoded segment appended.
        std::fs::create_dir_all(staging_dir_for_run(&sandbox_root, "r1")).unwrap();
        let staged = transcripts_root(true, "r1", &home, &sandbox_root);
        assert_eq!(
            staged.join(&enc),
            staged_claude_home(&sandbox_root, "r1")
                .join("projects")
                .join(&enc)
        );
        // The bases differ, but the trailing encoded segment is identical.
        assert_eq!(host.file_name(), staged.file_name()); // both end in "projects"
        assert_eq!(
            host.join(&enc).file_name(),
            staged.join(&enc).file_name(),
            "the encoded cwd segment is root-invariant"
        );
    }
}
