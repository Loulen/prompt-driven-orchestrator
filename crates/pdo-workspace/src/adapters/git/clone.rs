//! A shallow clone of a remote repository, for the contexts that read one
//! without working in it (the skill import scans a Source this way).

use std::path::Path;
use std::time::Duration;

/// Why a shallow clone did not produce a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneError {
    /// `git clone` exited non-zero; carries git's stderr.
    Failed(String),
    /// The clone took longer than the timeout it was given.
    Timeout,
    /// The caller fired the cancel signal.
    Cancelled,
    /// `git` could not be run or waited for.
    Io(String),
}

/// Clone `url` (at `git_ref` when given) into `dest`, one commit deep.
///
/// Runs with `GIT_TERMINAL_PROMPT=0`: a missing credential fails with git's own
/// message instead of hanging on a prompt nobody sees. The child is killed when
/// `cancel` fires or `timeout` elapses. `dest` is left as git left it: the
/// caller cleans it up on failure.
pub async fn clone_shallow(
    url: &str,
    git_ref: Option<&str>,
    dest: &Path,
    cancel: tokio::sync::oneshot::Receiver<()>,
    timeout: Duration,
) -> Result<(), CloneError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--depth", "1", "--single-branch", "--quiet"]);
    if let Some(git_ref) = git_ref {
        cmd.args(["--branch", git_ref]);
    }
    cmd.arg(url)
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let result = async {
        let child = cmd
            .spawn()
            .map_err(|e| CloneError::Io(format!("cannot run git: {e}")))?;
        tokio::select! {
            out = child.wait_with_output() => out.map_err(|e| CloneError::Io(e.to_string())),
            _ = cancel => Err(CloneError::Cancelled),
        }
    };
    match tokio::time::timeout(timeout, result).await {
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => Err(CloneError::Failed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        )),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(CloneError::Timeout),
    }
}
