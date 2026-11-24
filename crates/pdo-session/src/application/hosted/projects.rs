//! The Projets a session's Operations are in: a Run's, and those of a Session
//! hébergée by its record.

use std::sync::Arc;

use super::HostedRegistry;
use crate::application::ports::{BoxFuture, RunProjects};

/// The Projets of the Runs, and those of the Sessions hébergées by their
/// record.
pub(crate) struct WithHosted {
    pub(crate) runs: Arc<dyn RunProjects>,
    pub(crate) hosted: Arc<HostedRegistry>,
}

impl RunProjects for WithHosted {
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>> {
        self.runs.projects_of_run(run_id)
    }

    fn projects_of_hosted(&self, session: &str) -> Vec<String> {
        self.hosted.projects_of(session)
    }
}
