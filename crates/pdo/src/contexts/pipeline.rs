//! The `pipeline` context: the instance registry and the node library on
//! disk, the daemon's live stream, the skill bank, the Stats absorption a
//! rename leaves behind, and the Runs that still use a pipeline.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::adapters::events::BroadcastPipelineEvents;
use pdo_pipeline::adapters::library_fs::FsNodeLibrary;
use pdo_pipeline::adapters::registry::FsPipelineRegistry;
use pdo_pipeline::adapters::skill_bank::BankOfSkills;
use pdo_pipeline::{Pipelines, PipelinesPorts};
use pdo_run::adapters::pipeline_runs::PipelineRuns;
use pdo_run::Runs;
use pdo_skill::Skills;
use pdo_stats::adapters::rename_ledger::StatsRenames;
use pdo_stats::Stats;

use super::ContextResources;

pub(super) fn pipelines(
    r: &ContextResources,
    run: &Arc<Runs>,
    stats: &Arc<Stats>,
    skills: &Arc<Skills>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Pipelines> {
    Arc::new(Pipelines::new(PipelinesPorts {
        registry: Arc::new(FsPipelineRegistry::new(
            r.repo_root.clone(),
            r.self_writes.clone(),
        )),
        library: Arc::new(FsNodeLibrary::home()),
        events: Arc::new(BroadcastPipelineEvents(r.bus_tx.clone())),
        runs: Arc::new(PipelineRuns(run.store().clone())),
        renames: Arc::new(StatsRenames(stats.clone())),
        skills: Arc::new(BankOfSkills(skills.clone())),
        extensions: host,
    }))
}
