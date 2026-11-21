//! The `session` context: tmux on the daemon's private socket (or the one a
//! test hands in), the OS terminal window, and its Run-side ports over the
//! Runs, the platform's service health and the pipeline registry.

use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_pipeline::Pipelines;
use pdo_platform::Platform;
use pdo_run::Runs;
use pdo_session::adapters::hosted_store::FsHostedStore;
use pdo_session::adapters::terminal_window::OsTerminalWindow;
use pdo_session::application::hosted::HostedPorts;
use pdo_session::application::ports::Tmux;
use pdo_session::{Session, SessionOptions, SessionPorts};

use super::ContextResources;
use crate::ports::session::{
    GitCoreRepository, HostedSide, ManagedSkills, RunSide, SessionRunPorts, SupportSide,
    CORE_REPOSITORY_URL,
};
use crate::ports::Late;

pub(super) fn session(
    r: &ContextResources,
    tmux: Arc<dyn Tmux>,
    runs: &Late<Runs>,
    platform: &Late<Platform>,
    pipelines: &Late<Pipelines>,
    skills: &Arc<pdo_skill::Skills>,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Session> {
    let run = SessionRunPorts::over(RunSide {
        runs: runs.clone(),
        platform: platform.clone(),
        db: r.db.clone(),
        repo_root: r.repo_root.clone(),
        pipelines: pipelines.clone(),
        docker_cmd_override: r.docker_cmd_override.clone(),
    });
    // ADR-0092: the records and state of the Sessions hébergées live under the
    // daemon's `~/.pdo/`, never in the folder a session works in.
    let pdo_home = r.home_root.clone().unwrap_or_else(|| r.repo_root.clone());
    let hosted = Arc::new(HostedSide {
        runs: runs.clone(),
        db: r.db.clone(),
        home: r.home_root.clone(),
    });
    Session::new(
        SessionPorts {
            tmux,
            waits: run.waits,
            panes: run.panes,
            shells: run.shells,
            owners: run.owners,
            gauges: run.gauges,
            templates: run.templates,
            assistant_harness: run.assistant_harness,
            terminal_window: Arc::new(OsTerminalWindow),
            extensions: host,
            projects: run.projects,
            hosted: HostedPorts {
                store: Arc::new(FsHostedStore::new(pdo_home.join(".pdo").join("hosted"))),
                places: hosted.clone(),
                harness: hosted.clone(),
                admission: hosted,
            },
            support: Arc::new(SupportSide {
                runs: runs.clone(),
                db: r.db.clone(),
            }),
            events: r.bus.clone(),
            // The skills PDO manages, from the bank; the core's repository,
            // read at this binary's tag.
            support_skills: Arc::new(ManagedSkills {
                skills: skills.clone(),
            }),
            core_repository: Arc::new(GitCoreRepository {
                url: r
                    .core_source_url
                    .clone()
                    .unwrap_or_else(|| CORE_REPOSITORY_URL.to_string()),
            }),
        },
        SessionOptions {
            daemon_port: r.port,
            repo_root: r.repo_root.clone(),
            tmux_cmd_override: r.tmux_cmd_override.clone(),
            allowed_ws_origins: r.allowed_ws_origins.clone(),
            // One folder per Actor, under the daemon's `~/.pdo/`, like the
            // Sessions hébergées'.
            support_root: pdo_home.join(".pdo").join("support"),
            // The clones of the core, cloned at the first demand.
            core_source_root: pdo_home.join(".pdo").join("core-source"),
        },
    )
}
