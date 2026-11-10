//! The ports of the trigger context: what its use cases need from the outside
//! world, as traits the composition root plugs adapters into.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use pdo_platform::domain::audit::NewAuditEntry;
use pdo_skill::domain::selection::SkillRef;
use pdo_workspace::domain::branches::FetchError;

use crate::domain::fire_decision::GuardResult;
use crate::domain::trigger::{FireRecord, NewTrigger, Trigger, TriggerFire, UpdateTrigger};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A storage failure, as its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageError(pub String);

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StorageError {}

pub type Stored<T> = Result<T, StorageError>;

/// The Triggers and their fire history.
pub trait TriggerRepository: Send + Sync {
    /// Create the storage if it does not exist yet (additive migrations included).
    fn init(&self) -> BoxFuture<'_, Stored<()>>;
    /// Insert a new Trigger, returning the stored row.
    fn create(&self, new: NewTrigger) -> BoxFuture<'_, Stored<Trigger>>;
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<Trigger>>>;
    /// Every Trigger, newest first.
    fn list(&self) -> BoxFuture<'_, Stored<Vec<Trigger>>>;
    /// Whether a row was removed.
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>>;
    /// Apply a partial edit; a no-op when no field is set.
    fn update<'a>(&'a self, id: &'a str, edit: UpdateTrigger) -> BoxFuture<'a, Stored<()>>;
    /// Enabled Triggers whose next fire is at or before `now` (an RFC 3339
    /// instant), soonest first, compared as instants whatever their offset.
    fn due<'a>(&'a self, now: &'a str) -> BoxFuture<'a, Stored<Vec<Trigger>>>;
    fn set_next_fire<'a>(
        &'a self,
        id: &'a str,
        next_fire_at: Option<&'a str>,
    ) -> BoxFuture<'a, Stored<()>>;
    /// Record a fire of `id` and roll its outcome up onto the Trigger.
    fn record_fire<'a>(&'a self, id: &'a str, record: &'a FireRecord) -> BoxFuture<'a, Stored<()>>;
    /// The recent fires of `id`, newest first.
    fn fires<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Vec<TriggerFire>>>;
}

/// Runs a guard command: `sh -c`, in `cwd`, bounded by `timeout`.
pub trait GuardRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        command: &'a str,
        cwd: &'a Path,
        timeout: Duration,
    ) -> BoxFuture<'a, GuardResult>;
}

/// The instance settings the context reads and the pause it writes (the
/// settings context holds them). Read fresh at every call, so a change takes
/// effect on the next tick without a restart.
pub trait TriggerSettings: Send + Sync {
    /// The global kill-switch.
    fn paused(&self) -> BoxFuture<'_, Result<bool, String>>;
    fn set_paused(&self, paused: bool) -> BoxFuture<'_, Result<(), String>>;
    /// The guard timeout, `stored → env → default`.
    fn guard_timeout(&self) -> BoxFuture<'_, Duration>;
}

/// What a Trigger reads of the pipeline it fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineFacts {
    /// Its display name, denormalised on the Trigger.
    pub name: String,
    pub prompt_required: bool,
}

/// The pipelines a Trigger names.
pub trait TriggerPipelines: Send + Sync {
    /// `None` when no pipeline has this id; `Some(Err(parse error))` when its
    /// document does not parse.
    fn resolve(&self, pipeline_id: &str) -> Option<Result<PipelineFacts, String>>;
}

/// The repositories a Trigger names (the workspace context).
pub trait Repositories: Send + Sync {
    /// A **required** target repo at a write or fire boundary (ADR-0033): the
    /// validated path, or the message saying why it is refused.
    fn required(&self, field: Option<&str>) -> Result<PathBuf, String>;
    /// Fetch every remote of `targets` (primary first) before a fire cuts
    /// from them (#804): the primary's failure, `None` when it worked.
    fn refresh<'a>(&'a self, targets: &'a [String]) -> BoxFuture<'a, Option<FetchError>>;
}

/// The Projets a Trigger launches its Runs in (ADR-0087), checked as a Run's
/// launch is (the run context).
pub trait TriggerProjects: Send + Sync {
    /// Why a Run may not be launched in the Projet `project` on the primary
    /// repository `repo` (an unknown Projet, a repository it does not hold);
    /// `None` when it may.
    fn refusal<'a>(
        &'a self,
        project: &'a str,
        repo: Option<&'a str>,
    ) -> BoxFuture<'a, Option<String>>;
}

/// The bank a Trigger's skills come from (the skill context).
pub trait TriggerSkills: Send + Sync {
    /// The first skill of `next` neither the bank holds nor `current` already
    /// selects; `None` when `next` may replace `current`.
    fn foreign<'a>(
        &'a self,
        current: &'a [SkillRef],
        next: &'a [SkillRef],
    ) -> BoxFuture<'a, Result<Option<String>, String>>;
}

/// The staging profiles a Trigger's `sandbox` may name (the sandbox context).
pub trait SandboxProfiles: Send + Sync {
    /// The write-time existence gate of a stored `sandbox` value.
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// A Run a Trigger fires: the Trigger (its Run template), the input the
/// decision resolved, and why the pre-cut fetch did not happen, if it did not.
#[derive(Debug, Clone)]
pub struct RunFromTrigger {
    pub trigger: Trigger,
    pub input: String,
    pub source_fetch_error: Option<FetchError>,
}

/// The Runs a Trigger produces (the run context).
pub trait TriggerRuns: Send + Sync {
    /// How many of the Trigger's *own* Runs are still live.
    fn live_count<'a>(&'a self, trigger_id: &'a str) -> BoxFuture<'a, usize>;
    /// Create and start the Run: its id, or the reason it was refused.
    fn start(&self, fire: RunFromTrigger) -> BoxFuture<'_, Result<String, String>>;
}

/// The out-of-Run audit journal (the platform context), best-effort.
pub trait AuditTrail: Send + Sync {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()>;
}
