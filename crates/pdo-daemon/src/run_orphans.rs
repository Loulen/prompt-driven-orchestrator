//! The processes a Run leaves behind, stopped when the Run is archived.
//!
//! Killing a node's tmux session only reaches what is still attached to it. Anything
//! an agent detached — `nohup`, `setsid`, `cmd &` in a shell that then exits, a dev
//! server, a file watcher — is reparented to init and survives: in production a
//! `tsx` dev server of a Run archived at 02:42 was still running 16 h later. Over
//! days these orphans add up on a shared machine.
//!
//! Every process an agent starts inherits the session's `PDO_RUN_ID`, detached or
//! not, so the environment identifies a Run's processes with no bookkeeping at spawn.
//! Archiving is the moment they go: the Run is over, its worktree is about to be
//! removed, and nothing of it can be reattached. A merely terminal Run keeps its
//! processes — a server it started may still be under human validation.
//!
//! Containers are out of scope: their processes descend from the container runtime,
//! not from the session, and do not carry the variable.

use std::path::Path;
use std::time::{Duration, Instant};

/// How long processes get to exit on `SIGTERM` before `SIGKILL`.
const TERM_GRACE: Duration = Duration::from_secs(3);

/// The pids under `proc_root` whose environment carries exactly
/// `PDO_RUN_ID=<run_id>`, excluding `exclude` (the daemon itself). Processes whose
/// environment cannot be read (another user's, or gone meanwhile) are skipped.
pub(crate) fn find_run_processes(proc_root: &Path, run_id: &str, exclude: &[u32]) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| !exclude.contains(pid))
        .filter(|pid| carries_run_id(proc_root, *pid, run_id))
        .collect();
    pids.sort_unstable();
    pids
}

fn carries_run_id(proc_root: &Path, pid: u32, run_id: &str) -> bool {
    let Ok(environ) = std::fs::read(proc_root.join(pid.to_string()).join("environ")) else {
        return false;
    };
    let wanted = format!("PDO_RUN_ID={run_id}");
    environ
        .split(|byte| *byte == 0)
        .any(|var| var == wanted.as_bytes())
}

/// Stop every process left by `run_id`: `SIGTERM`, then `SIGKILL` for whatever
/// still runs after [`TERM_GRACE`]. Returns how many processes were signalled.
/// Blocking (it waits out the grace period): call it off the async workers.
#[cfg(target_os = "linux")]
pub(crate) fn terminate_run_processes(run_id: &str) -> usize {
    let proc_root = Path::new("/proc");
    let me = std::process::id();
    let pids = find_run_processes(proc_root, run_id, &[me]);
    for pid in &pids {
        signal(*pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + TERM_GRACE;
    loop {
        // Re-checked by environment, not by pid alone: a pid freed by an exited
        // orphan may already belong to an unrelated process.
        let remaining = find_run_processes(proc_root, run_id, &[me]);
        if remaining.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            for pid in &remaining {
                signal(*pid, libc::SIGKILL);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    pids.len()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn terminate_run_processes(_run_id: &str) -> usize {
    0
}

#[cfg(target_os = "linux")]
fn signal(pid: u32, sig: libc::c_int) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    // SAFETY: `kill` only sends a signal; an exited or foreign pid makes it fail
    // with ESRCH / EPERM, which is exactly "nothing to stop".
    unsafe {
        libc::kill(pid, sig);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_process(proc_root: &Path, pid: u32, environ: &[&str]) {
        let dir = proc_root.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes = Vec::new();
        for var in environ {
            bytes.extend_from_slice(var.as_bytes());
            bytes.push(0);
        }
        std::fs::write(dir.join("environ"), bytes).unwrap();
    }

    #[test]
    fn finds_exactly_the_runs_processes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_process(root, 10, &["HOME=/h", "PDO_RUN_ID=run-a", "PDO_NODE_ID=x"]);
        fake_process(root, 11, &["PDO_RUN_ID=run-a"]);
        // Another Run, a Run whose id merely starts the same, and no Run at all.
        fake_process(root, 12, &["PDO_RUN_ID=run-b"]);
        fake_process(root, 13, &["PDO_RUN_ID=run-a-child"]);
        fake_process(root, 14, &["HOME=/h"]);
        // A pid without a readable environ, and a non-pid entry.
        std::fs::create_dir_all(root.join("15")).unwrap();
        std::fs::create_dir_all(root.join("self")).unwrap();

        assert_eq!(find_run_processes(root, "run-a", &[]), vec![10, 11]);
    }

    #[test]
    fn never_lists_an_excluded_pid() {
        let tmp = tempfile::tempdir().unwrap();
        fake_process(tmp.path(), 20, &["PDO_RUN_ID=run-a"]);
        fake_process(tmp.path(), 21, &["PDO_RUN_ID=run-a"]);

        assert_eq!(find_run_processes(tmp.path(), "run-a", &[20]), vec![21]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn terminates_a_detached_process_of_the_run_and_spares_others() {
        let run_id = format!("orphan-test-{}", std::process::id());
        let spawn = |id: &str| {
            std::process::Command::new("sleep")
                .arg("60")
                .env("PDO_RUN_ID", id)
                .spawn()
                .unwrap()
        };
        let mut orphan = spawn(&run_id);
        let mut bystander = spawn(&format!("{run_id}-other"));

        assert_eq!(terminate_run_processes(&run_id), 1);
        // Stopped by SIGTERM: `wait` returns at once with a signal status.
        let status = orphan.wait().unwrap();
        assert!(!status.success());
        // The other Run's process is untouched.
        assert!(bystander.try_wait().unwrap().is_none());

        bystander.kill().unwrap();
        bystander.wait().unwrap();
    }
}
