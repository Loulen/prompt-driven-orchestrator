//! The ports a context answers with what several other contexts hold, so no
//! single context can implement them: the session context's Run-side facts
//! (the Runs, the platform's service health, the templates of the pipeline
//! registry), the page mounts an archive releases, the Runs as the platform
//! sees them, the Triggers' fires the Stats count, the Projets' announcements,
//! the shared pipelines a Run's snapshot overwrites, the Projets the
//! pipelines' validations name, the Runs' facts and cost the telemetry
//! attaches its measures to.
//!
//! Some of them reach a context wired after the one that reads them (the
//! session context is wired before the Runs, which read its tmux): they hold a
//! [`Late`] handle, set once every context is wired.

pub(crate) mod pipeline_projects;
pub(crate) mod platform_runs;
pub(crate) mod project_events;
pub(crate) mod run_pages;
pub(crate) mod session;
pub(crate) mod source_pipelines;
pub(crate) mod telemetry_runs;
pub(crate) mod trigger_fires;

use std::sync::{Arc, OnceLock, Weak};

/// A context wired after the port that reads it. Holds it weakly, as the
/// contexts hold each other through their ports: the daemon owns the contexts,
/// a port never keeps one alive.
pub(crate) struct Late<T>(Arc<OnceLock<Weak<T>>>);

impl<T> Clone for Late<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Late<T> {
    pub(crate) fn new() -> Self {
        Self(Arc::new(OnceLock::new()))
    }

    /// Bind the context, once it is wired. A second call is ignored.
    pub(crate) fn set(&self, context: &Arc<T>) {
        let _ = self.0.set(Arc::downgrade(context));
    }

    /// The context, unless the daemon is not wired yet or shutting down.
    pub(crate) fn get(&self) -> Result<Arc<T>, String> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| "the daemon is shutting down".to_string())
    }
}
