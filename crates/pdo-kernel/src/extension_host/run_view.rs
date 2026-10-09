//! The port through which an extension's [`Host`](pdo_extension_api::Host)
//! sees a Run as people do (API 0.12): who has it open, what it cost.

use pdo_extension_api::{Presence, RunCostRead};

use super::Extensions;

/// What the run context shows the extensions of a Run (a port: the Runs of
/// the run context in the daemon).
pub trait RunViews: Send + Sync {
    /// The Actors who have `run_id` open, by order of arrival.
    fn presences(&self, run_id: &str) -> Vec<Presence>;

    /// The cost of `run_id`, as its Info tab shows it; `None` for a Run the
    /// daemon does not know.
    fn run_cost(&self, run_id: &str) -> RunCostRead;
}

impl Extensions {
    /// Plug in what shows the extensions' [`Host`](pdo_extension_api::Host)
    /// a Run (its Presences, its cost), once the daemon wired its contexts.
    /// Until then nobody is present and no cost is known.
    pub fn serve_run_views(&self, views: std::sync::Arc<dyn RunViews>) {
        if self.registry.run_views.set(views).is_err() {
            tracing::warn!("the views of the Runs were plugged twice; the second is ignored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::{Actor, Extension, Manifest, RunCost};
    use std::sync::Arc;

    struct Quiet;
    impl Extension for Quiet {
        fn manifest(&self) -> Manifest {
            Manifest::new("quiet", "Quiet", "0.0.0")
        }
    }

    struct OneRun;
    impl RunViews for OneRun {
        fn presences(&self, run_id: &str) -> Vec<Presence> {
            match run_id {
                "r1" => vec![Presence::new(Actor::LocalOperator, "t1")],
                _ => Vec::new(),
            }
        }
        fn run_cost(&self, run_id: &str) -> RunCostRead {
            let cost = (run_id == "r1").then(|| RunCost::new(0.25));
            Box::pin(std::future::ready(cost))
        }
    }

    #[tokio::test]
    async fn a_host_sees_the_runs_once_the_daemon_plugs_its_views() {
        let extensions = Extensions::register(vec![Box::new(Quiet)]).unwrap();
        let host = extensions.host_of("quiet");
        assert!(host.presences("r1").is_empty());
        assert_eq!(host.run_cost("r1").await, None);
        extensions.serve_run_views(Arc::new(OneRun));
        assert_eq!(
            host.presences("r1"),
            [Presence::new(Actor::LocalOperator, "t1")]
        );
        assert!(host.presences("r2").is_empty());
        assert_eq!(host.run_cost("r1").await, Some(RunCost::new(0.25)));
        assert_eq!(host.run_cost("r2").await, None);
    }
}
