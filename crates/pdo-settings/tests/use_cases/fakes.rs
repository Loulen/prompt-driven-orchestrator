//! In-memory adapters of every port of the settings context, and the context
//! assembled over them.

use std::collections::BTreeMap;
use std::future::ready;
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_settings::application::ports::{
    AgentProfileStore, BoxFuture, Harnesses, InstanceConfigStore, ProfileReferents,
    SettingsSection, StagingProfiles, Stored, UpdateChecks,
};
use pdo_settings::domain::agent_profile::{AgentProfile, ProfileDraft, DEFAULT_PROFILE_ID};
use pdo_settings::domain::instance_config::{InstanceConfig, UpdateInstanceConfig};
use pdo_settings::domain::referents::{PipelineReferent, ProjectReferent, TriggerReferent};
use pdo_settings::{Settings, SettingsPorts};
use serde_json::{json, Map, Value};

/// The stored tier, in memory: an edit applies like the SQLite one (`""`
/// clears a text knob, a checkbox stores `0`/`1`).
pub struct InMemoryConfig {
    pub row: Mutex<InstanceConfig>,
    pub retired: Mutex<Vec<(&'static str, String)>>,
    pub paused: Mutex<bool>,
    pub writes: Mutex<usize>,
}

impl Default for InMemoryConfig {
    fn default() -> Self {
        Self {
            row: Mutex::new(InstanceConfig {
                session_cap: None,
                reaper_ttl_secs: None,
                libassist_idle_ttl_secs: None,
                guard_timeout_secs: None,
                default_model: None,
                default_sandbox: None,
                autocomplete_turn_end: None,
                default_auto_name: None,
                default_harness: None,
                default_harness_model: BTreeMap::new(),
                auto_fail: None,
                update_check: None,
                review_agent_can_resolve: None,
                agent_choice: None,
                skills: Vec::new(),
                manager_enabled: None,
                manager_profile: None,
                max_attachments_mb: None,
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }),
            retired: Mutex::new(Vec::new()),
            paused: Mutex::new(false),
            writes: Mutex::new(0),
        }
    }
}

fn text(v: String) -> Option<String> {
    (!v.is_empty()).then_some(v)
}

fn flag(v: bool) -> Option<i64> {
    Some(i64::from(v))
}

impl InstanceConfigStore for InMemoryConfig {
    fn get(&self) -> BoxFuture<'_, Stored<InstanceConfig>> {
        Box::pin(ready(Ok(self.row.lock().unwrap().clone())))
    }

    fn update(&self, e: UpdateInstanceConfig) -> BoxFuture<'_, Stored<InstanceConfig>> {
        if !e.is_empty() {
            *self.writes.lock().unwrap() += 1;
            let mut r = self.row.lock().unwrap();
            macro_rules! set {
                ($($f:ident),*) => { $(if let Some(v) = e.$f { r.$f = Some(v); })* };
            }
            set!(
                session_cap,
                reaper_ttl_secs,
                libassist_idle_ttl_secs,
                guard_timeout_secs,
                max_attachments_mb
            );
            macro_rules! set_with {
                ($conv:ident: $($f:ident),*) => { $(if let Some(v) = e.$f { r.$f = $conv(v); })* };
            }
            set_with!(text: default_model, default_sandbox, default_harness, manager_profile);
            set_with!(flag: autocomplete_turn_end, default_auto_name, auto_fail, update_check,
                review_agent_can_resolve, manager_enabled);
            if let Some(v) = e.default_harness_model {
                r.default_harness_model = v;
            }
            if let Some(v) = e.agent_choice {
                r.agent_choice = v;
            }
            if let Some(v) = e.skills {
                r.skills = v;
            }
            r.updated_at = "2026-01-02T00:00:00.000Z".into();
        }
        Box::pin(ready(Ok(self.row.lock().unwrap().clone())))
    }

    fn retired_sandbox_values(&self) -> BoxFuture<'_, Stored<Vec<(&'static str, String)>>> {
        Box::pin(ready(Ok(self.retired.lock().unwrap().clone())))
    }

    fn triggers_paused(&self) -> BoxFuture<'_, Stored<bool>> {
        Box::pin(ready(Ok(*self.paused.lock().unwrap())))
    }

    fn set_triggers_paused(&self, paused: bool) -> BoxFuture<'_, Stored<()>> {
        *self.paused.lock().unwrap() = paused;
        Box::pin(ready(Ok(())))
    }
}

/// The agentic profiles, in memory, seeded with the reserved `Default`.
pub struct InMemoryProfiles {
    pub profiles: Mutex<Vec<AgentProfile>>,
    next: Mutex<u32>,
}

pub fn profile(id: &str, name: &str, harness: &str) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        name: name.into(),
        harness: harness.into(),
        model: None,
        effort: None,
        created_at: "t0".into(),
        updated_at: "t0".into(),
        attributes: Default::default(),
    }
}

impl Default for InMemoryProfiles {
    fn default() -> Self {
        Self {
            profiles: Mutex::new(vec![profile(DEFAULT_PROFILE_ID, "Default", "claude")]),
            next: Mutex::new(0),
        }
    }
}

impl InMemoryProfiles {
    pub fn named(&self, name: &str) -> Option<AgentProfile> {
        self.profiles
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.name == name)
            .cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.profiles
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.id.clone())
            .collect()
    }
}

impl AgentProfileStore for InMemoryProfiles {
    fn list(&self) -> BoxFuture<'_, Stored<Vec<AgentProfile>>> {
        Box::pin(ready(Ok(self.profiles.lock().unwrap().clone())))
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        let found = self
            .profiles
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned();
        Box::pin(ready(Ok(found)))
    }

    fn find_by_name<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        let wanted = name.trim().to_lowercase();
        let found = self
            .profiles
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.name.trim().to_lowercase() == wanted)
            .cloned();
        Box::pin(ready(Ok(found)))
    }

    fn create(&self, d: ProfileDraft) -> BoxFuture<'_, Stored<AgentProfile>> {
        let mut next = self.next.lock().unwrap();
        *next += 1;
        let created = AgentProfile {
            id: format!("agp-{next}"),
            name: d.name,
            harness: d.harness,
            model: d.model,
            effort: d.effort,
            created_at: "t1".into(),
            updated_at: "t1".into(),
            attributes: Default::default(),
        };
        self.profiles.lock().unwrap().push(created.clone());
        Box::pin(ready(Ok(created)))
    }

    fn replace<'a>(
        &'a self,
        id: &'a str,
        d: ProfileDraft,
    ) -> BoxFuture<'a, Stored<Option<AgentProfile>>> {
        let mut profiles = self.profiles.lock().unwrap();
        let found = profiles.iter_mut().find(|p| p.id == id).map(|p| {
            p.name = d.name;
            p.harness = d.harness;
            p.model = d.model;
            p.effort = d.effort;
            p.updated_at = "t2".into();
            p.clone()
        });
        Box::pin(ready(Ok(found)))
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>> {
        let mut profiles = self.profiles.lock().unwrap();
        let before = profiles.len();
        profiles.retain(|p| p.id != id);
        Box::pin(ready(Ok(profiles.len() < before)))
    }

    fn snapshot(&self) -> BoxFuture<'_, Stored<BTreeMap<String, ResolvedCombo>>> {
        let snap = self
            .profiles
            .lock()
            .unwrap()
            .iter()
            .map(|p| (p.id.clone(), p.combo()))
            .collect();
        Box::pin(ready(Ok(snap)))
    }
}

/// A section that discloses fixed keys, and one that fails.
pub struct FixedSection(pub Map<String, Value>);

impl SettingsSection for FixedSection {
    fn disclose<'a>(
        &'a self,
        _stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        Box::pin(ready(Ok(self.0.clone())))
    }
}

/// A section echoing the stored session cap, so a test sees the page follow
/// an edit.
pub struct EchoCap;

impl SettingsSection for EchoCap {
    fn disclose<'a>(
        &'a self,
        stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        let mut keys = Map::new();
        keys.insert(
            "session_cap".into(),
            json!({ "stored": stored.session_cap }),
        );
        Box::pin(ready(Ok(keys)))
    }
}

pub struct FailingSection;

impl SettingsSection for FailingSection {
    fn disclose<'a>(
        &'a self,
        _stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        Box::pin(ready(Err("the probe broke".to_string())))
    }
}

/// The harnesses `claude` and `opencode`, and whether a warm-up ran.
#[derive(Default)]
pub struct KnownHarnesses {
    pub warmed: Mutex<usize>,
}

impl Harnesses for KnownHarnesses {
    fn check_default(&self, name: &str) -> Result<(), String> {
        match name {
            "claude" | "opencode" => Ok(()),
            other => Err(format!("unknown default harness `{other}`")),
        }
    }

    fn warm(&self) -> BoxFuture<'_, ()> {
        *self.warmed.lock().unwrap() += 1;
        Box::pin(ready(()))
    }
}

/// `off`, `minimal`, `full` and `team` exist.
pub struct KnownProfiles;

impl StagingProfiles for KnownProfiles {
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(ready(match raw {
            "" | "off" | "minimal" | "full" | "team" => Ok(()),
            other => Err(format!("unknown sandbox profile `{other}`")),
        }))
    }
}

#[derive(Default)]
pub struct Checks {
    pub now: Mutex<usize>,
}

impl UpdateChecks for Checks {
    fn check_now(&self) {
        *self.now.lock().unwrap() += 1;
    }
}

/// Fixed referents of every profile.
#[derive(Default)]
pub struct Referents {
    pub projects: Vec<ProjectReferent>,
    pub triggers: Vec<TriggerReferent>,
    pub pipelines: Vec<PipelineReferent>,
}

impl ProfileReferents for Referents {
    fn projects<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Vec<ProjectReferent>> {
        Box::pin(ready(self.projects.clone()))
    }

    fn triggers<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Vec<TriggerReferent>> {
        Box::pin(ready(self.triggers.clone()))
    }

    fn pipelines(&self, _id: &str) -> Vec<PipelineReferent> {
        self.pipelines.clone()
    }
}

/// A host that records what it is asked and refuses when told to.
#[derive(Default)]
pub struct Host {
    pub refuse: Mutex<bool>,
    pub asked: Mutex<Vec<(Actor, Operation)>>,
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.asked
            .lock()
            .unwrap()
            .push((actor.clone(), operation.clone()));
        if *self.refuse.lock().unwrap() {
            Authorization::Deny {
                reason: "the test host refuses everything".into(),
            }
        } else {
            Authorization::Allow
        }
    }

    fn publish(&self, _event: &Value) {}
}

/// The context over fresh fakes, and the fakes.
pub struct World {
    pub settings: Arc<Settings>,
    pub config: Arc<InMemoryConfig>,
    pub profiles: Arc<InMemoryProfiles>,
    pub harnesses: Arc<KnownHarnesses>,
    pub checks: Arc<Checks>,
    pub host: Arc<Host>,
}

impl World {
    pub fn refuse(&self) {
        *self.host.refuse.lock().unwrap() = true;
    }

    pub fn asked(&self) -> Vec<(Actor, Operation)> {
        self.host.asked.lock().unwrap().clone()
    }

    pub fn row(&self) -> InstanceConfig {
        self.config.row.lock().unwrap().clone()
    }
}

pub fn world() -> World {
    world_with(vec![Arc::new(EchoCap)], Referents::default())
}

pub fn world_with(sections: Vec<Arc<dyn SettingsSection>>, referents: Referents) -> World {
    let config = Arc::new(InMemoryConfig::default());
    let profiles = Arc::new(InMemoryProfiles::default());
    let harnesses = Arc::new(KnownHarnesses::default());
    let checks = Arc::new(Checks::default());
    let host = Arc::new(Host::default());
    let settings = Settings::new(SettingsPorts {
        config: config.clone(),
        profiles: profiles.clone(),
        sections,
        harnesses: harnesses.clone(),
        staging_profiles: Arc::new(KnownProfiles),
        update_checks: checks.clone(),
        referents: Arc::new(referents),
        extensions: host.clone(),
    });
    World {
        settings,
        config,
        profiles,
        harnesses,
        checks,
        host,
    }
}
