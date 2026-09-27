//! What the run context needs from the outside world. The composition root
//! plugs an adapter into each port; the use cases only ever see the traits.

use std::future::Future;
use std::pin::Pin;

use crate::domain::event::Event;
use crate::domain::state::RunState;

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The append-only event log of every Run.
pub trait EventStore: Send + Sync {
    /// Record `event` at the end of its Run's log. Refused when the Run has
    /// been forgotten (ADR-0024), in the same step as the write.
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, anyhow::Result<()>>;
    /// Every event of `run_id`, in append order. Empty for an unknown Run.
    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>>;
    /// The id of every Run the log holds, newest first.
    fn run_ids(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>>;
    /// Whether a Run was started as a child of `run_id`. `true` when the log
    /// cannot tell: the caller then takes the slow, exact path.
    fn has_children<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, bool>;
}

/// What a Run's detail reads live from disk on top of its projection: its
/// lines-of-code delta, its cost, the run-scoped pipeline's graph. Never
/// fails: an unreadable source leaves its field as the projection had it.
pub trait LiveFacts: Send + Sync {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()>;
}

/// The Pipeline Manager's sessions, as a Run's detail observes them.
pub trait ManagerSessions: Send + Sync {
    /// Whether the manager of `run_id` has a live session right now.
    fn exists(&self, run_id: &str) -> bool;
}
