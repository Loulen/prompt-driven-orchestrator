//! The guard, run by `sh -c "<cmd>"` before a fire (CONTEXT.md → *Trigger*).
//! Its contract:
//!
//! - CWD = the target repo (so `gh issue list`, `git log` work without paths);
//! - inherits the daemon environment, plus the variables the extensions give
//!   it (never in its argv) and an injected `PDO_TARGET_REPO`, which they never
//!   overwrite;
//! - a hard timeout so a hung guard never freezes the scheduler;
//! - its own process group, killed whole when the guard ends (whatever its
//!   end, a dropped request included): what it started never outlives it, nor
//!   the variables handed to it (a process that leaves the group, `setsid`,
//!   escapes);
//! - exit 0 fires with stdout as the resolved input, non-zero skips, and a spawn
//!   failure or timeout records a guard-error outcome.
//!
//! This is the side-effectful wrapper; the *decision* it feeds is the pure
//! [`crate::domain::fire_decision`].

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use super::env::TARGET_REPO_ENV;
use crate::application::ports::{BoxFuture, GuardRunner};
use crate::domain::fire_decision::GuardResult;
use crate::domain::guard::{cap_tail, GUARD_CAPTURE_LIMIT_BYTES};

/// The guard runner of the daemon: a real `sh`.
pub struct ShellGuard;

impl GuardRunner for ShellGuard {
    fn run<'a>(
        &'a self,
        command: &'a str,
        cwd: &'a Path,
        env: &'a BTreeMap<String, String>,
        timeout: Duration,
    ) -> BoxFuture<'a, GuardResult> {
        Box::pin(run_guard_with(command, cwd, env, timeout))
    }
}

/// Run a guard command and classify the outcome. Exceeding `timeout` yields
/// [`GuardResult::Error`] rather than blocking.
pub async fn run_guard(command: &str, target_repo: &Path, timeout: Duration) -> GuardResult {
    run_guard_with(command, target_repo, &BTreeMap::new(), timeout).await
}

/// [`run_guard`], with `env` added to the guard's environment (its values in
/// no argv); `PDO_TARGET_REPO` stays the core's.
pub async fn run_guard_with(
    command: &str,
    target_repo: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> GuardResult {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(target_repo)
        .envs(env)
        .env(TARGET_REPO_ENV, target_repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Backstop: kill on drop in case the explicit kill below races a runtime
    // shutdown. The explicit kill is the primary guarantee.
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return GuardResult::Error {
                detail: format!("failed to spawn guard: {e}"),
                timed_out: false,
            };
        }
    };
    // Kills the guard's whole group when this future ends or is dropped (a
    // "Run now" or a dry-run whose request goes away).
    let mut group = ProcessGroup::of(&child);

    // Read stdout AND stderr concurrently with the wait via separate tasks;
    // reading only after exit would deadlock a guard that fills *either* OS pipe
    // buffer (~64 KB). Draining stderr also fixes a latent deadlock (#244): it was
    // piped but never read, so a guard flooding stderr blocked forever and was
    // misclassified as a timeout `guard-error`.
    let stdout_task = child.stdout.take().map(|mut pipe| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf).await;
            buf
        })
    });
    let stderr_task = child.stderr.take().map(|mut pipe| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf).await;
            buf
        })
    });

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            // What the guard left running goes with it, and so do the pipes it
            // held open.
            group.kill();
            let stdout = collect_stream(stdout_task).await;
            if status.success() {
                // The input path: stdout is the Run input — keep it uncapped;
                // stderr is diagnostics, tail-capped like a skip's. Bounded: a
                // background process the guard left behind may hold stderr open,
                // and a passing guard must never hang the fire for its warnings.
                let stderr = collect_within(stderr_task, PASS_STDERR_GRACE).await;
                GuardResult::Pass {
                    stdout,
                    stderr: cap_tail(&stderr, GUARD_CAPTURE_LIMIT_BYTES),
                }
            } else {
                // The skip path: capture both streams + the exit code as
                // diagnostics, tail-capped so a chatty guard can't bloat the DB.
                let stderr = collect_stream(stderr_task).await;
                GuardResult::Skip {
                    stdout: cap_tail(&stdout, GUARD_CAPTURE_LIMIT_BYTES),
                    stderr: cap_tail(&stderr, GUARD_CAPTURE_LIMIT_BYTES),
                    exit_code: status.code(),
                }
            }
        }
        Ok(Err(e)) => {
            group.kill();
            GuardResult::Error {
                detail: format!("guard process error: {e}"),
                timed_out: false,
            }
        }
        Err(_) => {
            // Timed out: explicitly kill and reap the still-running guard and
            // all it started so none can outlive its bound, then surface the
            // error outcome.
            group.kill();
            let _ = child.start_kill();
            let _ = child.wait().await;
            GuardResult::Error {
                detail: format!("guard timed out after {}ms", timeout.as_millis()),
                timed_out: true,
            }
        }
    }
}

/// The process group a guard leads (its `sh` was spawned with
/// `process_group(0)`), killed whole once: on [`ProcessGroup::kill`], else on
/// drop. Once: after the guard is reaped and its group empty, the id may name
/// another group.
struct ProcessGroup {
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl ProcessGroup {
    fn of(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            Self {
                pgid: child.id().and_then(|pid| i32::try_from(pid).ok()),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = child;
            Self {}
        }
    }

    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid.take().filter(|pgid| *pgid > 1) {
            // SAFETY: `killpg` only sends a signal; the group is the guard's
            // own, never the daemon's (`process_group(0)` at spawn).
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

/// How long a passing guard's stderr is waited for once the guard exited.
const PASS_STDERR_GRACE: Duration = Duration::from_millis(500);

/// [`collect_stream`] within `grace`; past it, the reader is aborted and what
/// was not read is dropped (an empty string).
async fn collect_within(task: Option<tokio::task::JoinHandle<Vec<u8>>>, grace: Duration) -> String {
    let Some(handle) = task else {
        return String::new();
    };
    let abort = handle.abort_handle();
    match tokio::time::timeout(grace, handle).await {
        Ok(Ok(buf)) => String::from_utf8_lossy(&buf).to_string(),
        Ok(Err(_)) => String::new(),
        Err(_) => {
            abort.abort();
            String::new()
        }
    }
}

/// Decodes lossily and returns empty on a join error, so a guard with garbled
/// output still yields a usable outcome.
async fn collect_stream(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> String {
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
    use crate::domain::guard::TRUNCATION_MARKER;

    #[tokio::test]
    async fn exit_zero_with_stdout_passes_with_that_stdout() {
        let dir = std::env::temp_dir();
        let result = run_guard("echo hello", &dir, Duration::from_secs(5)).await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: "hello\n".to_string(),
                stderr: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn a_passing_guard_keeps_its_stderr_for_the_dry_run() {
        let dir = std::env::temp_dir();
        let result = run_guard(
            "echo work; echo 'rate limit near' >&2",
            &dir,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: "work\n".to_string(),
                stderr: "rate limit near\n".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn a_passing_guard_whose_child_holds_stderr_open_does_not_hang_the_fire() {
        let dir = std::env::temp_dir();
        let started = std::time::Instant::now();
        let result = run_guard(
            "echo work; (sleep 30 >/dev/null &)",
            &dir,
            Duration::from_secs(10),
        )
        .await;
        assert!(
            matches!(&result, GuardResult::Pass { stdout, .. } if stdout == "work\n"),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn nonzero_exit_skips() {
        let dir = std::env::temp_dir();
        let result = run_guard("exit 1", &dir, Duration::from_secs(5)).await;
        assert_eq!(
            result,
            GuardResult::Skip {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: Some(1),
            }
        );
    }

    #[tokio::test]
    async fn nonzero_exit_captures_stdout_stderr_and_exit_code() {
        // #244: a non-zero guard's streams + exit code are captured so the fire
        // history can explain the skip (grep-style guards print the "why" on
        // stderr while stdout stays empty).
        let dir = std::env::temp_dir();
        let result = run_guard(
            "printf 'checked 0 issues'; echo 'gh: no work to do' >&2; exit 7",
            &dir,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            result,
            GuardResult::Skip {
                stdout: "checked 0 issues".to_string(),
                stderr: "gh: no work to do\n".to_string(),
                exit_code: Some(7),
            }
        );
    }

    #[tokio::test]
    async fn stderr_flood_returns_promptly_not_a_timeout() {
        // Regression for the latent deadlock (#244): stderr was piped but never
        // drained, so a guard flooding it past the ~64 KB pipe buffer blocked
        // until the timeout bound and was misclassified as a `guard-error`.
        let dir = std::env::temp_dir();
        let result = run_guard(
            "yes flood | head -c 200000 >&2; exit 1",
            &dir,
            Duration::from_secs(10),
        )
        .await;
        match result {
            GuardResult::Skip {
                stderr, exit_code, ..
            } => {
                assert_eq!(exit_code, Some(1));
                assert!(
                    stderr.len() <= GUARD_CAPTURE_LIMIT_BYTES + TRUNCATION_MARKER.len(),
                    "stderr must be tail-capped, got {} bytes",
                    stderr.len()
                );
                assert!(
                    stderr.starts_with(TRUNCATION_MARKER),
                    "a truncated stream must carry the marker"
                );
            }
            other => panic!("expected a prompt Skip, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn long_running_command_times_out_with_error() {
        let dir = std::env::temp_dir();
        let result = run_guard("sleep 30", &dir, Duration::from_millis(200)).await;
        match result {
            GuardResult::Error { detail, timed_out } => assert!(
                timed_out && detail.contains("timed out"),
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
                stdout: "i am here".to_string(),
                stderr: String::new(),
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
                stdout: repo.path().to_string_lossy().to_string(),
                stderr: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn the_extensions_variables_reach_the_guard_but_never_its_target_repo() {
        let repo = tempfile::tempdir().unwrap();
        let env = BTreeMap::from([
            ("X_TEST_TOKEN".to_string(), "s3cr3t".to_string()),
            (TARGET_REPO_ENV.to_string(), "/elsewhere".to_string()),
        ]);
        let result = run_guard_with(
            "printf '%s|%s' \"$X_TEST_TOKEN\" \"$PDO_TARGET_REPO\"",
            repo.path(),
            &env,
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            result,
            GuardResult::Pass {
                stdout: format!("s3cr3t|{}", repo.path().display()),
                stderr: String::new(),
            }
        );
    }

    /// A guard whose background child would leave `marker` after `delay_ms`.
    fn guard_leaving_a_child(marker: &Path, delay_ms: u32, then: &str) -> String {
        format!(
            "(sleep {}; touch '{}') >/dev/null 2>&1 & {then}",
            f64::from(delay_ms) / 1000.0,
            marker.display()
        )
    }

    async fn assert_the_child_never_lands(marker: &Path) {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(!marker.exists(), "the guard's child outlived it");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_timed_out_guard_takes_its_children_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = guard_leaving_a_child(&marker, 800, "sleep 30");
        let result = run_guard(&command, dir.path(), Duration::from_millis(200)).await;
        assert!(
            matches!(
                result,
                GuardResult::Error {
                    timed_out: true,
                    ..
                }
            ),
            "{result:?}"
        );
        assert_the_child_never_lands(&marker).await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_guard_dropped_mid_run_takes_its_children_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = guard_leaving_a_child(&marker, 800, "sleep 30");
        let dropped = tokio::time::timeout(
            Duration::from_millis(200),
            run_guard(&command, dir.path(), Duration::from_secs(30)),
        )
        .await;
        assert!(dropped.is_err(), "the guard should still be running");
        assert_the_child_never_lands(&marker).await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_guard_that_ended_leaves_no_child_behind() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let command = guard_leaving_a_child(&marker, 800, "echo work");
        let result = run_guard(&command, dir.path(), Duration::from_secs(5)).await;
        assert!(
            matches!(&result, GuardResult::Pass { stdout, .. } if stdout == "work\n"),
            "{result:?}"
        );
        assert_the_child_never_lands(&marker).await;
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn a_passing_guard_whose_child_holds_stdout_open_does_not_hang_the_fire() {
        let dir = std::env::temp_dir();
        let started = std::time::Instant::now();
        let result = run_guard("echo work; sleep 30 &", &dir, Duration::from_secs(10)).await;
        assert!(
            matches!(&result, GuardResult::Pass { stdout, .. } if stdout == "work\n"),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn a_variable_s_value_never_sits_in_the_guard_s_command_line() {
        let repo = tempfile::tempdir().unwrap();
        let env = BTreeMap::from([("X_TEST_TOKEN".to_string(), "s3cr3t-value".to_string())]);
        // The guard prints its own shell's command line, which reads the
        // variable.
        let result = run_guard_with(
            "test -n \"$X_TEST_TOKEN\" && tr '\\0' ' ' < /proc/$$/cmdline",
            repo.path(),
            &env,
            Duration::from_secs(5),
        )
        .await;
        let GuardResult::Pass { stdout, .. } = result else {
            panic!("the guard did not pass: {result:?}");
        };
        assert!(stdout.contains("sh -c"), "{stdout}");
        assert!(!stdout.contains("s3cr3t-value"), "{stdout}");
    }
}
