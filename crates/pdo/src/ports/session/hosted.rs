//! The ports of the Sessions hébergées (ADR-0092) that read the other
//! contexts: the primary repository of a Projet (workspace), the harness an
//! agent runs on and the Cap de sessions concurrentes (the Runs').

use std::path::{Path, PathBuf};

use pdo_harness::registry::HarnessDescriptor;
use pdo_run::Runs;
use pdo_session::application::hosted::{
    AdmissionSlot, HostedHarness, HostedPlaces, NotAdmitted, SessionAdmission,
};
use pdo_session::application::ports::BoxFuture;

use crate::ports::Late;

/// What the Sessions hébergées read of the daemon.
pub(crate) struct HostedSide {
    pub(crate) runs: Late<Runs>,
    pub(crate) db: sqlx::SqlitePool,
    /// The daemon user's home (the test seam's in the tests).
    pub(crate) home: Option<PathBuf>,
}

impl HostedPlaces for HostedSide {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn project_repo<'a>(&'a self, project: &'a str) -> BoxFuture<'a, Option<PathBuf>> {
        Box::pin(async move {
            let project = pdo_workspace::adapters::sqlite::get(&self.db, project)
                .await
                .ok()
                .flatten()?;
            project.default_primary().map(PathBuf::from)
        })
    }
}

impl HostedHarness for HostedSide {
    /// The harness named, else the instance's default one (the `claude`
    /// floor), resolved against the descriptor file like the library
    /// assistant's.
    fn resolve<'a>(&'a self, name: Option<&'a str>) -> BoxFuture<'a, Option<HarnessDescriptor>> {
        Box::pin(async move {
            let Ok(runs) = self.runs.get() else {
                return name.is_none().then(pdo_harness::registry::claude);
            };
            let home_root = pdo_run::application::sandbox::sandbox_home_roots(&runs)
                .map(|(home, _)| home)
                .unwrap_or_default();
            let registry = pdo_harness::adapters::descriptor_file::load(&home_root);
            match name.filter(|n| !n.trim().is_empty()) {
                Some(name) => registry.resolve(name),
                None => {
                    let default_harness =
                        pdo_run::application::settings::stored_default_harness(&runs).await;
                    let name = pdo_harness::resolver::resolve_infra_harness(
                        None,
                        default_harness.as_deref(),
                    );
                    Some(
                        registry
                            .resolve(&name)
                            .unwrap_or_else(pdo_harness::registry::claude),
                    )
                }
            }
        })
    }

    /// Read where the Runs read a Node's transcript, on the host: a harness
    /// that declares no transcript tells nothing.
    fn conversation_written<'a>(
        &'a self,
        harness: &'a str,
        cwd: &'a Path,
        id: &'a str,
        claude_config_dir: Option<&'a Path>,
    ) -> BoxFuture<'a, Option<bool>> {
        Box::pin(async move {
            let home = self.harness_home()?;
            conversation_written_under(&home, harness, cwd, id, claude_config_dir)
        })
    }
}

/// Whether the conversation `id` of `harness` in `cwd` is written under the
/// harnesses' `home`, or, for a launch given a Claude Code configuration folder
/// (API 0.8), under that folder's `projects/` alone: `claude --resume` looks
/// nowhere else, so a transcript found under `home` would not resume. `None`
/// for a harness that declares no transcript.
fn conversation_written_under(
    home: &Path,
    harness: &str,
    cwd: &Path,
    id: &str,
    claude_config_dir: Option<&Path>,
) -> Option<bool> {
    pdo_harness::domain::probes::resolved(harness).transcript_resolution()?;
    let claude_root = match claude_config_dir {
        Some(dir) => dir.join("projects"),
        None => home.join(".claude").join("projects"),
    };
    let stores = pdo_sandbox::adapters::staging::stores::HarnessStores::from_home(home);
    let transcript = pdo_harness::adapters::transcripts::resolve_transcript(
        harness,
        stores.root_for(harness, &claude_root),
        cwd,
        Some(id),
    );
    Some(transcript.is_some_and(|path| path.is_file()))
}

impl HostedSide {
    /// The home the harnesses keep their conversations under.
    fn harness_home(&self) -> Option<PathBuf> {
        let runs = self.runs.get().ok()?;
        pdo_run::application::sandbox::sandbox_home_roots(&runs)
            .ok()
            .map(|(home, _)| home)
    }
}

impl SessionAdmission for HostedSide {
    /// The Runs' admission lock, held from the count until the session is
    /// recorded: the Nodes and the Sessions hébergées share the Cap.
    fn admit(&self) -> BoxFuture<'_, Result<AdmissionSlot, NotAdmitted>> {
        Box::pin(async move {
            let runs = self.runs.get().map_err(NotAdmitted::Unavailable)?;
            let guard = runs.admission_lock_owned().lock_owned().await;
            let stored = pdo_run::application::settings::stored_session_cap(&runs).await;
            let cap = pdo_run::adapters::env::configured_cap_with(stored);
            let live =
                pdo_run::application::admission_count::count_global_live_sessions(&runs).await;
            if pdo_run::domain::admission::can_admit(live, cap) {
                Ok(AdmissionSlot::holding(guard))
            } else {
                Err(NotAdmitted::CapReached { live, cap })
            }
        })
    }

    /// A freed slot wakes the Nodes waiting for one, as stopping a Node does.
    fn released(&self) {
        let Ok(runs) = self.runs.get() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                pdo_run::application::scheduler::node_completion::retry_waiting_nodes(&runs).await
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_harness::domain::claude_transcript::encode_working_dir;

    /// Write the transcript of conversation `id` in `cwd` under `projects`.
    fn transcript(projects: &Path, cwd: &Path, id: &str) {
        let dir = projects.join(encode_working_dir(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.jsonl")), "{}\n").unwrap();
    }

    #[test]
    fn a_conversation_under_the_given_claude_config_dir_is_written() {
        let (home, cfg) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let cwd = Path::new("/work/folder");
        transcript(&cfg.path().join("projects"), cwd, "c1");
        assert_eq!(
            conversation_written_under(home.path(), "claude", cwd, "c1", Some(cfg.path())),
            Some(true)
        );
    }

    #[test]
    fn a_conversation_only_under_the_home_is_not_written_for_a_given_claude_config_dir() {
        let (home, cfg) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let cwd = Path::new("/work/folder");
        transcript(&home.path().join(".claude").join("projects"), cwd, "c1");
        assert_eq!(
            conversation_written_under(home.path(), "claude", cwd, "c1", Some(cfg.path())),
            Some(false),
            "`claude --resume` with that folder would not find it"
        );
    }

    #[test]
    fn without_a_claude_config_dir_the_conversation_is_read_under_the_home() {
        let home = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/folder");
        transcript(&home.path().join(".claude").join("projects"), cwd, "c1");
        assert_eq!(
            conversation_written_under(home.path(), "claude", cwd, "c1", None),
            Some(true)
        );
        assert_eq!(
            conversation_written_under(home.path(), "claude", cwd, "c2", None),
            Some(false)
        );
    }
}
