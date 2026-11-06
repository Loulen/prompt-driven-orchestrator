//! The ports of the settings context: what its use cases need from the outside
//! world, as traits the composition root plugs adapters into.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::Actor;
use pdo_skill::domain::selection::SkillRef;

use crate::domain::agent_profile::{AgentProfile, ProfileDraft};
use crate::domain::appearance::AppearanceDocument;
use crate::domain::instance_config::{InstanceConfig, UpdateInstanceConfig};
use crate::domain::referents::{PipelineReferent, ProjectReferent, TriggerReferent};

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

/// The stored tier of every instance default: one singleton row.
pub trait InstanceConfigStore: Send + Sync {
    /// The row, always present once the storage exists.
    fn get(&self) -> BoxFuture<'_, Stored<InstanceConfig>>;
    /// Apply a partial edit and return the row. A no-op edit leaves
    /// `updated_at` alone.
    fn update(&self, edit: UpdateInstanceConfig) -> BoxFuture<'_, Stored<InstanceConfig>>;
    /// Whatever is left in the retired sandbox image fields, non-empty values
    /// only, in the order the boot warning reads them.
    fn retired_sandbox_values(&self) -> BoxFuture<'_, Stored<Vec<(&'static str, String)>>>;
    /// The global Trigger pause, stored on the same row but kept off the page:
    /// a kill-switch has no env tier and no default other than "off".
    fn triggers_paused(&self) -> BoxFuture<'_, Stored<bool>>;
    fn set_triggers_paused(&self, paused: bool) -> BoxFuture<'_, Stored<()>>;
}

/// The agentic profiles.
pub trait AgentProfileStore: Send + Sync {
    /// Every profile, the reserved `Default` first, then by creation order.
    fn list(&self) -> BoxFuture<'_, Stored<Vec<AgentProfile>>>;
    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>>;
    /// The profile named `name`, case-insensitively and trimmed.
    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>>;
    /// Store a new profile under a fresh id.
    fn create(&self, draft: ProfileDraft) -> BoxFuture<'_, Stored<AgentProfile>>;
    /// Replace the editable fields of `id`; `None` when it no longer exists.
    fn replace<'a>(
        &'a self,
        id: &'a str,
        draft: ProfileDraft,
    ) -> BoxFuture<'a, Stored<Option<AgentProfile>>>;
    /// Whether a row was removed.
    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>>;
    /// Every profile's `id → combo` in ONE read, so a resolution walking the
    /// tiers never mixes two revisions of a profile (ADR-0057 ¶4).
    fn snapshot(&self) -> BoxFuture<'_, Stored<BTreeMap<String, ResolvedCombo>>>;
}

/// The Apparence of each Actor: one document per Actor, replaced whole.
pub trait AppearanceStore: Send + Sync {
    /// `actor`'s document; `None` while it never wrote one.
    fn get<'a>(&'a self, actor: &'a Actor) -> BoxFuture<'a, Stored<Option<AppearanceDocument>>>;
    /// Replace `actor`'s document.
    fn put<'a>(
        &'a self,
        actor: &'a Actor,
        document: &'a AppearanceDocument,
    ) -> BoxFuture<'a, Stored<()>>;
}

/// A part of the settings page a context discloses: its instance defaults
/// over the stored tier, or the facts it shows beside them. Each key belongs to
/// one section.
pub trait SettingsSection: Send + Sync {
    fn disclose<'a>(
        &'a self,
        stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<serde_json::Map<String, serde_json::Value>, String>>;
}

/// The harnesses (the harness context).
pub trait Harnesses: Send + Sync {
    /// Whether `name` resolves to an embedded or declared harness; the refusal
    /// names the known ones.
    fn check_default(&self, name: &str) -> Result<(), String>;
    /// Probe every installed harness's catalogue once, so the first page
    /// answers warm.
    fn warm(&self) -> BoxFuture<'_, ()>;
    /// Probe the catalogue of `name` again now, at the same binary version or
    /// not, and answer its entry of the page's `harness_descriptors.harnesses`;
    /// `None` when no harness is called `name`.
    fn refresh<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Option<serde_json::Value>>;
}

/// Who hears that a harness's catalogue was probed again: every open page
/// reads it anew.
pub trait CatalogueEvents: Send + Sync {
    fn catalogue_refreshed(&self, harness: &str);
}

/// The staging profiles (the sandbox context).
pub trait StagingProfiles: Send + Sync {
    /// The write-time gate of a stored `sandbox` value.
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

/// The bank of skills (the skill context).
pub trait SkillBank: Send + Sync {
    /// Why the instance's selection may not become `next`: it gains a skill
    /// the bank does not hold (one `current` already selects is kept, as the
    /// selectors keep it). `None`: it may.
    fn refusal<'a>(
        &'a self,
        current: &'a [SkillRef],
        next: &'a [SkillRef],
    ) -> BoxFuture<'a, Result<Option<String>, String>>;
}

/// The version check (the platform context).
pub trait UpdateChecks: Send + Sync {
    /// Run one check now, detached: re-enabling the check must not wait for
    /// the next period.
    fn check_now(&self);
}

/// The telemetry export (the telemetry context).
pub trait TelemetryReloads: Send + Sync {
    /// Apply the stored setting now, detached: an edit takes effect without a
    /// restart.
    fn reload(&self);
}

/// Who names an agentic profile, outside the instance tier.
pub trait ProfileReferents: Send + Sync {
    fn projects<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, Vec<ProjectReferent>>;
    fn triggers<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, Vec<TriggerReferent>>;
    fn pipelines(&self, profile_id: &str) -> Vec<PipelineReferent>;
}
