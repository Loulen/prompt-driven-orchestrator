//! The review context over its real adapters — the run context over the
//! SQLite event store (the run context's own lifecycle harness), git in a
//! temporary repository — and the instance setting in memory. Driven through
//! its router merged with the run context's (a Run's refs and files are its
//! reads), behind the core's identification: the session headers make the
//! node's Agent, anything else is the local operator.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use pdo_kernel::actor::actor_from_headers;
use pdo_kernel::extension_host::NoExtensions;
use pdo_review::adapters::git::CliGit;
use pdo_review::adapters::runs::ContextRuns;
use pdo_review::application::ports::{BoxFuture, ReviewSettings};
use pdo_review::{Review, ReviewPorts};

/// The run context's lifecycle harness, shared rather than copied.
#[allow(dead_code, unused_imports)]
#[path = "../../../pdo-run/tests/lifecycle/harness.rs"]
mod run_harness;

pub use run_harness::event_log;
use run_harness::TestState as RunContext;

/// `review_agent_can_resolve`, as a test sets it.
#[derive(Default)]
pub struct AgentCanResolve(AtomicBool);

impl AgentCanResolve {
    pub fn set(&self, on: bool) {
        self.0.store(on, Ordering::SeqCst);
    }
}

impl ReviewSettings for AgentCanResolve {
    fn agent_can_resolve(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { self.0.load(Ordering::SeqCst) })
    }
}

/// The review context of one test, and the run context it reads.
pub struct TestState {
    pub db: sqlx::SqlitePool,
    pub run: Arc<RunContext>,
    pub review: Arc<Review>,
    pub settings: Arc<AgentCanResolve>,
}

fn over(run: Arc<RunContext>) -> Arc<TestState> {
    let settings = Arc::new(AgentCanResolve::default());
    let review = Review::new(ReviewPorts {
        runs: Arc::new(ContextRuns(run.run.clone())),
        git: Arc::new(CliGit),
        settings: settings.clone(),
        extensions: Arc::new(NoExtensions),
    });
    Arc::new(TestState {
        db: run.db.clone(),
        run,
        review,
        settings,
    })
}

/// A review context over a fresh temporary repository.
pub async fn test_state() -> Arc<TestState> {
    over(run_harness::test_state().await)
}

/// A review context over `dir`, the test's own repository.
pub async fn test_state_with_dir(dir: &Path) -> Arc<TestState> {
    over(run_harness::test_state_with_dir(dir).await)
}

async fn identify(mut request: Request, next: Next) -> Response {
    let actor = actor_from_headers(request.headers());
    request.extensions_mut().insert(actor);
    next.run(request).await
}

/// The review's routes and the run context's, behind the core's
/// identification.
pub fn build_router(state: Arc<TestState>) -> Router {
    pdo_run::router(state.run.run.clone())
        .merge(pdo_review::router(state.review.clone()))
        .layer(middleware::from_fn(identify))
}

/// Append `event` to its Run's log, as every in-process caller does.
pub async fn append_event(state: &TestState, event: &event_log::Event) -> anyhow::Result<()> {
    pdo_run::application::events::append_event(&state.run.run, event).await
}

/// Every event of `run_id`, in append order.
pub async fn load_events(
    db: &sqlx::SqlitePool,
    run_id: &str,
) -> anyhow::Result<Vec<event_log::Event>> {
    pdo_run::adapters::sqlite_event_store::load(db, run_id).await
}
