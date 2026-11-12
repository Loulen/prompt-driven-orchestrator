//! What a reader of the Stats may count: the Runs it may read, with the same
//! filter as the list of the Runs (`run` × `read` in the Projet recorded at
//! the Run's launch), and the Triggers it may read (`trigger` × `read` in the
//! Projet it launches its Runs in). A Projet's name only shows beside a Run,
//! so a reader never sees the name of a Projet it reads no Run of.
//!
//! Without extension nothing is asked, and nothing is narrowed: the three
//! sections read exactly as before.

use std::collections::BTreeSet;

use pdo_kernel::authorization::authorize_read;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_run::application::read_access::read_run;

use super::ports::{RunEvents, TriggerFires};

/// The Runs `actor` may read, `None` when it may read them all because no
/// extension asks.
pub(crate) async fn readable_runs(
    host: &dyn ExtensionHost,
    events: &dyn RunEvents,
    actor: &Actor,
) -> anyhow::Result<Option<BTreeSet<String>>> {
    if host.allows_everything() {
        return Ok(None);
    }
    let runs = events.run_projects().await?;
    Ok(Some(
        runs.into_iter()
            .filter(|run| {
                let read = read_run(&run.run_id).in_projects(run.project_id.as_deref());
                authorize_read(host, actor, &read).is_ok()
            })
            .map(|run| run.run_id)
            .collect(),
    ))
}

/// The Triggers `actor` may read, `None` when it may read them all because
/// no extension asks.
pub(crate) async fn readable_triggers(
    host: &dyn ExtensionHost,
    fires: &dyn TriggerFires,
    actor: &Actor,
) -> anyhow::Result<Option<BTreeSet<String>>> {
    if host.allows_everything() {
        return Ok(None);
    }
    let triggers = fires.trigger_projects().await?;
    Ok(Some(
        triggers
            .into_iter()
            .filter(|(id, project)| {
                let read = Operation::new(ResourceKind::Trigger, Verb::Read)
                    .on(id.as_str())
                    .in_projects(project.as_deref());
                authorize_read(host, actor, &read).is_ok()
            })
            .map(|(id, _)| id)
            .collect(),
    ))
}
