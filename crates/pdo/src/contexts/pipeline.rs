//! The `pipeline` context: the instance registry and the node library on
//! disk, the daemon's live stream, the skill bank, the Stats absorption a
//! rename leaves behind, the Runs that still use a pipeline, and the
//! validations of the pipelines over SQLite with the Projets they name, and
//! their shared drafts (SQLite, `yrs`) and versions (SQLite).

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::adapters::draft_doc::YDraftDocuments;
use pdo_pipeline::adapters::events::BroadcastPipelineEvents;
use pdo_pipeline::adapters::library_fs::FsNodeLibrary;
use pdo_pipeline::adapters::registry::FsPipelineRegistry;
use pdo_pipeline::adapters::skill_bank::BankOfSkills;
use pdo_pipeline::adapters::sqlite_drafts::SqliteDrafts;
use pdo_pipeline::adapters::sqlite_validations::SqlitePipelineValidations;
use pdo_pipeline::adapters::sqlite_versions::SqlitePipelineVersions;
use pdo_pipeline::{Pipelines, PipelinesPorts};
use pdo_run::adapters::pipeline_runs::PipelineRuns;
use pdo_run::Runs;
use pdo_skill::Skills;
use pdo_stats::adapters::rename_ledger::StatsRenames;
use pdo_stats::Stats;
use pdo_workspace::adapters::sqlite::SqliteProjects;

use super::ContextResources;
use crate::ports::pipeline_projects::WorkspaceKnownProjects;

pub(super) fn pipelines(
    r: &ContextResources,
    run: &Arc<Runs>,
    stats: &Arc<Stats>,
    skills: &Arc<Skills>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Pipelines> {
    let pipelines = Arc::new(Pipelines::new(PipelinesPorts {
        registry: Arc::new(FsPipelineRegistry::new(
            r.repo_root.clone(),
            r.self_writes.clone(),
        )),
        library: Arc::new(FsNodeLibrary::home()),
        events: Arc::new(BroadcastPipelineEvents(r.bus_tx.clone())),
        runs: Arc::new(PipelineRuns(run.store().clone())),
        renames: Arc::new(StatsRenames(stats.clone())),
        skills: Arc::new(BankOfSkills(skills.clone())),
        validations: Arc::new(SqlitePipelineValidations::new(r.db.clone())),
        projects: Arc::new(WorkspaceKnownProjects(Arc::new(SqliteProjects::new(
            r.db.clone(),
        )))),
        drafts: Arc::new(SqliteDrafts::new(r.db.clone())),
        documents: Arc::new(YDraftDocuments),
        versions: Arc::new(SqlitePipelineVersions::new(r.db.clone())),
        extensions: host,
    }));
    // ADR-0088: a written file lands in its draft. Outside a runtime (the
    // composition root's own sync tests), nothing follows.
    if tokio::runtime::Handle::try_current().is_ok() {
        pdo_pipeline::adapters::draft_follow::spawn(pipelines.clone(), r.bus_tx.subscribe());
    }
    pipelines
}
