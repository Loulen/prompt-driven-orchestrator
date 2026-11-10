//! The pipeline context's `RenameLedger`, over this context: a rename that
//! changes a Pipeline's id leaves an absorption behind (ADR-0077 §4, #891).

use std::sync::Arc;

use pdo_pipeline::application::ports::{BoxFuture, RenameLedger, Renamed};

use crate::domain::absorption::Named;
use crate::Stats;

/// The Stats absorptions a Pipeline rename is recorded in.
pub struct StatsRenames(pub Arc<Stats>);

impl RenameLedger for StatsRenames {
    fn record<'a>(&'a self, rename: &'a Renamed) -> BoxFuture<'a, ()> {
        Box::pin(self.0.record_rename().best_effort(
            Named {
                key: rename.old_id.clone(),
                name: rename.old_name.clone(),
            },
            Named {
                key: rename.new_id.clone(),
                name: rename.new_name.clone(),
            },
        ))
    }
}
