//! What an Agent de support reads besides its prompt: the skills it receives,
//! and the core's code at the tag of the instance, cloned at the first demand
//! and kept in a cache, never for a version without a public tag.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pdo_kernel::authorization::authorize;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::ports::{CoreRepository, SupportSkills};
use super::{Support, SupportError};
use crate::domain::names::support_session_name_of;
use crate::domain::support_sources::{
    core_source_dir, core_source_tag, instance_section, CoreCheckout, CoreSource, CoreSourceRefusal,
};

/// The action of the `session` Operation that gives the core's code to an
/// Agent de support.
pub const SOURCE: &str = "source";

/// How long an opening trusts a failed question to the repository (no
/// network): within it, the next openings do not wait for the repository
/// again. `pdo support source` always asks again.
const UNCHECKED_MEMORY: Duration = Duration::from_secs(600);

/// What is known of the tag in the repository.
#[derive(Debug, Clone)]
enum Published {
    /// Not asked yet.
    Unknown,
    /// Answered: a tag never comes and goes, kept for the daemon's life.
    Known(bool),
    /// The repository could not be asked, at this instant, for this reason.
    Failed(Instant, String),
}

/// Where the skills and the core's code of the Agents de support come from.
pub struct SupportSources {
    skills: Arc<dyn SupportSkills>,
    repository: Arc<dyn CoreRepository>,
    /// The folder the clones of the core are kept in, one per tag.
    cache_root: PathBuf,
    /// The tag of the core this binary is built on.
    tag: String,
    published: Mutex<Published>,
    /// Held while a clone runs: two demands never clone twice.
    cloning: tokio::sync::Mutex<()>,
}

impl SupportSources {
    /// The sources of a core at `version`, its clones kept under `cache_root`.
    pub fn new(
        skills: Arc<dyn SupportSkills>,
        repository: Arc<dyn CoreRepository>,
        cache_root: PathBuf,
        version: &str,
    ) -> Self {
        Self {
            skills,
            repository,
            cache_root,
            tag: core_source_tag(version),
            published: Mutex::new(Published::Unknown),
            cloning: tokio::sync::Mutex::new(()),
        }
    }

    fn dir(&self) -> PathBuf {
        core_source_dir(&self.cache_root, &self.tag)
    }

    /// The clone of the tag, when one is in the cache.
    fn cloned(&self) -> Option<CoreCheckout> {
        let dir = self.dir();
        dir.join(".git").exists().then(|| CoreCheckout {
            tag: self.tag.clone(),
            dir,
        })
    }

    /// Whether the repository has the tag: an answer is remembered; a failure
    /// too, for [`UNCHECKED_MEMORY`], unless `ask_again`.
    async fn published(&self, ask_again: bool) -> Result<bool, String> {
        match self.published.lock().unwrap().clone() {
            Published::Known(known) => return Ok(known),
            Published::Failed(at, reason) if !ask_again && at.elapsed() < UNCHECKED_MEMORY => {
                return Err(reason)
            }
            _ => {}
        }
        let answer = self.repository.has_tag(&self.tag).await;
        *self.published.lock().unwrap() = match &answer {
            Ok(known) => Published::Known(*known),
            Err(reason) => Published::Failed(Instant::now(), reason.clone()),
        };
        answer
    }

    /// Where the core's code stands, cloning nothing: in the cache, public,
    /// without a public tag, or not known now.
    pub async fn core_source(&self) -> CoreSource {
        if let Some(checkout) = self.cloned() {
            return CoreSource::Cloned {
                tag: checkout.tag,
                dir: checkout.dir,
            };
        }
        let tag = self.tag.clone();
        match self.published(false).await {
            Ok(true) => CoreSource::Published { tag },
            Ok(false) => CoreSource::NoPublicTag { tag },
            Err(reason) => CoreSource::Unchecked { tag, reason },
        }
    }

    /// The core's code at the tag: the clone in the cache, else a new one, if
    /// the tag is public; no clone is tried otherwise.
    pub async fn checkout(&self) -> Result<CoreCheckout, CoreSourceRefusal> {
        if let Some(checkout) = self.cloned() {
            return Ok(checkout);
        }
        let _cloning = self.cloning.lock().await;
        // Another demand may have cloned it while this one waited.
        if let Some(checkout) = self.cloned() {
            return Ok(checkout);
        }
        let tag = self.tag.clone();
        match self.published(true).await {
            Ok(true) => {}
            Ok(false) => return Err(CoreSourceRefusal::NoPublicTag { tag }),
            Err(reason) => return Err(CoreSourceRefusal::Unchecked { tag, reason }),
        }
        let dir = self.dir();
        self.repository
            .clone_at(&tag, &dir)
            .await
            .map_err(|reason| CoreSourceRefusal::CloneFailed {
                tag: tag.clone(),
                reason,
            })?;
        info!("Cloned the core at {tag} into {}", dir.display());
        Ok(CoreCheckout { tag, dir })
    }

    /// Deliver the skills into the folder `dir` of an agent, and say, for its
    /// instructions, which were delivered and where the core's code stands.
    async fn instance_section(&self, dir: &Path) -> Result<String, String> {
        let skills = self.skills.deliver(dir).await?;
        Ok(instance_section(&skills, &self.core_source().await))
    }
}

/// What a daemon without sources says: no skill, no core's code.
fn unsourced() -> (String, &'static str) {
    (
        core_source_tag(pdo_platform::domain::release_channel::CORE_VERSION),
        "this daemon has no source for the core's code",
    )
}

impl Support {
    /// The core's code at the instance's tag, for the Agent de support of
    /// `actor` (the Operation `session` × `source` on its session): the clone
    /// in the cache, made at the first demand. Refused, with no clone tried,
    /// when the version has no public tag or that could not be checked. The
    /// clone runs on its own task: a caller that stops waiting (its request
    /// dropped) does not kill it, and the next demand finds it done.
    pub async fn core_source(&self, actor: &Actor) -> Result<CoreCheckout, SupportError> {
        let session = support_session_name_of(actor);
        let operation =
            Operation::new(ResourceKind::Session, Verb::Action(SOURCE.into())).on(session);
        authorize(self.host.as_ref(), actor, &operation).map_err(SupportError::Refused)?;
        let Some(sources) = self.sources.clone() else {
            let (tag, reason) = unsourced();
            return Err(SupportError::CoreSource(CoreSourceRefusal::Unchecked {
                tag,
                reason: reason.into(),
            }));
        };
        let tag = sources.tag.clone();
        tokio::spawn(async move { sources.checkout().await })
            .await
            .unwrap_or_else(|e| {
                Err(CoreSourceRefusal::CloneFailed {
                    tag,
                    reason: e.to_string(),
                })
            })
            .map_err(SupportError::CoreSource)
    }

    /// The part of the instructions of the agent working in `dir` that this
    /// instance gives, its skills delivered there first. A delivery that fails
    /// on the disk refuses the opening, as it refuses a Node's spawn.
    pub(super) async fn instance_section(&self, dir: &Path) -> Result<String, SupportError> {
        let Some(sources) = &self.sources else {
            let (tag, reason) = unsourced();
            return Ok(instance_section(
                &[],
                &CoreSource::Unchecked {
                    tag,
                    reason: reason.into(),
                },
            ));
        };
        sources.instance_section(dir).await.map_err(|e| {
            SupportError::Folder(format!(
                "failed to deliver the skills to {}: {e}",
                dir.display()
            ))
        })
    }
}
