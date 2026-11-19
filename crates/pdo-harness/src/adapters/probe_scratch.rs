//! The probe's disposable temp directory (CORE-140): every harness binary PDO
//! runs for its metadata gets a private `TMPDIR`, removed once the probe ends.
//!
//! Some harnesses write into `$TMPDIR` on every launch and never clean up:
//! `opencode` is a Bun single-file executable that extracts a ~4 MB native
//! library (`.<hex>-00000000.so`) there each time it starts. Probed at every
//! daemon boot and on the version re-check behind `/settings`, it filled `/tmp`
//! (50 GB in two days under the test suite). Pointing the child at a directory
//! the probe owns, and deleting that directory afterwards, makes a probe leave
//! nothing behind in the host's temp dir.

use std::path::Path;
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, SystemTime};

/// The env vars a child reads its temp dir from: `TMPDIR` (POSIX, Bun), and
/// `TMP` / `TEMP`, which some runtimes consult first.
const TEMP_VARS: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];

/// The name every scratch starts with, and the one the stale sweep matches.
const PREFIX: &str = "pdo-probe-";

/// Age past which a `pdo-probe-*` dir is an orphan. A probe lasts seconds (its
/// longest deadline is well under a minute), so an hour-old scratch belongs to
/// a process that died mid-probe (daemon stopped, test process killed) and
/// never ran the drop.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// The orphan sweep runs once per process, on its first probe.
static SWEEP: Once = Once::new();

/// A private temp dir for one probe child. Hold it until the child has exited:
/// dropping it removes the directory and everything the child wrote there.
///
/// Best effort by design: when the directory cannot be created, the scratch is
/// empty and [`ProbeScratch::apply`] leaves the child on the ambient temp dir,
/// as before CORE-140. A probe never fails on account of its scratch.
#[derive(Debug)]
pub(crate) struct ProbeScratch(Option<tempfile::TempDir>);

impl ProbeScratch {
    /// A scratch under the process's temp dir (`std::env::temp_dir()`).
    /// The first call of the process also removes the orphans
    /// ([`sweep_stale`]) earlier processes left there.
    pub(crate) fn new() -> Self {
        let base = std::env::temp_dir();
        SWEEP.call_once(|| sweep_stale(&base, STALE_AFTER));
        Self::in_dir(&base)
    }

    /// A scratch under `base`; empty when `base` refuses a new directory.
    pub(crate) fn in_dir(base: &Path) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(PREFIX)
                .tempdir_in(base)
                .ok(),
        )
    }

    /// The scratch directory, if one could be created.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.0.as_ref().map(tempfile::TempDir::path)
    }

    /// Point `cmd`'s temp dir at the scratch. No-op on an empty scratch.
    pub(crate) fn apply(&self, cmd: &mut Command) {
        if let Some(dir) = self.path() {
            for var in TEMP_VARS {
                cmd.env(var, dir);
            }
        }
    }
}

/// Remove the `pdo-probe-*` directories under `base` last modified more than
/// `older_than` ago: the scratches of a process that died mid-probe, with what
/// their child wrote (a Bun `.so` of ~4 MB for `opencode`). Best effort: an
/// unreadable base or entry is skipped, a live probe's scratch is too recent.
pub(crate) fn sweep_stale(base: &Path, older_than: Duration) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
            continue;
        }
        // `symlink_metadata`: never follow a link out of the temp dir.
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let stale = meta
            .modified()
            .ok()
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age >= older_than);
        if meta.is_dir() && stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(cmd: &Command, var: &str) -> Option<std::ffi::OsString> {
        cmd.get_envs()
            .find(|(k, _)| *k == var)
            .and_then(|(_, v)| v.map(ToOwned::to_owned))
    }

    /// CORE-140: the scratch is a fresh `pdo-probe-*` dir under its base, every temp
    /// var of the child points at it, and dropping it removes it with its contents.
    #[test]
    fn scratch_points_the_child_at_a_dir_it_removes_on_drop() {
        let base = tempfile::tempdir().unwrap();
        let scratch = ProbeScratch::in_dir(base.path());
        let dir = scratch
            .path()
            .expect("a writable base yields a scratch")
            .to_path_buf();
        assert_eq!(dir.parent(), Some(base.path()));
        assert!(dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("pdo-probe-"));

        let mut cmd = Command::new("true");
        scratch.apply(&mut cmd);
        for var in TEMP_VARS {
            assert_eq!(env_of(&cmd, var).as_deref(), Some(dir.as_os_str()), "{var}");
        }

        std::fs::write(dir.join(".deadbeef-00000000.so"), b"lib").unwrap();
        drop(scratch);
        assert!(
            !dir.exists(),
            "the scratch and what the child left are gone"
        );
    }

    /// CORE-140: a base that refuses a new directory yields an empty scratch, and the
    /// child keeps the ambient temp dir — the probe still runs.
    #[test]
    fn scratch_creation_failure_falls_back_to_ambient_tmpdir() {
        let base = tempfile::tempdir().unwrap();
        let not_a_dir = base.path().join("a-file");
        std::fs::write(&not_a_dir, b"").unwrap();

        let scratch = ProbeScratch::in_dir(&not_a_dir);
        assert!(scratch.path().is_none());
        let mut cmd = Command::new("true");
        scratch.apply(&mut cmd);
        for var in TEMP_VARS {
            assert_eq!(env_of(&cmd, var), None, "{var} must stay inherited");
        }
    }

    /// A daemon stopped mid-probe leaves its scratch, `.so` included: the sweep
    /// removes old `pdo-probe-*` dirs and keeps recent ones and anything else.
    #[test]
    fn sweep_removes_only_stale_probe_scratches() {
        let base = tempfile::tempdir().unwrap();
        let orphan = base.path().join("pdo-probe-orphan");
        std::fs::create_dir(&orphan).unwrap();
        std::fs::write(orphan.join(".deadbeef-00000000.so"), b"lib").unwrap();
        let other = base.path().join("not-a-probe");
        std::fs::create_dir(&other).unwrap();
        let link = base.path().join("pdo-probe-link");
        std::os::unix::fs::symlink(&other, &link).unwrap();

        sweep_stale(base.path(), STALE_AFTER);
        assert!(
            orphan.exists(),
            "a recent scratch may belong to a live probe"
        );

        sweep_stale(base.path(), Duration::ZERO);
        assert!(!orphan.exists(), "the stale scratch and its .so are gone");
        assert!(other.exists(), "only pdo-probe-* dirs are swept");
        assert!(link.symlink_metadata().is_ok(), "a link is never followed");
    }
}
