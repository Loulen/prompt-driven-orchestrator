//! The private file a session's script travels in, so the variables it exports
//! (a Node's token, an extension's keys) never sit in the argv of a process:
//! `tmux new-session` and the shell it starts only see the file's path.
//!
//! The file is created `0600` with an unguessable name in a private folder
//! (`$XDG_RUNTIME_DIR`, else the temporary folder), never in a repository. Its
//! first line deletes it: `bash` keeps reading through the descriptor it holds,
//! so the file is gone before the harness starts. Whatever happens, it never
//! outlives [`LaunchFile`]: dropping it deletes the file, which covers a launch
//! that failed before `bash` read it.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::domain::launch::sh_single_quote;

/// How long [`LaunchFile::await_read`] waits for `bash` to read and delete the
/// file: milliseconds in practice, far more on a loaded host or behind a slow
/// shell startup file. Past it, the launch is a failure and the drop deletes
/// the file.
const READ_DEADLINE: Duration = Duration::from_secs(30);

pub(super) struct LaunchFile {
    path: PathBuf,
}

impl LaunchFile {
    /// Write `script` in a new `0600` file under `dir`, behind the line that
    /// deletes it.
    pub(super) fn write_in(dir: &Path, script: &str) -> std::io::Result<Self> {
        let path = dir.join(format!("pdo-launch-{}.sh", uuid::Uuid::new_v4().simple()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let launch = Self { path };
        let body = format!(
            "rm -f -- {}\n{script}\n",
            sh_single_quote(&launch.path.to_string_lossy())
        );
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        Ok(launch)
    }

    /// Write `script` in the private folder of the current user.
    pub(super) fn write(script: &str) -> std::io::Result<Self> {
        Self::write_in(&private_dir(), script)
    }

    /// The command `tmux` hands its shell: `bash` reading the file.
    pub(super) fn command(&self) -> String {
        format!(
            "exec bash {}",
            sh_single_quote(&self.path.to_string_lossy())
        )
    }

    /// Wait until `bash` has read the file and deleted it, up to
    /// [`READ_DEADLINE`]: whether it did. When it did not, the session never
    /// started its script, and the drop deletes the file unread.
    pub(super) fn await_read(&self) -> bool {
        self.await_read_within(READ_DEADLINE)
    }

    fn await_read_within(&self, deadline: Duration) -> bool {
        let start = Instant::now();
        while self.path.exists() && start.elapsed() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        !self.path.exists()
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LaunchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `$XDG_RUNTIME_DIR` (`0700`, the user's own) when it is a folder, else the
/// temporary folder, where the file's `0600` keeps it private.
fn private_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[test]
    fn the_file_is_private_and_deletes_itself_before_the_tail_runs() {
        let dir = tempfile::tempdir().unwrap();
        let launch = LaunchFile::write_in(
            dir.path(),
            "export SECRET_VALUE='witness-value' && printf %s \"$SECRET_VALUE\"",
        )
        .unwrap();
        let mode = std::fs::metadata(launch.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(launch.path().starts_with(dir.path()));

        let read = std::thread::scope(|scope| {
            let waiter = scope.spawn(|| launch.await_read_within(Duration::from_secs(10)));
            let out = Command::new("sh")
                .args(["-c", &launch.command()])
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
            assert_eq!(String::from_utf8_lossy(&out.stdout), "witness-value");
            waiter.join().unwrap()
        });
        assert!(read, "the launch saw bash read its file");
        assert!(!launch.path().exists(), "deleted before the tail ran");
        assert!(
            !launch.command().contains("witness-value"),
            "only the path reaches the argv"
        );
    }

    #[test]
    fn a_launch_that_never_reads_the_file_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let launch = LaunchFile::write_in(dir.path(), "exec true").unwrap();
        let path = launch.path().to_path_buf();
        assert!(
            !launch.await_read_within(Duration::from_millis(20)),
            "never read"
        );
        assert!(path.exists(), "still there until the drop");
        drop(launch);
        assert!(!path.exists());
    }
}
