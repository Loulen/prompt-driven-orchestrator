//! The `review` context: the Runs of the run context, git through the
//! workspace's adapter, and the instance setting of the settings context.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_review::adapters::git::CliGit;
use pdo_review::adapters::runs::ContextRuns;
use pdo_review::{Review, ReviewPorts};
use pdo_run::Runs;
use pdo_settings::adapters::contexts::SettingsOfReview;

use super::StoredSettings;

pub(super) fn review(
    run: &Arc<Runs>,
    stored: &StoredSettings,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Review> {
    Review::new(ReviewPorts {
        runs: Arc::new(ContextRuns(run.clone())),
        git: Arc::new(CliGit),
        settings: Arc::new(SettingsOfReview(stored.config.clone())),
        extensions: host,
    })
}
