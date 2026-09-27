//! Panic isolation for the daemon's sweeps and for the detached tail of a
//! terminal gesture.

use std::sync::Arc;

use tracing::{error, warn};

use crate::application::events::append_event;
use crate::domain::event;
use crate::Runs;

/// Run one tick of a background sweep under panic isolation — the shared spine of
/// the daemon's three otherwise-unsupervised sweeps (Trigger scheduler, stale
/// detector, reaper).
///
/// Don't run any of them bare in a `loop { tick().await }`: a single panic silently
/// and permanently kills the task, and for the stale detector that means all stall /
/// idle / session-death detection goes dark for the daemon's remaining life with
/// nothing logged. Driving the future on its own `tokio::spawn` contains the panic at
/// that boundary, so the caller returns normally and the next tick recovers.
/// (`tokio::sync::Mutex` does not poison, so a per-tick lock is released on unwind.)
pub async fn run_isolated<F>(label: &str, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Err(e) = tokio::spawn(fut).await {
        if e.is_panic() {
            warn!("{label}: tick panicked, isolated — next tick recovers: {e}");
        }
    }
}

/// Park until the terminal-tail gate is released (test seam; a no-op unarmed). The
/// re-check between creating the `Notified` and awaiting it closes the
/// release-vs-register race.
pub async fn wait_node_done_tail_gate(state: &Runs) {
    while state
        .node_done_tail_gate_armed
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        let notified = state.node_done_tail_gate.notified();
        if !state
            .node_done_tail_gate_armed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            break;
        }
        notified.await;
    }
}

/// Detach the post-terminal-event tail of a CLI terminal handler
/// (`node_done` / `node_fail` / `node_skip`) onto its own task.
///
/// The tail MUST NOT run inline in the request future: the `pdo` CLI runs inside the
/// node's own tmux session, and the tail's first act is to reap that session —
/// killing the HTTP client of the in-flight request. hyper then cancels the request
/// future at its next `.await`, silently dropping the successor spawn, the end-port
/// finalization and the `RunFailed`/`RunSkipped` append.
///
/// The tail carries its OWN panic isolation rather than reusing `run_isolated`,
/// which only logs: a panic after the successor's `NodeStarted` or inside the
/// completion gate escapes the spawn reconciler, and a bare `tokio::spawn` would
/// swallow it into a never-awaited `JoinError`. Here it is logged AND surfaced as
/// `RunFailed`, never a silent stall.
pub fn detach_terminal_tail<F>(
    label: &'static str,
    state: Arc<Runs>,
    run_id: String,
    node_id: String,
    fut: F,
) where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        // MUST be the first instruction of the detached task: that position is what
        // makes the test discriminate detached from inline. No-op unarmed.
        wait_node_done_tail_gate(&state).await;
        let outcome =
            futures_util::future::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(fut)).await;
        if let Err(panic) = outcome {
            let msg = panic_payload_message(panic.as_ref());
            let reason =
                format!("run advance panicked after terminal event ({label}) for {node_id}: {msg}");
            error!("Run {run_id}: {reason}");
            let run_failed = event::Event {
                id: None,
                run_id: run_id.clone(),
                ts: pdo_kernel::clock::now_iso(),
                kind: event::EventKind::RunFailed,
                node_id: None,
                iter: None,
                payload: Some(serde_json::json!({ "reason": reason })),
            };
            if let Err(e) = append_event(&state, &run_failed).await {
                error!("Run {run_id}: failed to append RunFailed after detached-tail panic: {e}");
            }
        }
    });
}

/// Extract a human-readable message from a caught panic payload. `std::panic`'s
/// default payload is a `&str` or `String`; anything else is opaque.
pub fn panic_payload_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::domain::scheduler;
    use crate::domain::scheduler::dispatcher as scheduler_dispatcher;
    use pdo_pipeline::domain::pipeline;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A Run's events, its state and their projection, as these tests read them.
    mod event_log {
        pub use crate::domain::event::*;
        pub use crate::domain::projection::*;
        pub use crate::domain::state::*;
    }

    /// The shared spine of the panic-isolated sweeps: a panic inside
    /// the driven future is contained at the `tokio::spawn` boundary, so the
    /// `.await` returns normally instead of unwinding the caller — and a normal
    /// future still runs to completion through the same path. Reaching the
    /// assertions below at all proves containment (an un-isolated panic would
    /// unwind this test task).
    #[tokio::test]
    async fn run_isolated_contains_a_panic_and_still_runs_normal_work() {
        run_isolated("test", async { panic!("boom — must be contained") }).await;

        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        run_isolated("test", async move {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        })
        .await;
        assert!(
            ran.load(std::sync::atomic::Ordering::Relaxed),
            "a non-panicking future must run to completion through run_isolated"
        );
    }
}
