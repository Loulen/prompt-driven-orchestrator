//! The versions of the registry's pipelines (ADR-0088): what a Save froze,
//! read back for the history, restored into the draft by the editor, kept as
//! a pipeline of its own.
//!
//! A pipeline nobody saved since the versions exist has none: the first read
//! makes its file v1, titled by the free « Version » of its identity.

use std::collections::{BTreeMap, HashMap, HashSet};

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::warn;

use super::editor_json::editor_json;
use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::queries::PipelineDetail;
use super::registry_names::{lenient_names, listed_name};
use super::Created;
use crate::domain::naming::{available_identity, copy_name};
use crate::domain::pipeline::parse_pipeline;
use crate::domain::version::{
    default_title, first_title, kept_name, PipelineVersion, PipelineVersionRef, VersionMeta,
    EXTERNAL_EDIT,
};

/// A version as the history lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedVersion {
    pub meta: VersionMeta,
    /// The Runs that launched it.
    pub runs: usize,
}

/// A version as the editor shows it, read only.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionDetail {
    pub meta: VersionMeta,
    pub yaml: String,
    /// The pipeline as `GET /pipelines/{id}` serves it.
    pub pipeline: serde_json::Value,
    pub prompts: HashMap<String, String>,
}

impl Pipelines {
    /// The versions of `id`, oldest first; its file becomes v1 the first time.
    pub(crate) async fn ensure_versions(
        &self,
        id: &str,
    ) -> Result<Vec<VersionMeta>, PipelineError> {
        let listed = self
            .versions
            .list(id)
            .await
            .map_err(PipelineError::Storage)?;
        if !listed.is_empty() {
            return Ok(listed);
        }
        let yaml = self.registry.read(id).ok_or(PipelineError::NotFound)?;
        let declared = parse_pipeline(&yaml)
            .ok()
            .and_then(|parsed| parsed.pipeline.version);
        let saved_at = self
            .registry
            .files()
            .into_iter()
            .find(|file| file.id == id)
            .and_then(|file| file.modified)
            .unwrap_or_else(pdo_kernel::clock::now_iso);
        let first = PipelineVersion {
            meta: VersionMeta {
                number: 1,
                title: first_title(declared.as_deref()),
                author: None,
                saved_at,
                external: false,
            },
            prompts: live_prompts(&yaml, &self.registry.prompts(id)),
            yaml,
        };
        // Only where nothing was kept meanwhile: a save that numbered v2 since
        // keeps the v1 it read.
        self.versions
            .put_first(id, &first)
            .await
            .map_err(PipelineError::Storage)?;
        self.versions.list(id).await.map_err(PipelineError::Storage)
    }

    /// The history of `id`: its versions, newest first, with the Runs each
    /// launched.
    pub async fn versions(&self, id: &str) -> Result<Vec<ListedVersion>, PipelineError> {
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        let mut runs = self.runs.per_version(id).await;
        let mut listed: Vec<ListedVersion> = self
            .ensure_versions(id)
            .await?
            .into_iter()
            .map(|meta| ListedVersion {
                runs: runs.remove(&meta.number).unwrap_or(0),
                meta,
            })
            .collect();
        listed.reverse();
        Ok(listed)
    }

    /// The version `number` of `id`, as the editor reads it.
    pub async fn version(&self, id: &str, number: u32) -> Result<VersionDetail, PipelineError> {
        if !self.registry.exists(id) {
            return Err(PipelineError::NotFound);
        }
        self.ensure_versions(id).await?;
        let version = self
            .versions
            .get(id, number)
            .await
            .map_err(PipelineError::Storage)?
            .ok_or(PipelineError::NotFound)?;
        let parsed = parse_pipeline(&version.yaml)?;
        let detail = PipelineDetail {
            id: id.to_string(),
            path: self.registry.path_of(id),
            yaml: version.yaml.clone(),
            pipeline: parsed.pipeline,
            prompts: version.prompts.clone().into_iter().collect(),
            diagnostics: parsed.diagnostics.into_iter().map(|d| d.message).collect(),
        };
        let pipeline =
            editor_json(&detail).map_err(|e| PipelineError::BadRequest(e.to_string()))?;
        Ok(VersionDetail {
            meta: version.meta,
            yaml: version.yaml,
            pipeline,
            prompts: detail.prompts,
        })
    }

    /// The last version of `id`: the file, which New Run and the Triggers
    /// launch. `None` for a pipeline the registry does not hold.
    pub async fn last_version(&self, id: &str) -> Option<PipelineVersionRef> {
        if !self.registry.exists(id) {
            return None;
        }
        match self.ensure_versions(id).await {
            Ok(listed) => listed.last().map(VersionMeta::reference),
            Err(e) => {
                warn!("versions of pipeline {id}: unreadable: {e:?}");
                None
            }
        }
    }

    /// The last version of each of `ids`.
    pub async fn last_versions(&self, ids: &[String]) -> HashMap<String, PipelineVersionRef> {
        let mut lasts = self.versions.lasts().await.unwrap_or_default();
        for id in ids {
            if lasts.contains_key(id) {
                continue;
            }
            if let Some(last) = self.last_version(id).await {
                lasts.insert(id.clone(), last);
            }
        }
        lasts
    }

    /// Keep what the file of `id` now holds as the version `number`, titled
    /// `title`, saved by `author`. Best effort: the file is written, and
    /// failing the save over a lost row would lie about it.
    pub(crate) async fn record_version(
        &self,
        author: Option<&Actor>,
        id: &str,
        number: u32,
        title: String,
    ) -> PipelineVersionRef {
        let reference = PipelineVersionRef {
            number,
            title: title.clone(),
        };
        let Some(yaml) = self.registry.read(id) else {
            return reference;
        };
        let version = PipelineVersion {
            meta: VersionMeta {
                number,
                title,
                author: author.cloned(),
                saved_at: pdo_kernel::clock::now_iso(),
                external: false,
            },
            prompts: live_prompts(&yaml, &self.registry.prompts(id)),
            yaml,
        };
        if let Err(e) = self.versions.put(id, &version).await {
            warn!("version {number} of pipeline {id}: not kept: {e}");
        }
        reference
    }

    /// The file of `id` was written outside PDO: what it holds now is a
    /// version « external edit », with no author, unless the last version
    /// already holds it (the same write heard twice) or it does not read (a
    /// write caught half-way, a conflicted merge: the next write that reads
    /// is kept). The version the file is; `None` when there is none to tell.
    pub(crate) async fn record_external_version(&self, id: &str) -> Option<PipelineVersionRef> {
        let _one_at_a_time = self.version_writes.lock().await;
        let listed = match self.versions.list(id).await {
            Ok(listed) => listed,
            Err(e) => {
                warn!("versions of pipeline {id}: unreadable: {e}");
                return None;
            }
        };
        // No version yet: the file, as written, becomes v1.
        let Some(last) = listed.last() else {
            return self
                .ensure_versions(id)
                .await
                .ok()?
                .last()
                .map(VersionMeta::reference);
        };
        let yaml = self.registry.read(id)?;
        parse_pipeline(&yaml).ok()?;
        let prompts = live_prompts(&yaml, &self.registry.prompts(id));
        if let Ok(Some(kept)) = self.versions.get(id, last.number).await {
            if kept.yaml == yaml && kept.prompts == prompts {
                return Some(last.reference());
            }
        }
        let version = PipelineVersion {
            meta: VersionMeta {
                number: last.number + 1,
                title: EXTERNAL_EDIT.to_string(),
                author: None,
                saved_at: pdo_kernel::clock::now_iso(),
                external: true,
            },
            prompts,
            yaml,
        };
        if let Err(e) = self.versions.put(id, &version).await {
            warn!(
                "version {} of pipeline {id}: not kept: {e}",
                version.meta.number
            );
            return None;
        }
        self.events.version_changed(id);
        Some(version.meta.reference())
    }

    /// The file of `id` was written by PDO outside a Save (a rename, an
    /// overwrite): it is the last version from now on.
    pub(crate) async fn file_is_last_version(&self, author: Option<&Actor>, id: &str, new: bool) {
        let _one_at_a_time = self.version_writes.lock().await;
        let listed = match self.ensure_versions(id).await {
            Ok(listed) => listed,
            Err(e) => {
                warn!("versions of pipeline {id}: unreadable: {e:?}");
                return;
            }
        };
        let Some(last) = listed.last() else { return };
        if new {
            let number = last.number + 1;
            self.record_version(author, id, number, default_title(number))
                .await;
            return;
        }
        let Ok(Some(mut kept)) = self.versions.get(id, last.number).await else {
            return;
        };
        let Some(yaml) = self.registry.read(id) else {
            return;
        };
        kept.prompts = live_prompts(&yaml, &self.registry.prompts(id));
        kept.yaml = yaml;
        if let Err(e) = self.versions.put(id, &kept).await {
            warn!("version {} of pipeline {id}: not updated: {e}", last.number);
        }
    }

    /// Keep `version` of a pipeline as a pipeline of its own, named
    /// `<name> (vN)` (the first free name): a creation, as a duplicate is. It
    /// starts its history at v1, with the title, author and date `version`
    /// had.
    pub(crate) async fn keep_as_pipeline(
        &self,
        actor: &Actor,
        version: &PipelineVersion,
    ) -> Result<Created, PipelineError> {
        self.authorize(actor, Operation::new(ResourceKind::Pipeline, Verb::Create))?;
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        let mut pipeline = parse_pipeline(&version.yaml)?.pipeline;
        let files = self.registry.files();
        let listed: HashSet<String> = files.iter().map(listed_name).collect();
        let wanted = kept_name(&pipeline.name, version.meta.number);
        let wanted = if listed.contains(&wanted) {
            copy_name(&wanted, &listed)
        } else {
            wanted
        };
        let (id, name) = available_identity(&wanted, &lenient_names(&files), |stem| {
            self.registry.exists(stem)
        });
        pipeline.name = name;
        let prompts: HashMap<String, String> = version.prompts.clone().into_iter().collect();
        let path = self
            .registry
            .publish(&id, &pipeline, &prompts)
            .map_err(PipelineError::Storage)?;
        self.settle_created(actor, &id).await;
        if let Some(yaml) = self.registry.read(&id) {
            let first = PipelineVersion {
                meta: VersionMeta {
                    number: 1,
                    ..version.meta.clone()
                },
                prompts: live_prompts(&yaml, &self.registry.prompts(&id)),
                yaml,
            };
            if let Err(e) = self.versions.put(&id, &first).await {
                warn!("version 1 of pipeline {id}: not kept: {e}");
            }
        }
        self.events.changed(&id, &path.to_string_lossy());
        Ok(Created { id, path })
    }

    /// A rename moved `from` to `to`: its versions follow.
    pub(crate) async fn carry_versions(&self, from: &str, to: &str) {
        if from == to {
            return;
        }
        if let Err(e) = self.versions.carry(from, to).await {
            warn!("versions of pipeline {from}: not moved to {to}: {e}");
        }
    }

    /// The pipeline `id` was deleted: so are its versions.
    pub(crate) async fn forget_versions(&self, id: &str) {
        if let Err(e) = self.versions.remove(id).await {
            warn!("versions of pipeline {id}: not removed: {e}");
        }
    }
}

/// The prompts of the nodes `yaml` defines (all of them when it does not
/// parse): the ones a version holds.
fn live_prompts(yaml: &str, prompts: &HashMap<String, String>) -> BTreeMap<String, String> {
    match parse_pipeline(yaml) {
        Ok(parsed) => parsed
            .pipeline
            .nodes
            .iter()
            .filter_map(|node| prompts.get(&node.id).map(|p| (node.id.clone(), p.clone())))
            .collect(),
        Err(_) => prompts.clone().into_iter().collect(),
    }
}
