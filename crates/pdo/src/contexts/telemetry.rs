//! The `telemetry` context: the OTLP exporters over the daemon's runtime, the
//! setting read off the stored tier, the Runs' facts and cost read from the
//! run context, the live sessions from the session context.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_run::adapters::disk_live_facts::DiskLiveFacts;
use pdo_run::Runs;
use pdo_session::Session;
use pdo_settings::adapters::contexts::OtlpSettingOfTelemetry;
use pdo_telemetry::adapters::{env, host, otlp::OtlpExporters};
use pdo_telemetry::application::runs::RunRecorder;
use pdo_telemetry::{Identity, Telemetry};

use super::{ContextResources, StoredSettings};
use crate::ports::telemetry_runs::{RunCostsOfRuns, RunFactsOfRuns, SessionsOfTelemetry};

pub(super) fn telemetry(
    r: &ContextResources,
    run: &Arc<Runs>,
    session: &Arc<Session>,
    stored: &StoredSettings,
    host: Arc<dyn ExtensionHost>,
) -> (Arc<Telemetry>, RunRecorder) {
    let telemetry = Arc::new(
        Telemetry::new(
            Arc::new(OtlpSettingOfTelemetry(stored.config.clone())),
            Arc::new(OtlpExporters::current()),
            Identity {
                instance: host::instance_id(r.port),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            env::export_interval(),
        )
        .with_sessions(Arc::new(SessionsOfTelemetry(session.clone())))
        .with_extensions(host),
    );
    let recorder = RunRecorder::new(
        telemetry.clone(),
        Arc::new(RunFactsOfRuns {
            runs: run.clone(),
            project_names: r.project_names.clone(),
            repo_root: r.repo_root.clone(),
        }),
        Arc::new(RunCostsOfRuns {
            runs: run.clone(),
            disk: Arc::new(DiskLiveFacts::new(
                r.repo_root.clone(),
                r.sandbox_home_override.clone(),
            )),
        }),
    );
    (telemetry, recorder)
}
