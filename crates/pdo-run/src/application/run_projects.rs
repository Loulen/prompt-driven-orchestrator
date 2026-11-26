//! The Projet of each Run, as an extension's
//! [`Host::run_project`](pdo_extension_api::Host::run_project) reads it (API
//! 0.34): answered from memory, loaded from the Résumés before the daemon
//! serves, then kept current by [`AppendEvent`](super::append_event::AppendEvent)
//! and [`Runs::forget_run`].

use tracing::warn;

use crate::Runs;

impl Runs {
    /// The Projet `run_id` was launched in, the `project_id` `GET /runs/{id}`
    /// shows; `None` for a Run the daemon does not know (or forgot) and a Run
    /// in no Projet. Synchronous and quick: never reads the store.
    pub fn run_project(&self, run_id: &str) -> Option<String> {
        self.run_projects.project_of(run_id)
    }

    /// Load the Projet of every Run from the Résumés (ADR-0084), before the
    /// daemon serves. A store that cannot be read leaves the index as it is:
    /// the Runs already there answer `None` until the daemon restarts, while each
    /// new Run is still recorded.
    pub async fn load_run_projects(&self) {
        match self.store.summaries().await {
            Ok(summaries) => self.run_projects.fill(
                summaries
                    .into_iter()
                    .filter_map(|summary| Some((summary.run_id, summary.project_id?))),
            ),
            Err(e) => warn!("the Projets of the Runs cannot be loaded: {e}"),
        }
    }
}
