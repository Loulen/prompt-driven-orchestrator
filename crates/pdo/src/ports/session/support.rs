//! The ports of the Agent de support that read the other contexts: which
//! agent a choice resolves to, over the agentic profiles (settings), the
//! harness registry and the binary probe (harness), and the instance's
//! default models (the Runs'); the instance's idle timeout (settings); the
//! skills it receives (the bank's); the public repository of the core (git,
//! through the workspace's adapter).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pdo_harness::domain::agent_choice::AgentChoice;
use pdo_run::Runs;
use pdo_session::application::ports::BoxFuture;
use pdo_session::application::support::{
    ChoiceRefusal, CoreRepository, ResolvedAgent, SupportAgents, SupportSkills,
};
use pdo_workspace::adapters::git::clone::{clone_shallow, remote_has_tag, CloneError};

use crate::ports::Late;

/// What the Agents de support read of the daemon.
pub(crate) struct SupportSide {
    pub(crate) runs: Late<Runs>,
    pub(crate) db: sqlx::SqlitePool,
}

impl SupportAgents for SupportSide {
    /// A profile by its id, or the custom combination; its harness resolved
    /// against the descriptor file and its binary found on the harness probe
    /// `PATH`; without a model, the instance's default one for that harness.
    fn resolve<'a>(
        &'a self,
        choice: &'a AgentChoice,
    ) -> BoxFuture<'a, Result<ResolvedAgent, ChoiceRefusal>> {
        Box::pin(async move {
            let (label, harness, model, effort) = match choice {
                AgentChoice::Inherit => return Err(ChoiceRefusal::Inherit),
                AgentChoice::Profile { profile_id } => {
                    let profile = pdo_settings::adapters::sqlite::agent_profiles::get(
                        &self.db,
                        profile_id.trim(),
                    )
                    .await
                    .map_err(|e| ChoiceRefusal::Unreadable(e.to_string()))?
                    .ok_or_else(|| ChoiceRefusal::UnknownProfile(profile_id.clone()))?;
                    (profile.name, profile.harness, profile.model, profile.effort)
                }
                AgentChoice::Custom {
                    harness,
                    model,
                    effort,
                } => (
                    "Custom".to_string(),
                    harness.clone(),
                    model.clone(),
                    effort.clone(),
                ),
            };
            let name = harness.trim();
            let runs = self.runs.get().ok();
            let home_root = runs
                .as_ref()
                .and_then(|runs| pdo_run::application::sandbox::sandbox_home_roots(runs).ok())
                .map(|(home, _)| home)
                .unwrap_or_default();
            let registry = pdo_harness::adapters::descriptor_file::load(&home_root);
            let descriptor = registry
                .resolve(name)
                .ok_or_else(|| ChoiceRefusal::UnknownHarness(name.to_string()))?;
            if !pdo_harness::adapters::binary_path::binary_available(&descriptor.binary) {
                return Err(ChoiceRefusal::NotInstalled(descriptor.name));
            }
            let model = match filled(model) {
                Some(model) => Some(model),
                None => match &runs {
                    Some(runs) => {
                        pdo_run::application::settings::stored_default_harness_models(runs)
                            .await
                            .remove(&descriptor.name)
                    }
                    None => None,
                },
            };
            Ok(ResolvedAgent {
                harness: descriptor,
                model,
                effort: filled(effort),
                label,
            })
        })
    }

    /// `stored → env → default`, read at every tick: a failed read falls to
    /// the env and default tiers.
    fn idle_timeout(&self) -> BoxFuture<'_, Duration> {
        Box::pin(async move {
            let stored = pdo_settings::adapters::sqlite::instance_config::get(&self.db)
                .await
                .ok()
                .and_then(|c| c.support_idle_timeout_secs)
                .map(|n| n.max(0) as u64);
            pdo_session::adapters::env::support_idle_timeout_with(stored)
        })
    }
}

/// A field that says something: trimmed, `None` when blank.
fn filled(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The skills an Agent de support receives: those PDO manages (its own and
/// its extensions'), from the bank.
pub(crate) struct ManagedSkills {
    pub(crate) skills: Arc<pdo_skill::Skills>,
}

impl SupportSkills for ManagedSkills {
    fn deliver<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            let report = self
                .skills
                .deliver_managed(dir)
                .await
                .map_err(|e| format!("{e:#}"))?;
            for skipped in &report.skipped {
                tracing::warn!(
                    "Support agent: skill `{}` not delivered: {}",
                    skipped.name,
                    skipped.reason
                );
            }
            Ok(report.delivered)
        })
    }
}

/// The public repository of the core, where its releases are tagged.
pub(crate) const CORE_REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");

/// How long asking the repository for a tag may take: it is asked when an
/// agent opens, once per daemon.
const TAG_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the clone of the core may take.
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);

/// The repository of the core at `url`, through git: no credential, nothing
/// prompts.
pub(crate) struct GitCoreRepository {
    pub(crate) url: String,
}

impl CoreRepository for GitCoreRepository {
    fn has_tag<'a>(&'a self, tag: &'a str) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move { remote_has_tag(&self.url, tag, &BTreeMap::new(), TAG_TIMEOUT).await })
    }

    /// Cloned beside `dest`, then renamed: a failure or a daemon stopped
    /// mid-clone never leaves a half clone where the cache looks. What such a
    /// clone left beside `dest`, and a `dest` that is no clone (its `.git`
    /// gone), are removed first: the cache is the daemon's.
    fn clone_at<'a>(&'a self, tag: &'a str, dest: &'a Path) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let (Some(parent), Some(name)) = (dest.parent(), dest.file_name()) else {
                return Err(format!("{} names no folder to clone into", dest.display()));
            };
            let name = name.to_string_lossy();
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            let leftover = format!(".{name}.cloning-");
            for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
                if entry.file_name().to_string_lossy().starts_with(&leftover) {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
            if dest.exists() {
                std::fs::remove_dir_all(dest)
                    .map_err(|e| format!("could not clear {}: {e}", dest.display()))?;
            }
            let staging = parent.join(format!("{leftover}{}", std::process::id()));
            // Held to the end: a dropped sender would cancel the clone.
            let (_cancel, cancelled) = tokio::sync::oneshot::channel();
            let cloned = clone_shallow(
                &self.url,
                Some(tag),
                &staging,
                &BTreeMap::new(),
                cancelled,
                CLONE_TIMEOUT,
            )
            .await
            .map_err(|e| match e {
                CloneError::Failed(stderr) => stderr.trim().to_string(),
                CloneError::Timeout => {
                    format!("the clone took more than {}s", CLONE_TIMEOUT.as_secs())
                }
                CloneError::Cancelled => "the clone was cancelled".to_string(),
                CloneError::Io(why) => why,
            })
            .and_then(|()| match std::fs::rename(&staging, dest) {
                Ok(()) => Ok(()),
                // Another daemon sharing this home put its clone there first.
                Err(_) if dest.join(".git").exists() => Ok(()),
                Err(e) => Err(format!(
                    "could not move the clone to {}: {e}",
                    dest.display()
                )),
            });
            let _ = std::fs::remove_dir_all(&staging);
            cloned
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository with one commit tagged `v1.0.0`.
    fn origin(dir: &Path) -> GitCoreRepository {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("lib.rs"), "fn core() {}\n").unwrap();
        git(dir, &["add", "."]);
        git(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "core",
            ],
        );
        git(dir, &["tag", "v1.0.0"]);
        GitCoreRepository {
            url: format!("file://{}", dir.display()),
        }
    }

    #[tokio::test]
    async fn a_failed_clone_leaves_nothing_and_a_broken_cache_is_cloned_again() {
        let tmp = tempfile::tempdir().unwrap();
        let repository = origin(&tmp.path().join("core"));
        let cache = tmp.path().join("cache");
        let dest = cache.join("v1.0.0");

        let failed = repository.clone_at("v9.9.9", &cache.join("v9.9.9")).await;
        assert!(failed.is_err(), "{failed:?}");
        assert_eq!(
            std::fs::read_dir(&cache).unwrap().count(),
            0,
            "no half clone, no staging left"
        );

        // A cache whose `.git` went, and a staging a crash left behind.
        std::fs::create_dir_all(dest.join("stale")).unwrap();
        std::fs::create_dir_all(cache.join(".v1.0.0.cloning-1")).unwrap();
        repository.clone_at("v1.0.0", &dest).await.unwrap();
        assert!(dest.join("lib.rs").is_file());
        assert!(!dest.join("stale").exists());
        let names: Vec<String> = std::fs::read_dir(&cache)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["v1.0.0".to_string()]);
    }
}
