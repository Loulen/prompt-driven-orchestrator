//! What the pipeline context needs from the outside world. The composition
//! root plugs an adapter into each port; the use cases only ever see the
//! traits.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use pdo_kernel::Actor;
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::{SidecarImportReport, SidecarSkill};

use crate::domain::draft::{DraftContent, DraftSummary};
use crate::domain::library::LibraryEntry;
use crate::domain::pipeline::PipelineDef;
use crate::domain::validation::PipelineValidation;
use crate::domain::version::{PipelineVersion, PipelineVersionRef, VersionMeta};

/// The future an async port method returns (the ports are `dyn`, D23).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One pipeline file of the registry, as the registry holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryFile {
    /// The file stem, which is the pipeline's id.
    pub id: String,
    pub path: PathBuf,
    /// The file's modification time, RFC 3339 to the second.
    pub modified: Option<String>,
    /// The YAML, `None` when the file cannot be read.
    pub yaml: Option<String>,
}

/// Why the registry could not create a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateFailure {
    /// A file already sits under that id.
    Exists,
    /// Any other failure, as the filesystem words it.
    Io(String),
}

/// The instance registry: one `<id>.yaml` per pipeline, and its prompts in
/// `<id>.prompts/<node>.md`. Every write it makes on its own behalf is known
/// to the registry's watcher, which does not announce it as an outside edit.
pub trait PipelineRegistry: Send + Sync {
    /// The registry's directory; `None` when it cannot be determined.
    fn dir(&self) -> Option<PathBuf>;
    /// Where the pipeline `id` lives (or would live).
    fn path_of(&self, id: &str) -> PathBuf;
    /// Fold the retired registries into this one, once. A transition of the
    /// daemon: no actor asks for it.
    fn unify_legacy(&self) -> Result<(), String>;
    /// Every `.yaml` file of the registry, in directory order.
    fn files(&self) -> Vec<RegistryFile>;
    /// Whether a file sits under `id`.
    fn exists(&self, id: &str) -> bool;
    /// The YAML of `id`; `None` when it cannot be read.
    fn read(&self, id: &str) -> Option<String>;
    /// The prompts of `id`, by node id.
    fn prompts(&self, id: &str) -> HashMap<String, String>;
    /// Overwrite the YAML of `id`.
    fn write(&self, id: &str, yaml: &str) -> Result<(), String>;
    /// Write each prompt of `id`. A prompt that fails is logged, not fatal.
    fn write_prompts(&self, id: &str, prompts: &HashMap<String, String>);
    /// Delete the prompts of `id` whose node `saved` no longer defines.
    fn prune_prompts(&self, id: &str, saved: &PipelineDef);
    /// Create `id` with `yaml`, refusing to replace an existing file.
    fn create(&self, id: &str, yaml: &str) -> Result<PathBuf, CreateFailure>;
    /// Move `from` to `to`, its prompts with it, writing `yaml` as the new
    /// file. All or nothing: a failure leaves `from` as it was.
    fn relocate(&self, from: &str, to: &str, yaml: &str) -> Result<PathBuf, String>;
    /// Publish a new entry `id`: `pipeline` serialized, and the prompts of its
    /// live nodes. All or nothing.
    fn publish(
        &self,
        id: &str,
        pipeline: &PipelineDef,
        prompts: &HashMap<String, String>,
    ) -> Result<PathBuf, String>;
    /// Remove the prompts of `id` (logged on failure), then its file.
    fn remove(&self, id: &str) -> Result<(), String>;
}

/// The node library.
pub trait NodeLibrary: Send + Sync {
    /// Every entry, by name, case-insensitively.
    fn list(&self) -> Vec<LibraryEntry>;
    fn get(&self, name: &str) -> Option<LibraryEntry>;
    /// Create the entry, or overwrite the one of the same name.
    fn save(&self, entry: &LibraryEntry) -> Result<(), String>;
    /// `false` when no entry carries `name`.
    fn delete(&self, name: &str) -> Result<bool, String>;
}

/// The live stream the clients listen to.
pub trait PipelineEvents: Send + Sync {
    /// `pipeline_changed`: the list and any open editor refresh `id`.
    fn changed(&self, id: &str, path: &str);
    /// `pipeline_validation_changed`: the list, its indicators and the launch
    /// forms read the validation of `id` again.
    fn validation_changed(&self, id: &str);
    /// `pipeline_draft_changed`: the list shows who has the draft of `id`
    /// open, and its changes.
    fn draft_changed(&self, id: &str, draft: &DraftSummary);
}

/// The validations the pipelines state (CONTEXT.md « Pipeline validé »), keyed
/// by pipeline id. A pipeline absent from the store is validated for every
/// Projet.
pub trait PipelineValidations: Send + Sync {
    /// Every stated validation.
    fn all(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineValidation>, String>>;
    /// Replace the validation of `id`.
    fn put<'a>(
        &'a self,
        id: &'a str,
        validation: &'a PipelineValidation,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// Drop the validation of `id`: it reads as every Projet again.
    fn forget<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// A rename moved `from` to `to`: its validation follows. Nothing stated,
    /// nothing moved.
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// The Projets, as the pipeline context sees them.
pub trait KnownProjects: Send + Sync {
    /// Whether the Projet `project_id` exists.
    fn exists<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, Result<bool, String>>;
}

/// The Runs, as the pipeline context sees them.
pub trait ActiveRuns: Send + Sync {
    /// How many Runs not yet completed nor archived run the pipeline `id`.
    fn using<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, usize>;
    /// How many Runs launched each version of the pipeline `id`, by number.
    fn per_version<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, HashMap<u32, usize>>;
}

/// Where the versions of the pipelines are kept (ADR-0088), keyed by
/// pipeline id and number.
pub trait PipelineVersions: Send + Sync {
    /// The versions of `id`, oldest first, without their content.
    fn list<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<VersionMeta>, String>>;
    /// The version `number` of `id`, with its content.
    fn get<'a>(
        &'a self,
        id: &'a str,
        number: u32,
    ) -> BoxFuture<'a, Result<Option<PipelineVersion>, String>>;
    /// Keep `version` under its number, replacing the one there.
    fn put<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// Keep `version` only when `id` has no version yet.
    fn put_first<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>>;
    /// The pipeline `id` was deleted: its versions go.
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// A rename moved `from` to `to`: its versions follow.
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// The last version of every pipeline, by id.
    fn lasts(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineVersionRef>, String>>;
}

/// A pipeline's identity before and after a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    pub old_id: String,
    pub old_name: String,
    pub new_id: String,
    pub new_name: String,
}

/// Stats, as the pipeline context sees them: the new id carries the old id's
/// series (ADR-0077 §4).
pub trait RenameLedger: Send + Sync {
    /// Best effort: a failure is logged, never refused.
    fn record<'a>(&'a self, rename: &'a Renamed) -> BoxFuture<'a, ()>;
}

/// The skill context's bank, as the portable document reads and fills it.
pub trait SkillBank: Send + Sync {
    /// The referenced skills the bank holds, and the ids it does not.
    fn sidecar_of(&self, references: &[SkillRef]) -> (Vec<SidecarSkill>, Vec<String>);
    /// Bring the skills a document references into the bank.
    fn import_sidecar<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_name: &'a str,
        references: Vec<SkillRef>,
        sidecar: &'a [SidecarSkill],
    ) -> BoxFuture<'a, Result<SidecarImportReport, SkillError>>;
}

/// A draft as the store keeps it across a restart (ADR-0088).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredDraft {
    /// Drawn when the draft started from the file: a client holding a copy
    /// of another generation drops it rather than merge two unrelated
    /// histories.
    pub generation: String,
    /// The replicated document, encoded.
    pub state: Vec<u8>,
    /// The file the draft started from, or was last saved as.
    pub baseline: DraftContent,
    /// Its changes since `baseline`.
    pub changes: u32,
    /// The nodes it holds.
    pub nodes: u32,
}

/// Where the drafts are kept.
pub trait DraftStore: Send + Sync {
    fn load<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<StoredDraft>, String>>;
    fn save<'a>(&'a self, id: &'a str, draft: &'a StoredDraft)
        -> BoxFuture<'a, Result<(), String>>;
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// A rename moved the pipeline `from` to `to`: its draft follows.
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// Every kept draft as the list shows it (nobody in it), by pipeline id.
    fn summaries(&self) -> BoxFuture<'_, Result<HashMap<String, DraftSummary>, String>>;
}

/// The replicated document of a draft: it merges the changes of every
/// participant field by field, and converges to the same state for all.
pub trait DraftDocument: Send + Sync {
    /// Its state now.
    fn content(&self) -> DraftContent;
    /// Apply an update a participant sent.
    fn apply_update(&self, update: &[u8]) -> Result<(), String>;
    /// Change the document from `from` to `to`, field by field; the update
    /// that did it, for the participants.
    fn change(&self, from: &DraftContent, to: &DraftContent) -> Vec<u8>;
    /// The whole document, encoded.
    fn encode(&self) -> Vec<u8>;
    /// What it knows, as a state vector.
    fn state_vector(&self) -> Vec<u8>;
    /// What a peer with the state vector `state_vector` misses.
    fn missing_for(&self, state_vector: &[u8]) -> Result<Vec<u8>, String>;
}

/// Makes the replicated documents.
pub trait DraftDocuments: Send + Sync {
    /// A new document holding `content`.
    fn create(&self, content: &DraftContent) -> Box<dyn DraftDocument>;
    /// A document from its encoding.
    fn load(&self, state: &[u8]) -> Result<Box<dyn DraftDocument>, String>;
}
