//! The port of the review context that what is still legacy implements: the
//! `review_agent_can_resolve` instance setting (the settings context).

use pdo_review::adapters::env::agent_can_resolve_with;
use pdo_review::application::ports::{BoxFuture, ReviewSettings};

use crate::instance_config;

/// `review_agent_can_resolve`, read FRESH at every call (never cached at
/// boot) through `stored → env → default(false)`.
pub(crate) struct LegacyReviewSettings {
    pub(crate) db: sqlx::SqlitePool,
}

impl ReviewSettings for LegacyReviewSettings {
    fn agent_can_resolve(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let stored = instance_config::get(&self.db)
                .await
                .ok()
                .and_then(|c| c.review_agent_can_resolve);
            agent_can_resolve_with(stored)
        })
    }
}
