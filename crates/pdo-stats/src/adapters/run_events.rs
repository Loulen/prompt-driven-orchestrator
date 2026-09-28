//! [`RunEvents`] over the run context's ports: its `EventWindows` for the
//! cross-Run reads, its `EventStore` for one Run's events. Stats never reads
//! the event log's table itself.

use std::collections::BTreeSet;
use std::sync::Arc;

use pdo_run::application::ports::{EventStore, EventWindow, EventWindows, NodeStart, RunStart};
use pdo_run::domain::event::Event;

use crate::application::ports::{BoxFuture, RunEvents};
use crate::domain::absorption::UNKNOWN_PIPELINE;

/// The Runs' event log, through the run context.
pub struct RunContextEvents {
    store: Arc<dyn EventStore>,
    windows: Arc<dyn EventWindows>,
}

impl RunContextEvents {
    pub fn new(store: Arc<dyn EventStore>, windows: Arc<dyn EventWindows>) -> Self {
        Self { store, windows }
    }
}

impl RunEvents for RunContextEvents {
    fn count_by_period<'a>(
        &'a self,
        kind: &'a str,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<(String, i64)>>> {
        self.windows.count_by_period(kind, bucket_format, window)
    }

    fn run_starts<'a>(
        &'a self,
        bucket_format: Option<&'a str>,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<RunStart>>> {
        self.windows.run_starts(bucket_format, window)
    }

    fn node_starts<'a>(
        &'a self,
        bucket_format: &'a str,
        window: EventWindow<'a>,
    ) -> BoxFuture<'a, anyhow::Result<Vec<NodeStart>>> {
        self.windows.node_starts(bucket_format, window)
    }

    fn nodes_started_under<'a>(
        &'a self,
        pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<BTreeSet<String>>> {
        self.windows.nodes_started_under(pipeline, UNKNOWN_PIPELINE)
    }

    fn latest_run_start<'a>(
        &'a self,
        pipeline: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<serde_json::Value>>> {
        self.windows.latest_run_start(pipeline, UNKNOWN_PIPELINE)
    }

    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>> {
        self.store.events(run_id)
    }
}
