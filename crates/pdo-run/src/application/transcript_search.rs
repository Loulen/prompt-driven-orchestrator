//! The search over the **Transcripts** of a whole Run (CONTEXT.md): every
//! node iteration whose transcript PDO reads, in the order of the nodes then of
//! their iterations, read through the harness context's reader
//! (`pdo_harness::adapters::transcript_reader`) — no persistent index, each
//! file indexed in memory once, then only what it gains.
//!
//! Bounded on every side: [`MAX_HITS`] excerpts at most (every match still
//! counted, for the « N in M nodes » and the canvas counts), excerpts of a few
//! lines, at most [`SEARCH_SLOTS`] searches reading at once, and a search
//! **cancelled** as soon as the same client asks a new one (the next keystroke)
//! or hangs up.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pdo_harness::adapters::transcript_reader::TranscriptReader;
use pdo_harness::transcript::{Needle, TranscriptHit};
use serde::Serialize;
use tokio::sync::Semaphore;
use ts_rs::TS;

use crate::application::node::transcript::{internal, RunTranscripts, TranscriptSource};
use crate::Runs;

/// The most excerpts a search answers.
pub const MAX_HITS: usize = 200;
/// The longest query, in characters.
pub const MAX_QUERY: usize = 200;
/// How many searches read transcripts at once; the others wait their turn, so
/// a burst of searches never takes the daemon's other reads' CPU.
pub const SEARCH_SLOTS: usize = 4;

/// A matching entry of one node iteration's transcript.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct RunTranscriptHit {
    pub node_id: String,
    pub iter: i64,
    pub hit: TranscriptHit,
}

/// How many entries of one node's transcripts (all its iterations) match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NodeTranscriptMatches {
    pub node_id: String,
    pub matched: u32,
}

/// The answer of a search over a Run's transcripts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct RunTranscriptSearch {
    pub query: String,
    /// The first [`MAX_HITS`] matching entries, by node then chronology.
    pub hits: Vec<RunTranscriptHit>,
    /// Every matching entry of the Run.
    pub matched: u32,
    /// The nodes with a match, in the order of the hits.
    pub nodes: Vec<NodeTranscriptMatches>,
    /// More entries match than the hits carry.
    pub truncated: bool,
    /// A newer search of the same client cancelled this one: what it carries
    /// is partial.
    pub cancelled: bool,
}

/// One transcript file to search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTarget {
    pub node_id: String,
    pub iter: i64,
    pub harness: String,
    pub path: PathBuf,
}

/// The node iterations a search visits, in order: the nodes as the pipeline
/// declares them (`declared`), then any other in name order, each by its
/// iterations ascending.
pub fn search_order(
    declared: &[&str],
    iterations: &HashMap<String, Vec<i64>>,
) -> Vec<(String, i64)> {
    let mut nodes: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|id| iterations.contains_key(*id))
        .collect();
    let mut others: Vec<&str> = iterations
        .keys()
        .map(String::as_str)
        .filter(|id| !declared.contains(id))
        .collect();
    others.sort_unstable();
    nodes.extend(others);
    nodes
        .into_iter()
        .flat_map(|id| {
            let mut iters = iterations[id].clone();
            iters.sort_unstable();
            iters.dedup();
            iters.into_iter().map(move |iter| (id.to_string(), iter))
        })
        .collect()
}

/// The transcript files of the Run's node iterations, in [`search_order`].
pub fn targets(run: &RunTranscripts) -> Vec<SearchTarget> {
    let iterations: HashMap<String, Vec<i64>> = run
        .run_state
        .nodes
        .iter()
        .map(|(id, node)| {
            let mut iters: Vec<i64> = node.iterations.iter().map(|i| i.iter).collect();
            if iters.is_empty() && node.iter > 0 {
                iters.push(node.iter);
            }
            (id.clone(), iters)
        })
        .collect();
    let declared: Vec<&str> = run
        .run_state
        .node_defs
        .iter()
        .map(|d| d.id.as_str())
        .collect();
    search_order(&declared, &iterations)
        .into_iter()
        .filter_map(|(node_id, iter)| match run.locate(&node_id, iter) {
            TranscriptSource::File { harness, path } => Some(SearchTarget {
                node_id,
                iter,
                harness,
                path,
            }),
            _ => None,
        })
        .collect()
}

/// Search `targets` in order with `reader`: the first `keep` hits kept, every
/// match counted. A file that is gone is skipped; `cancelled` stops the search
/// between entries and between files.
pub fn search_targets(
    reader: &TranscriptReader,
    targets: &[SearchTarget],
    needle: &Needle,
    keep: usize,
    cancelled: &dyn Fn() -> bool,
) -> std::io::Result<RunTranscriptSearch> {
    let mut answer = RunTranscriptSearch::default();
    for target in targets {
        if cancelled() {
            answer.cancelled = true;
            break;
        }
        let left = keep.saturating_sub(answer.hits.len());
        let found = match reader.search(&target.harness, &target.path, needle, left, cancelled) {
            Ok(found) => found,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        answer.cancelled |= found.cancelled;
        if found.matched > 0 {
            answer.matched += found.matched;
            match answer
                .nodes
                .iter_mut()
                .find(|n| n.node_id == target.node_id)
            {
                Some(node) => node.matched += found.matched,
                None => answer.nodes.push(NodeTranscriptMatches {
                    node_id: target.node_id.clone(),
                    matched: found.matched,
                }),
            }
            answer
                .hits
                .extend(found.hits.into_iter().map(|hit| RunTranscriptHit {
                    node_id: target.node_id.clone(),
                    iter: target.iter,
                    hit,
                }));
        }
        if answer.cancelled {
            break;
        }
    }
    answer.truncated = answer.matched as usize > answer.hits.len();
    Ok(answer)
}

/// The searches in flight, one per client: a client's new search cancels its
/// previous one.
#[derive(Default)]
pub struct SearchCancels {
    inflight: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl SearchCancels {
    /// The daemon's registry.
    pub fn shared() -> &'static SearchCancels {
        static CANCELS: OnceLock<SearchCancels> = OnceLock::new();
        CANCELS.get_or_init(SearchCancels::default)
    }

    /// Start a search for `client`, cancelling the one it had in flight.
    pub fn begin(&self, client: Option<&str>) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        if let Some(client) = client {
            let mut inflight = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(previous) = inflight.insert(client.to_string(), token.clone()) {
                previous.store(true, Ordering::Relaxed);
            }
        }
        token
    }

    /// The search `token` of `client` is over: forget it, unless a newer one
    /// already took its place.
    pub fn finish(&self, client: Option<&str>, token: &Arc<AtomicBool>) {
        let Some(client) = client else { return };
        let mut inflight = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        if inflight.get(client).is_some_and(|t| Arc::ptr_eq(t, token)) {
            inflight.remove(client);
        }
    }

    /// How many clients have a search in flight.
    pub fn in_flight(&self) -> usize {
        self.inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// Cancels its search when dropped: the request's future is dropped when the
/// client hangs up, and the read running on a blocking thread stops.
struct CancelOnDrop<'a> {
    token: Arc<AtomicBool>,
    client: Option<&'a str>,
}

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        self.token.store(true, Ordering::Relaxed);
        SearchCancels::shared().finish(self.client, &self.token);
    }
}

fn slots() -> &'static Semaphore {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| Semaphore::new(SEARCH_SLOTS))
}

/// `GET /runs/{run_id}/transcripts/search?q=&client=`: the entries of the
/// Run's transcripts holding `q`. `client` names the search box asking, so its
/// next search cancels this one.
pub async fn search(state: &Runs, run_id: &str, query: &str, client: Option<&str>) -> Response {
    if query.chars().count() > MAX_QUERY {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("the query is longer than {MAX_QUERY} characters") })),
        )
            .into_response();
    }
    let token = SearchCancels::shared().begin(client);
    let guard = CancelOnDrop {
        token: token.clone(),
        client,
    };
    let Some(needle) = Needle::new(query) else {
        return Json(RunTranscriptSearch::default()).into_response();
    };
    let run = match RunTranscripts::load(state, run_id).await {
        Ok(run) => run,
        Err(response) => return *response,
    };
    let targets = targets(&run);
    let Ok(_slot) = slots().acquire().await else {
        return internal("the search slots are closed".to_string());
    };
    let read = tokio::task::spawn_blocking(move || {
        search_targets(
            TranscriptReader::shared(),
            &targets,
            &needle,
            MAX_HITS,
            &|| token.load(Ordering::Relaxed),
        )
    })
    .await;
    drop(guard);
    match read {
        Ok(Ok(mut answer)) => {
            answer.query = query.trim().to_string();
            Json(answer).into_response()
        }
        Ok(Err(e)) => internal(format!("cannot read the transcripts: {e}")),
        Err(e) => internal(format!("transcript search panicked: {e}")),
    }
}

#[cfg(test)]
mod tests;
