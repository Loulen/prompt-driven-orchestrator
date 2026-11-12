//! Clones of a remote repository: a shallow one, for the contexts that read one
//! without working in it (the skill import scans a Source this way), and a full
//! one, for a repository added by its URL; and the check that a remote answers.
//!
//! Each runs with the environment it is given (the credentials an extension
//! provides for the Actor, ADR-0098: in git's environment, never in an argv)
//! under the core's own variables, which nothing overwrites.

use std::collections::BTreeMap;
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

/// `ssh` in batch mode: the operator's own `GIT_SSH_COMMAND` (a jump host, an
/// identity file) with the batch flag added, else plain `ssh` with it.
fn batch_ssh_command() -> String {
    match std::env::var("GIT_SSH_COMMAND") {
        Ok(existing) if !existing.trim().is_empty() => format!("{existing} -oBatchMode=yes"),
        _ => "ssh -oBatchMode=yes".to_string(),
    }
}

/// `git <args>` reaching a remote, `env` added to its environment under the
/// core's own variables (`CORE_GIT_VARIABLES`), which come last so nothing
/// overwrites them: no prompt (`GIT_TERMINAL_PROMPT=0`, an askpass that
/// answers nothing, a batch-mode ssh keeping the operator's
/// `GIT_SSH_COMMAND`) and git's messages in English, which the core reads.
/// No stdin, no stdout, stderr piped; killed when dropped.
pub(super) fn network_git(
    args: &[&str],
    env: &BTreeMap<String, String>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .envs(env)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_SSH_COMMAND", batch_ssh_command())
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    cmd
}

/// Clone `url` (at `git_ref` when given) into `dest`, one commit deep, with
/// `env` added to git's environment.
///
/// Non-interactive ([`network_git`]): a missing credential fails with git's own
/// message instead of hanging on a prompt nobody sees. The child is killed when
/// `cancel` fires or `timeout` elapses. `dest` is left as git left it: the
/// caller cleans it up on failure.
pub async fn clone_shallow(
    url: &str,
    git_ref: Option<&str>,
    dest: &Path,
    env: &BTreeMap<String, String>,
    cancel: tokio::sync::oneshot::Receiver<()>,
    timeout: Duration,
) -> Result<(), CloneError> {
    let mut cmd = network_git(
        &["clone", "--depth", "1", "--single-branch", "--quiet"],
        env,
    );
    if let Some(git_ref) = git_ref {
        cmd.args(["--branch", git_ref]);
    }
    cmd.arg(url).arg(dest);
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

/// Clone `url` into `dest` in full (every branch, every commit), with `env`
/// added to git's environment, calling `on_progress` with each progress line
/// git prints.
///
/// The daemon's git credentials apply (its credential helpers, its ssh keys
/// and agent) beside what `env` provides, but nothing prompts
/// ([`network_git`]): a missing credential fails with git's own message. The parent of
/// `dest` is created when missing. git clones into a hidden folder beside
/// `dest`, renamed `dest` once complete: a failure, a cancel or a daemon
/// stopped mid-clone never leaves a half clone where a clone is looked for.
/// `dest` may be an empty folder, replaced then.
pub async fn clone_full(
    url: &str,
    dest: &Path,
    env: &BTreeMap<String, String>,
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
    let mut cmd = network_git(&["clone", "--progress", "--"], env);
    cmd.arg(url).arg(&staging);
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

/// How long the check that a remote answers may take.
const REACH_TIMEOUT: Duration = Duration::from_secs(20);

/// Whether `url` answers with `env` added to git's environment: `git
/// ls-remote` lists its refs, nothing is written. Non-interactive as the
/// clones are; `Err` carries git's stderr.
pub async fn ls_remote(url: &str, env: &BTreeMap<String, String>) -> Result<(), CloneFailure> {
    let mut cmd = network_git(&["ls-remote", "--quiet", "--"], env);
    cmd.arg(url);
    let child = cmd
        .spawn()
        .map_err(|e| CloneFailure::Failed(format!("cannot run git: {e}")))?;
    match tokio::time::timeout(REACH_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => Err(CloneFailure::Failed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        )),
        Ok(Err(e)) => Err(CloneFailure::Failed(format!("git ls-remote failed: {e}"))),
        Err(_) => Err(CloneFailure::Failed(format!(
            "{url} did not answer within {}s",
            REACH_TIMEOUT.as_secs()
        ))),
    }
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
