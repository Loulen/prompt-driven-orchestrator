//! In-memory adapters of every port of the pipeline context.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_pipeline::adapters::draft_doc::YDraftDocuments;
use pdo_pipeline::application::ports::*;
use pdo_pipeline::domain::draft::DraftSummary;
use pdo_pipeline::domain::library::LibraryEntry;
use pdo_pipeline::domain::pipeline::{split_live_prompts, PipelineDef};
use pdo_pipeline::domain::validation::PipelineValidation;
use pdo_pipeline::domain::version::{PipelineVersion, PipelineVersionRef, VersionMeta};
use pdo_pipeline::{Pipelines, PipelinesPorts};
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::{SidecarImportReport, SidecarSkill};

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

/// An extension host that records every Operation and refuses them all when
/// given a reason.
#[derive(Default)]
pub struct Host {
    pub refusal: Option<String>,
    pub asked: Mutex<Vec<Operation>>,
}

impl Host {
    pub fn refusing(reason: &str) -> Self {
        Self {
            refusal: Some(reason.to_string()),
            ..Self::default()
        }
    }

    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }
    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        match &self.refusal {
            Some(reason) => Authorization::deny(reason.clone()),
            None => Authorization::Allow,
        }
    }
    fn publish(&self, _event: &serde_json::Value) {}
}

/// One pipeline of the in-memory registry.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub yaml: String,
    pub prompts: HashMap<String, String>,
}

/// The registry, as a map from id to file. `dir: None` is a registry whose
/// directory cannot be determined.
pub struct Registry {
    pub dir: Option<PathBuf>,
    pub entries: Mutex<BTreeMap<String, Entry>>,
    pub unified: Mutex<usize>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            dir: Some(PathBuf::from("/registry")),
            entries: Mutex::default(),
            unified: Mutex::default(),
        }
    }
}

impl Registry {
    pub fn put(&self, id: &str, yaml: &str) {
        self.entries.lock().unwrap().insert(
            id.to_string(),
            Entry {
                yaml: yaml.to_string(),
                prompts: HashMap::new(),
            },
        );
    }

    pub fn put_prompt(&self, id: &str, node: &str, prompt: &str) {
        self.entries
            .lock()
            .unwrap()
            .get_mut(id)
            .expect("the pipeline exists")
            .prompts
            .insert(node.to_string(), prompt.to_string());
    }

    pub fn get(&self, id: &str) -> Option<Entry> {
        self.entries.lock().unwrap().get(id).cloned()
    }

    pub fn exists_id(&self, id: &str) -> bool {
        self.entries.lock().unwrap().contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.entries.lock().unwrap().keys().cloned().collect()
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("/repo/.pdo/pipelines"))
            .join(format!("{id}.yaml"))
    }
}

impl PipelineRegistry for Registry {
    fn dir(&self) -> Option<PathBuf> {
        self.dir.clone()
    }
    fn path_of(&self, id: &str) -> PathBuf {
        self.path(id)
    }
    fn unify_legacy(&self) -> Result<(), String> {
        *self.unified.lock().unwrap() += 1;
        Ok(())
    }
    fn files(&self) -> Vec<RegistryFile> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .map(|(id, entry)| RegistryFile {
                id: id.clone(),
                path: self.path(id),
                modified: Some("2026-09-27T00:00:00Z".into()),
                yaml: Some(entry.yaml.clone()),
            })
            .collect()
    }
    fn exists(&self, id: &str) -> bool {
        self.entries.lock().unwrap().contains_key(id)
    }
    fn read(&self, id: &str) -> Option<String> {
        self.get(id).map(|e| e.yaml)
    }
    fn prompts(&self, id: &str) -> HashMap<String, String> {
        self.get(id).map(|e| e.prompts).unwrap_or_default()
    }
    fn write(&self, id: &str, yaml: &str) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .yaml = yaml.to_string();
        Ok(())
    }
    fn write_prompts(&self, id: &str, prompts: &HashMap<String, String>) {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.entry(id.to_string()).or_default();
        for (node, prompt) in prompts {
            entry.prompts.insert(node.clone(), prompt.clone());
        }
    }
    fn prune_prompts(&self, id: &str, saved: &PipelineDef) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(id) {
            let (kept, _) = split_live_prompts(saved, &entry.prompts);
            entry.prompts = kept;
        }
    }
    fn create(&self, id: &str, yaml: &str) -> Result<PathBuf, CreateFailure> {
        if self.exists(id) {
            return Err(CreateFailure::Exists);
        }
        self.put(id, yaml);
        Ok(self.path(id))
    }
    fn relocate(&self, from: &str, to: &str, yaml: &str) -> Result<PathBuf, String> {
        let mut entries = self.entries.lock().unwrap();
        let mut entry = entries.remove(from).ok_or("no such pipeline")?;
        entry.yaml = yaml.to_string();
        entries.insert(to.to_string(), entry);
        Ok(self.path(to))
    }
    fn publish(
        &self,
        id: &str,
        pipeline: &PipelineDef,
        prompts: &HashMap<String, String>,
    ) -> Result<PathBuf, String> {
        if self.exists(id) {
            return Err("pipeline identity is no longer available".into());
        }
        let (prompts, _) = split_live_prompts(pipeline, prompts);
        self.entries.lock().unwrap().insert(
            id.to_string(),
            Entry {
                yaml: serde_yaml::to_string(pipeline).unwrap(),
                prompts,
            },
        );
        Ok(self.path(id))
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| "delete failed: gone".to_string())
    }
}

#[derive(Default)]
pub struct Library(pub Mutex<Vec<LibraryEntry>>);

impl NodeLibrary for Library {
    fn list(&self) -> Vec<LibraryEntry> {
        let mut all = self.0.lock().unwrap().clone();
        all.sort_by_key(|e| e.name.to_lowercase());
        all
    }
    fn get(&self, name: &str) -> Option<LibraryEntry> {
        self.list().into_iter().find(|e| e.name == name)
    }
    fn save(&self, entry: &LibraryEntry) -> Result<(), String> {
        let mut all = self.0.lock().unwrap();
        all.retain(|e| e.name != entry.name);
        all.push(entry.clone());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<bool, String> {
        let mut all = self.0.lock().unwrap();
        let before = all.len();
        all.retain(|e| e.name != name);
        Ok(all.len() != before)
    }
}

/// Every `pipeline_changed` announced, as `(id, path)`, every
/// `pipeline_validation_changed`, as the id, every `pipeline_draft_changed`,
/// and every `pipeline_version_changed`, as the id.
#[derive(Default)]
pub struct Events(
    pub Mutex<Vec<(String, String)>>,
    pub Mutex<Vec<String>>,
    pub Mutex<Vec<(String, DraftSummary)>>,
    pub Mutex<Vec<String>>,
);

impl Events {
    pub fn validations_changed(&self) -> Vec<String> {
        self.1.lock().unwrap().clone()
    }

    /// The last draft announced for `id`.
    pub fn last_draft(&self, id: &str) -> Option<DraftSummary> {
        self.2
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(of, _)| of == id)
            .map(|(_, draft)| draft.clone())
    }
}

impl PipelineEvents for Events {
    fn changed(&self, id: &str, path: &str) {
        self.0
            .lock()
            .unwrap()
            .push((id.to_string(), path.to_string()));
    }
    fn validation_changed(&self, id: &str) {
        self.1.lock().unwrap().push(id.to_string());
    }
    fn draft_changed(&self, id: &str, draft: &DraftSummary) {
        self.2.lock().unwrap().push((id.to_string(), draft.clone()));
    }
    fn version_changed(&self, id: &str) {
        self.3.lock().unwrap().push(id.to_string());
    }
}

/// The kept drafts, by pipeline id.
#[derive(Default)]
pub struct Drafts(pub Mutex<HashMap<String, StoredDraft>>);

impl DraftStore for Drafts {
    fn load<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<StoredDraft>, String>> {
        ready(Ok(self.0.lock().unwrap().get(id).cloned()))
    }
    fn save<'a>(
        &'a self,
        id: &'a str,
        draft: &'a StoredDraft,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.0.lock().unwrap().insert(id.to_string(), draft.clone());
        ready(Ok(()))
    }
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        self.0.lock().unwrap().remove(id);
        ready(Ok(()))
    }
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        let mut all = self.0.lock().unwrap();
        if let Some(draft) = all.remove(from) {
            all.insert(to.to_string(), draft);
        }
        ready(Ok(()))
    }
    fn summaries(&self) -> BoxFuture<'_, Result<HashMap<String, DraftSummary>, String>> {
        let all = self.0.lock().unwrap();
        ready(Ok(all
            .iter()
            .map(|(id, d)| {
                let summary = DraftSummary {
                    changes: d.changes,
                    nodes: d.nodes,
                    participants: Vec::new(),
                };
                (id.clone(), summary)
            })
            .collect()))
    }
}

/// The stated validations, by pipeline id.
#[derive(Default)]
pub struct Validations(pub Mutex<HashMap<String, PipelineValidation>>);

impl Validations {
    pub fn of(&self, id: &str) -> Option<PipelineValidation> {
        self.0.lock().unwrap().get(id).cloned()
    }
    pub fn set(&self, id: &str, validation: PipelineValidation) {
        self.0.lock().unwrap().insert(id.to_string(), validation);
    }
}

impl PipelineValidations for Validations {
    fn all(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineValidation>, String>> {
        ready(Ok(self.0.lock().unwrap().clone()))
    }
    fn put<'a>(
        &'a self,
        id: &'a str,
        validation: &'a PipelineValidation,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.set(id, validation.clone());
        ready(Ok(()))
    }
    fn forget<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        self.0.lock().unwrap().remove(id);
        ready(Ok(()))
    }
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        let mut all = self.0.lock().unwrap();
        all.remove(to);
        if let Some(validation) = all.remove(from) {
            all.insert(to.to_string(), validation);
        }
        ready(Ok(()))
    }
}

/// The Projets that exist.
#[derive(Default)]
pub struct Projects(pub Mutex<Vec<String>>);

impl Projects {
    pub fn add(&self, id: &str) {
        self.0.lock().unwrap().push(id.to_string());
    }
}

impl KnownProjects for Projects {
    fn exists<'a>(&'a self, project_id: &'a str) -> BoxFuture<'a, Result<bool, String>> {
        ready(Ok(self.0.lock().unwrap().iter().any(|id| id == project_id)))
    }
}

/// How many Runs in flight use each pipeline, and how many launched each
/// of its versions.
#[derive(Default)]
pub struct Runs(
    pub Mutex<HashMap<String, usize>>,
    pub Mutex<HashMap<String, HashMap<u32, usize>>>,
);

impl ActiveRuns for Runs {
    fn using<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, usize> {
        ready(
            self.0
                .lock()
                .unwrap()
                .get(pipeline_id)
                .copied()
                .unwrap_or(0),
        )
    }
    fn per_version<'a>(&'a self, pipeline_id: &'a str) -> BoxFuture<'a, HashMap<u32, usize>> {
        ready(
            self.1
                .lock()
                .unwrap()
                .get(pipeline_id)
                .cloned()
                .unwrap_or_default(),
        )
    }
}

/// The versions, in memory: `id → number → version`.
#[derive(Default)]
pub struct Versions(pub Mutex<BTreeMap<String, BTreeMap<u32, PipelineVersion>>>);

impl Versions {
    /// The numbers and titles of `id`, oldest first.
    pub fn titles(&self, id: &str) -> Vec<(u32, String)> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .map(|versions| {
                versions
                    .values()
                    .map(|v| (v.meta.number, v.meta.title.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn of(&self, id: &str, number: u32) -> Option<PipelineVersion> {
        self.0.lock().unwrap().get(id)?.get(&number).cloned()
    }
}

impl PipelineVersions for Versions {
    fn list<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<VersionMeta>, String>> {
        let listed = self
            .0
            .lock()
            .unwrap()
            .get(id)
            .map(|versions| versions.values().map(|v| v.meta.clone()).collect())
            .unwrap_or_default();
        ready(Ok(listed))
    }
    fn get<'a>(
        &'a self,
        id: &'a str,
        number: u32,
    ) -> BoxFuture<'a, Result<Option<PipelineVersion>, String>> {
        ready(Ok(self.of(id, number)))
    }
    fn put<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.0
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .insert(version.meta.number, version.clone());
        ready(Ok(()))
    }
    fn put_first<'a>(
        &'a self,
        id: &'a str,
        version: &'a PipelineVersion,
    ) -> BoxFuture<'a, Result<(), String>> {
        let mut all = self.0.lock().unwrap();
        let versions = all.entry(id.to_string()).or_default();
        if versions.is_empty() {
            versions.insert(version.meta.number, version.clone());
        }
        ready(Ok(()))
    }
    fn remove<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        self.0.lock().unwrap().remove(id);
        ready(Ok(()))
    }
    fn carry<'a>(&'a self, from: &'a str, to: &'a str) -> BoxFuture<'a, Result<(), String>> {
        let mut all = self.0.lock().unwrap();
        all.remove(to);
        if let Some(versions) = all.remove(from) {
            all.insert(to.to_string(), versions);
        }
        ready(Ok(()))
    }
    fn lasts(&self) -> BoxFuture<'_, Result<HashMap<String, PipelineVersionRef>, String>> {
        let lasts = self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, versions)| {
                versions
                    .values()
                    .last()
                    .map(|v| (id.clone(), v.meta.reference()))
            })
            .collect();
        ready(Ok(lasts))
    }
}

/// Every rename recorded for Stats.
#[derive(Default)]
pub struct Renames(pub Mutex<Vec<Renamed>>);

impl RenameLedger for Renames {
    fn record<'a>(&'a self, rename: &'a Renamed) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().push(rename.clone());
        ready(())
    }
}

/// A bank holding the given skills; an import relabels nothing unless told.
#[derive(Default)]
pub struct Bank {
    pub held: Vec<SidecarSkill>,
    pub report: SidecarImportReport,
    pub refusal: Option<SkillError>,
    pub imported: Mutex<Vec<(String, Vec<SkillRef>, usize)>>,
}

impl SkillBank for Bank {
    fn sidecar_of(&self, references: &[SkillRef]) -> (Vec<SidecarSkill>, Vec<String>) {
        let mut skills = Vec::new();
        let mut absent = Vec::new();
        for reference in references {
            match self.held.iter().find(|s| s.id == reference.id) {
                Some(skill) => skills.push(skill.clone()),
                None => absent.push(reference.id.clone()),
            }
        }
        (skills, absent)
    }
    fn import_sidecar<'a>(
        &'a self,
        _actor: &'a Actor,
        pipeline_name: &'a str,
        references: Vec<SkillRef>,
        sidecar: &'a [SidecarSkill],
    ) -> BoxFuture<'a, Result<SidecarImportReport, SkillError>> {
        self.imported
            .lock()
            .unwrap()
            .push((pipeline_name.to_string(), references, sidecar.len()));
        ready(match &self.refusal {
            Some(error) => Err(error.clone()),
            None => Ok(self.report.clone()),
        })
    }
}

/// The context over in-memory ports, and a handle on each port.
pub struct World {
    pub pipelines: Pipelines,
    pub host: Arc<Host>,
    pub registry: Arc<Registry>,
    pub library: Arc<Library>,
    pub events: Arc<Events>,
    pub runs: Arc<Runs>,
    pub renames: Arc<Renames>,
    pub bank: Arc<Bank>,
    pub validations: Arc<Validations>,
    pub projects: Arc<Projects>,
    pub drafts: Arc<Drafts>,
    pub versions: Arc<Versions>,
}

impl World {
    pub fn new() -> Self {
        Self::with(Host::default(), Registry::default(), Bank::default())
    }

    pub fn refusing(reason: &str) -> Self {
        Self::with(Host::refusing(reason), Registry::default(), Bank::default())
    }

    pub fn with(host: Host, registry: Registry, bank: Bank) -> Self {
        Self::over(
            Arc::new(host),
            Arc::new(registry),
            Arc::new(bank),
            Arc::default(),
            Arc::default(),
        )
    }

    /// The same registry and drafts, as a restarted daemon finds them.
    pub fn restarted(&self) -> Self {
        Self::over(
            self.host.clone(),
            self.registry.clone(),
            self.bank.clone(),
            self.drafts.clone(),
            self.versions.clone(),
        )
    }

    /// The same files and drafts, seen by an Acteur `host` refuses.
    pub fn refusing_over(&self, reason: &str) -> Self {
        Self::over(
            Arc::new(Host::refusing(reason)),
            self.registry.clone(),
            self.bank.clone(),
            self.drafts.clone(),
            self.versions.clone(),
        )
    }

    fn over(
        host: Arc<Host>,
        registry: Arc<Registry>,
        bank: Arc<Bank>,
        drafts: Arc<Drafts>,
        versions: Arc<Versions>,
    ) -> Self {
        let library = Arc::new(Library::default());
        let events = Arc::new(Events::default());
        let runs = Arc::new(Runs::default());
        let renames = Arc::new(Renames::default());
        let validations = Arc::new(Validations::default());
        let projects = Arc::new(Projects::default());
        let pipelines = Pipelines::new(PipelinesPorts {
            registry: registry.clone(),
            library: library.clone(),
            events: events.clone(),
            runs: runs.clone(),
            renames: renames.clone(),
            skills: bank.clone(),
            validations: validations.clone(),
            projects: projects.clone(),
            drafts: drafts.clone(),
            documents: Arc::new(YDraftDocuments),
            versions: versions.clone(),
            extensions: host.clone(),
        });
        Self {
            pipelines,
            host,
            registry,
            library,
            events,
            runs,
            renames,
            bank,
            validations,
            projects,
            drafts,
            versions,
        }
    }

    /// `count` Runs in flight use `id`.
    pub fn busy(&self, id: &str, count: usize) {
        self.runs.0.lock().unwrap().insert(id.to_string(), count);
    }

    /// `count` Runs launched the version `number` of `id`.
    pub fn launched(&self, id: &str, number: u32, count: usize) {
        self.runs
            .1
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .insert(number, count);
    }
}

/// The actor of every request in these tests.
pub fn operator() -> Actor {
    Actor::LocalOperator
}

/// A minimal valid pipeline named `name`.
pub fn pipeline_yaml(name: &str) -> String {
    format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges: []\n"
    )
}

/// A valid pipeline named `name` with one agent node `worker`.
pub fn pipeline_with_worker(name: &str) -> String {
    format!(
        "name: {name}\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n    outputs:\n      - name: out\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n  - source: {{ node: start, port: user_prompt }}\n    target: {{ node: worker, port: in }}\n  - source: {{ node: worker, port: out }}\n    target: {{ node: end, port: result }}\n"
    )
}
