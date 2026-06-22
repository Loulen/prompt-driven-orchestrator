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

/// Cap on the guard output we *capture* for the fire history (#244): each of
/// stdout/stderr is tail-kept to at most this many bytes. This bounds the
/// diagnostic blob stored per skip row; it is **never** applied to the `Pass`
/// path, whose stdout is the actual Run input and must stay byte-for-byte.
pub const GUARD_CAPTURE_LIMIT_BYTES: usize = 16 * 1024;

/// Prefix prepended to a tail-capped stream so a reader knows the head was
/// dropped (errors usually print last, so we keep the tail).
const TRUNCATION_MARKER: &str = "…[truncated, showing last 16 KB]\n";

/// Keep the last `limit` *bytes* of `s`, snapped forward to a UTF-8 char
/// boundary so the result is always valid UTF-8; prefix [`TRUNCATION_MARKER`]
/// when truncated. Counts bytes (it's a storage bound), not chars.
fn cap_tail(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut start = s.len() - limit;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("{TRUNCATION_MARKER}{}", &s[start..])
}

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
            let stdout = collect_stream(stdout_task).await;
            if status.success() {
                // The input path: stdout is the Run input — keep it uncapped.
                GuardResult::Pass { stdout }
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

/// Await a stream-draining task (stdout or stderr) and decode it lossily.
/// Returns empty on a join or read error so a guard with garbled output still
/// yields a usable outcome.
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
        // forever and was misclassified as a timeout `guard-error`. With the
        // concurrent stderr drain it must return a Skip well under the timeout.
        let dir = std::env::temp_dir();
        let result = run_guard(
            "yes flood | head -c 200000 >&2; exit 1",
            &dir,
            // A generous-but-finite timeout: pre-fix this deadlocks until the
            // bound, post-fix it returns in milliseconds.
            Duration::from_secs(10),
        )
        .await;
        match result {
            GuardResult::Skip {
                stderr, exit_code, ..
            } => {
                assert_eq!(exit_code, Some(1));
                // Tail-capped to the 16 KB bound (+ marker), never unbounded.
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

    #[test]
    fn cap_tail_keeps_the_last_bytes_with_a_marker() {
        // Short input passes through untouched.
        assert_eq!(cap_tail("short", 1024), "short");

        // Long input keeps the tail and prefixes the marker.
        let s: String = (0..1000).map(|_| 'a').collect();
        let capped = cap_tail(&s, 100);
        assert!(capped.starts_with(TRUNCATION_MARKER));
        assert_eq!(capped.len(), TRUNCATION_MARKER.len() + 100);
        assert!(capped.ends_with("aaaa"));
    }

    #[test]
    fn cap_tail_snaps_to_a_utf8_char_boundary() {
        // A multi-byte char straddling the cut must not panic and must yield
        // valid UTF-8; we snap forward, so the cap is the *upper* bound.
        let s = "é".repeat(100); // each 'é' is 2 bytes → 200 bytes
        let capped = cap_tail(&s, 51); // 51 lands mid-codepoint
        // Snapping forward drops the straddling byte, so ≤ 51 bytes of payload.
        assert!(capped.starts_with(TRUNCATION_MARKER));
        let payload = &capped[TRUNCATION_MARKER.len()..];
        assert!(payload.chars().all(|c| c == 'é'));
        assert!(payload.len() <= 51);
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
