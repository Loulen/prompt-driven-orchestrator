//! Execute a Trigger guard command before a fire.
//!
//! A guard is a single shell command run via `sh -c "<cmd>"` before each fire
//! (CONTEXT.md → *Trigger*). Its contract:
//!
//! - CWD = the target repo (so `gh issue list`, `git log` work without paths);
//! - inherits the daemon environment plus an injected `PDO_TARGET_REPO`;
//! - a hard timeout (default 60 s) so a hung guard never freezes the scheduler;
//! - exit 0 fires with stdout as the resolved input, non-zero skips, and a spawn
//!   failure or timeout records a guard-error outcome.
//!
//! This module is the side-effectful wrapper; the *decision* it feeds is the
//! pure [`crate::fire_decision`]. Returning a [`GuardResult`] keeps the
//! scheduler's branch matrix exhaustively unit-testable.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::fire_decision::GuardResult;

/// The hard timeout for a guard command. Run off the scheduler tick so a hung
/// guard never freezes the scheduler.
pub const GUARD_TIMEOUT_SECS: u64 = 60;

/// Test seam: override the guard timeout (in milliseconds) so integration tests
/// can exercise the timeout path without waiting the full production bound.
pub const GUARD_TIMEOUT_MS_OVERRIDE_ENV: &str = "PDO_GUARD_TIMEOUT_MS";

/// Environment variable injected into the guard process pointing at the target
/// repo, so a guard can reference it without hardcoding a path.
pub const TARGET_REPO_ENV: &str = "PDO_TARGET_REPO";

/// Resolve the guard timeout, honoring the [`GUARD_TIMEOUT_MS_OVERRIDE_ENV`]
/// test seam and falling back to [`GUARD_TIMEOUT_SECS`].
pub fn guard_timeout() -> Duration {
    std::env::var(GUARD_TIMEOUT_MS_OVERRIDE_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(GUARD_TIMEOUT_SECS))
}

/// Run a guard command and classify the outcome.
///
/// `command` is run as `sh -c "<command>"` with CWD set to `target_repo`. The
/// daemon environment is inherited and `PDO_TARGET_REPO` is injected.
/// The command is bounded by `timeout`; exceeding it yields
/// [`GuardResult::Error`] rather than blocking.
pub async fn run_guard(command: &str, target_repo: &Path, timeout: Duration) -> GuardResult {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(target_repo)
        .env(TARGET_REPO_ENV, target_repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Backstop: kill on drop in case the explicit kill below races a runtime
    // shutdown. The explicit kill is the primary guarantee.
    cmd.kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return GuardResult::Error {
                detail: format!("failed to spawn guard: {e}"),
            };
        }
    };

    // Read stdout concurrently with the wait via a separate task; reading only
    // after exit would deadlock a guard that fills the OS pipe buffer (~64 KB).
    let stdout_task = child.stdout.take().map(|mut pipe| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf).await;
            buf
        })
    });

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            let stdout = collect_stdout(stdout_task).await;
            if status.success() {
                GuardResult::Pass { stdout }
            } else {
                GuardResult::Skip
            }
        }
        Ok(Err(e)) => GuardResult::Error {
            detail: format!("guard process error: {e}"),
        },
        Err(_) => {
            // Timed out: explicitly kill and reap the still-running guard so it
            // can never outlive its bound, then surface the error outcome.
            let _ = child.start_kill();
            let _ = child.wait().await;
            GuardResult::Error {
                detail: format!("guard timed out after {}ms", timeout.as_millis()),
            }
        }
    }
}

/// Await the stdout-draining task and decode it lossily. Returns empty on a join
/// or read error so a guard with garbled output still yields a usable outcome.
async fn collect_stdout(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> String {
    match task {
        Some(handle) => match handle.await {
            Ok(buf) => String::from_utf8_lossy(&buf).to_string(),
            Err(_) => String::new(),
        },
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exit_zero_with_stdout_passes_with_that_stdout() {
        let dir = std::env::temp_dir();
        let result = run_guard("echo hello", &dir, Duration::from_secs(5)).await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: "hello\n".to_string()
            }
        );
    }

    #[tokio::test]
    async fn nonzero_exit_skips() {
        let dir = std::env::temp_dir();
        let result = run_guard("exit 1", &dir, Duration::from_secs(5)).await;
        assert_eq!(result, GuardResult::Skip);
    }

    #[tokio::test]
    async fn long_running_command_times_out_with_error() {
        let dir = std::env::temp_dir();
        // A command that outlives the timeout must be classified as an error,
        // never block the caller.
        let result = run_guard("sleep 30", &dir, Duration::from_millis(200)).await;
        match result {
            GuardResult::Error { detail } => assert!(
                detail.contains("timed out"),
                "expected a timeout detail, got {detail:?}"
            ),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cwd_is_the_target_repo() {
        // Create a unique marker file in a temp repo; `cat` it by relative name
        // — only succeeds if the guard runs with CWD = that repo.
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("marker.txt"), "i am here").unwrap();
        let result = run_guard("cat marker.txt", repo.path(), Duration::from_secs(5)).await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: "i am here".to_string()
            }
        );
    }

    #[tokio::test]
    async fn injects_pdo_target_repo_env_var() {
        let repo = tempfile::tempdir().unwrap();
        let result = run_guard(
            "printf '%s' \"$PDO_TARGET_REPO\"",
            repo.path(),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: repo.path().to_string_lossy().to_string()
            }
        );
    }
}
