//! Clones of a remote repository: a shallow one, for the contexts that read one
//! without working in it (the skill import scans a Source this way), and a full
//! one, for a repository added by its URL.

use std::path::Path;
use std::time::Duration;

use crate::domain::clone::CloneFailure;

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

/// Clone `url` into `dest` in full (every branch, every commit), calling
/// `on_progress` with each progress line git prints.
///
/// The daemon's git credentials apply (its credential helpers, its ssh keys
/// and agent), but nothing prompts: `GIT_TERMINAL_PROMPT=0` and a batch-mode
/// ssh make a missing credential fail with git's own message. The parent of
/// `dest` is created when missing. git clones into a hidden folder beside
/// `dest`, renamed `dest` once complete: a failure, a cancel or a daemon
/// stopped mid-clone never leaves a half clone where a clone is looked for.
/// `dest` may be an empty folder, replaced then.
pub async fn clone_full(
    url: &str,
    dest: &Path,
    on_progress: &(dyn Fn(&str) + Send + Sync),
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), CloneFailure> {
    use tokio::io::AsyncReadExt;

    let (Some(parent), Some(name)) = (dest.parent(), dest.file_name()) else {
        return Err(CloneFailure::Failed(format!(
            "{} names no folder to clone into",
            dest.display()
        )));
    };
    std::fs::create_dir_all(parent)
        .map_err(|e| CloneFailure::Failed(format!("could not create {}: {e}", parent.display())))?;
    let staging = parent.join(format!(
        ".{}.cloning-{}",
        name.to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    // Keep an operator's own `GIT_SSH_COMMAND`, only adding the batch flag
    // (the precedent of the fetch).
    let ssh_command = match std::env::var("GIT_SSH_COMMAND") {
        Ok(existing) if !existing.trim().is_empty() => format!("{existing} -oBatchMode=yes"),
        _ => "ssh -oBatchMode=yes".to_string(),
    };
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--progress", "--"])
        .arg(url)
        .arg(&staging)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_SSH_COMMAND", ssh_command)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| CloneFailure::Failed(format!("cannot run git: {e}")))?;
    let mut stderr = child.stderr.take().expect("stderr is piped");

    let read = async {
        // git rewrites its progress line with `\r`; every `\r` or `\n` ends one.
        let mut said = String::new();
        let mut line = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stderr.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            for &byte in &buf[..n] {
                if byte == b'\r' || byte == b'\n' {
                    let text = String::from_utf8_lossy(&line).to_string();
                    if !text.trim().is_empty() {
                        on_progress(&text);
                        if byte == b'\n' {
                            said.push_str(&text);
                            said.push('\n');
                        }
                    }
                    line.clear();
                } else {
                    line.push(byte);
                }
            }
        }
        if !line.is_empty() {
            said.push_str(&String::from_utf8_lossy(&line));
        }
        said
    };

    // A cancel sender dropped without firing cancels nothing.
    let said = tokio::select! {
        biased;
        Ok(()) = cancel => None,
        said = read => Some(said),
    };
    let outcome = match said {
        None => {
            let _ = child.kill().await;
            Err(CloneFailure::Cancelled)
        }
        Some(said) => match child.wait().await {
            Ok(status) if status.success() => into_place(&staging, dest),
            Ok(_) => Err(CloneFailure::Failed(said)),
            Err(e) => Err(CloneFailure::Failed(format!("git clone failed: {e}"))),
        },
    };
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    outcome
}

/// The complete clone at `staging` becomes `dest` (an empty `dest` folder
/// gives it its place).
fn into_place(staging: &Path, dest: &Path) -> Result<(), CloneFailure> {
    if dest.is_dir() {
        // Only an empty folder goes: `remove_dir` refuses any other.
        std::fs::remove_dir(dest)
            .map_err(|e| CloneFailure::Failed(format!("{} is not empty: {e}", dest.display())))?;
    }
    std::fs::rename(staging, dest).map_err(|e| {
        CloneFailure::Failed(format!(
            "could not move the clone to {}: {e}",
            dest.display()
        ))
    })
}
