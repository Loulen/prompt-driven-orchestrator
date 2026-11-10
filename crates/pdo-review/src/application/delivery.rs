//! The delivery of a batch to the Pipeline Manager, off the request (CORE-239):
//! it watches the manager's session until the batch is answered, types the
//! batch into a running manager once its screen can take it, records a prompt
//! that holds it (`blocked`) and a session that ends before answering
//! (`exited`) as `review_comment_delivery` events, so the thread says them.

use std::sync::Arc;
use std::time::Duration;

use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::review_comment::ReviewCommentStatus;
use serde_json::json;
use tokio::time::Instant;
use tracing::{error, info};

use super::ports::ReviewRuns;
use crate::domain::comments::delivery::{Step, Watch};

/// The grace before the first look: a paste right after the send, as before.
const FIRST_LOOK: Duration = Duration::from_millis(300);
/// How often the session is looked at while the batch is in flight, and for
/// the first minutes after (a launch prompt shows its trust check early).
const CLOSE_LOOK: Duration = Duration::from_secs(1);
const CLOSE_FOR: Duration = Duration::from_secs(180);
/// How often once the batch is with the manager: only its end matters.
const DISTANT_LOOK: Duration = Duration::from_secs(5);
/// How often the log is read to see whether every comment was answered.
const ANSWER_CHECK: Duration = Duration::from_secs(5);
/// A manager that neither answers nor ends is left alone after this long: a
/// thread whose manager is gone still says so on the next read of the page.
const GIVE_UP: Duration = Duration::from_secs(6 * 3600);

/// One batch on its way: its comments, and the message still to type into a
/// manager that was already running (`None` when the send started it with
/// the batch in its launch prompt).
pub struct Handover {
    pub run_id: String,
    pub ids: Vec<String>,
    pub paste: Option<String>,
}

/// Follow `handover` on a task of its own.
pub fn spawn(runs: Arc<dyn ReviewRuns>, handover: Handover) {
    tokio::spawn(follow(runs, handover));
}

/// Watch the manager's session until every comment of the batch is answered,
/// the session ends, or [`GIVE_UP`].
pub async fn follow(runs: Arc<dyn ReviewRuns>, handover: Handover) {
    let Handover { run_id, ids, paste } = handover;
    let mut watch = Watch::new(paste.is_some());
    let started = Instant::now();
    let mut answers_checked = Instant::now();
    tokio::time::sleep(FIRST_LOOK).await;
    loop {
        let screen = runs.manager_screen(&run_id);
        for step in watch.observe(screen.as_deref()) {
            match step {
                Step::Paste => {
                    if let Some(message) = &paste {
                        runs.paste_to_manager(&run_id, message);
                    }
                }
                Step::Record { state, reason } => {
                    let open = unanswered(runs.as_ref(), &run_id, &ids).await;
                    if open.is_empty() {
                        return;
                    }
                    record(runs.as_ref(), &run_id, &open, state, reason).await;
                }
                Step::Stop => return,
            }
        }
        if answers_checked.elapsed() >= ANSWER_CHECK {
            answers_checked = Instant::now();
            if !watch.in_flight() && unanswered(runs.as_ref(), &run_id, &ids).await.is_empty() {
                return;
            }
        }
        if started.elapsed() >= GIVE_UP {
            info!("run {run_id}: stopped following review comments {ids:?} — no answer, the manager still up");
            return;
        }
        let close = watch.in_flight() || started.elapsed() < CLOSE_FOR;
        tokio::time::sleep(if close { CLOSE_LOOK } else { DISTANT_LOOK }).await;
    }
}

/// The comments of `ids` still waiting for the manager: sent, not answered.
/// An unreadable log keeps them all — saying too much beats saying nothing.
async fn unanswered(runs: &dyn ReviewRuns, run_id: &str, ids: &[String]) -> Vec<String> {
    let Ok(Some(run)) = runs.load(run_id).await else {
        return ids.to_vec();
    };
    ids.iter()
        .filter(|id| {
            run.state.review_comments.iter().any(|c| {
                &c.id == *id && c.status == ReviewCommentStatus::Sent && c.replies.is_empty()
            })
        })
        .cloned()
        .collect()
}

/// Append one `review_comment_delivery` for `ids`.
async fn record(
    runs: &dyn ReviewRuns,
    run_id: &str,
    ids: &[String],
    state: &str,
    reason: Option<String>,
) {
    let mut payload = json!({ "ids": ids, "state": state });
    if let Some(reason) = reason {
        payload["reason"] = json!(reason);
    }
    let event = Event {
        id: None,
        run_id: run_id.to_string(),
        ts: pdo_kernel::clock::now_iso(),
        kind: EventKind::ReviewCommentDelivery,
        node_id: None,
        iter: None,
        payload: Some(payload),
    };
    match runs.append(&event).await {
        Ok(()) => {
            info!("run {run_id}: review comments {ids:?} {state} on their way to the manager")
        }
        Err(e) => error!("run {run_id}: failed to record review_comment_delivery {state}: {e}"),
    }
}
