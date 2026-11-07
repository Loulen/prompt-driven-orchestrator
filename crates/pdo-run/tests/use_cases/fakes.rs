//! In-memory adapters of every port of the run context.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts};
use pdo_harness::agent_choice::ResolvedCombo;
use pdo_kernel::event_bus::EventBus;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};
use pdo_run::application::ports::*;
use pdo_run::domain::event::{Event, EventKind};
use pdo_run::domain::run_summary::{summarize, RunSummary};
use pdo_run::domain::state::{CostStat, RunState};
use pdo_run::{Runs, RunsOptions, RunsPorts};
use pdo_sandbox::application::ports::{SandboxProfileRepository, StoreError};
use pdo_sandbox::domain::profile::draft::ProfileDraft;
use pdo_sandbox::domain::profile::ProfileDiff;
use pdo_session::application::ports::Tmux;
use pdo_skill::domain::delivery::SkippedSkill;
use pdo_skill::domain::selection::ActiveSkill;
use pdo_workspace::domain::project::Project;
use pdo_workspace::domain::provisioning::{ProvisioningRules, ProvisioningScope, ScopedRules};
use tokio::sync::broadcast;

/// The event log, as a list, with the Résumé de Run of each (ADR-0084). A
/// Run listed in `forgotten` refuses every append; `broken` makes every read
/// fail; `failing_summary` makes the next append fail once its event is
/// written, as a failed transaction would: neither the event nor the Résumé
/// stays. `summaries_current` is the mark of a rebuild.
#[derive(Default)]
pub struct Store {
    pub events: Mutex<Vec<Event>>,
    pub forgotten: Mutex<HashSet<String>>,
    pub broken: bool,
    pub summaries: Mutex<BTreeMap<String, RunSummary>>,
    pub summaries_current: Mutex<bool>,
    pub failing_summary: Mutex<bool>,
}

impl EventStore for Store {
    fn append<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            if self.forgotten.lock().unwrap().contains(&event.run_id) {
                anyhow::bail!("run {} has been forgotten", event.run_id);
            }
            let mut events = self.events.lock().unwrap();
            let mut stored = event.clone();
            stored.id = Some(events.len() as i64 + 1);
            let mut log: Vec<Event> = events
                .iter()
                .filter(|e| e.run_id == event.run_id)
                .cloned()
                .collect();
            log.push(stored.clone());
            if std::mem::take(&mut *self.failing_summary.lock().unwrap()) {
                anyhow::bail!("failed to write the run summary");
            }
            let mut summaries = self.summaries.lock().unwrap();
            match summarize(&log) {
                Some(summary) => summaries.insert(event.run_id.clone(), summary),
                None => summaries.remove(&event.run_id),
            };
            events.push(stored);
            Ok(())
        })
    }

    fn events<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<Event>>> {
        Box::pin(async move {
            if self.broken {
                anyhow::bail!("failed to load events");
            }
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.run_id == run_id)
                .cloned()
                .collect())
        })
    }

    fn run_ids(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>> {
        Box::pin(async move {
            let mut ids: Vec<String> = self
                .events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.run_id.clone())
                .collect();
            ids.sort();
            ids.dedup();
            ids.reverse();
            Ok(ids)
        })
    }

    fn is_forgotten<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { Ok(self.forgotten.lock().unwrap().contains(run_id)) })
    }

    fn has_children<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.events.lock().unwrap().iter().any(|e| {
                e.kind == EventKind::RunStarted
                    && e.payload
                        .as_ref()
                        .and_then(|p| p.get("parent_run_id"))
                        .and_then(|v| v.as_str())
                        == Some(run_id)
            })
        })
    }

    fn child_run_ids<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move {
            if self.broken {
                anyhow::bail!("failed to read the children");
            }
            let field = |e: &Event, key: &str| {
                e.payload
                    .as_ref()
                    .and_then(|p| p.get(key))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            let mut ids: Vec<String> = self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| {
                    e.kind == EventKind::RunStarted
                        && field(e, "parent_run_id").as_deref() == Some(run_id)
                        && field(e, "parent_node_id").as_deref() == Some(node_id)
                })
                .map(|e| e.run_id.clone())
                .collect();
            ids.dedup();
            Ok(ids)
        })
    }

    fn forget<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            self.forgotten.lock().unwrap().insert(run_id.to_string());
            self.events.lock().unwrap().retain(|e| e.run_id != run_id);
            self.summaries.lock().unwrap().remove(run_id);
            Ok(())
        })
    }

    fn summaries(&self) -> BoxFuture<'_, anyhow::Result<Vec<RunSummary>>> {
        Box::pin(async move {
            if self.broken {
                anyhow::bail!("failed to load the run summaries");
            }
            Ok(self
                .summaries
                .lock()
                .unwrap()
                .values()
                .rev()
                .cloned()
                .collect())
        })
    }

    fn summary<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, anyhow::Result<Option<RunSummary>>> {
        Box::pin(async move { Ok(self.summaries.lock().unwrap().get(run_id).cloned()) })
    }

    fn summaries_current(&self) -> BoxFuture<'_, anyhow::Result<bool>> {
        Box::pin(async move { Ok(*self.summaries_current.lock().unwrap()) })
    }

    fn replace_summaries(&self, all: Vec<RunSummary>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            *self.summaries.lock().unwrap() = all
                .into_iter()
                .map(|summary| (summary.run_id.clone(), summary))
                .collect();
            *self.summaries_current.lock().unwrap() = true;
            Ok(())
        })
    }
}

/// The bus, recording what was published.
pub struct Bus {
    pub published: Mutex<Vec<serde_json::Value>>,
    tx: broadcast::Sender<serde_json::Value>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            published: Mutex::default(),
            tx: broadcast::channel(16).0,
        }
    }
}

impl EventBus for Bus {
    fn publish(&self, event: serde_json::Value) {
        self.published.lock().unwrap().push(event.clone());
        let _ = self.tx.send(event);
    }

    fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.tx.subscribe()
    }
}

/// The disk, as a lines-of-code delta it writes on every Run it fills.
#[derive(Default)]
pub struct Disk {
    pub filled: Mutex<Vec<(String, usize)>>,
}

impl LiveFacts for Disk {
    fn fill<'a>(&'a self, run: &'a mut RunState, events: &'a [Event]) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.filled
                .lock()
                .unwrap()
                .push((run.run_id.clone(), events.len()));
            run.loc = Some(pdo_run::domain::state::LocStat {
                insertions: 3,
                deletions: 1,
                files_changed: 1,
            });
        })
    }

    fn cost<'a>(
        &'a self,
        _run: &'a RunState,
        _events: &'a [Event],
    ) -> BoxFuture<'a, Option<CostStat>> {
        Box::pin(async move {
            Some(CostStat {
                usd: 1.5,
                partial: false,
                unpriced_models: Vec::new(),
                uncosted_harnesses: Vec::new(),
                by_harness: Vec::new(),
            })
        })
    }
}

/// The managers with a live session.
#[derive(Default)]
pub struct Managers(pub Mutex<HashSet<String>>);

impl ManagerSessions for Managers {
    fn exists(&self, run_id: &str) -> bool {
        self.0.lock().unwrap().contains(run_id)
    }
}

/// tmux, as a set of live sessions: every session the context opens is
/// recorded, every kill is recorded, and a session "exists" while it is live.
#[derive(Default)]
pub struct FakeTmux {
    pub live: Mutex<HashSet<String>>,
    pub opened: Mutex<Vec<(String, PathBuf, String)>>,
    pub killed: Mutex<Vec<String>>,
    pub typed: Mutex<Vec<(String, String)>>,
}

impl Tmux for FakeTmux {
    fn socket(&self) -> &str {
        "pdo-test-fake"
    }
    fn session_path(&self) -> String {
        String::new()
    }
    fn new_session(
        &self,
        name: &str,
        working_dir: &Path,
        script: &str,
        _what: Option<&str>,
    ) -> anyhow::Result<()> {
        self.live.lock().unwrap().insert(name.to_string());
        self.opened.lock().unwrap().push((
            name.to_string(),
            working_dir.to_path_buf(),
            script.to_string(),
        ));
        Ok(())
    }
    fn kill(&self, name: &str) {
        self.live.lock().unwrap().remove(name);
        self.killed.lock().unwrap().push(name.to_string());
    }
    fn session_exists(&self, name: &str) -> bool {
        self.live.lock().unwrap().contains(name)
    }
    fn server_alive(&self) -> Option<bool> {
        Some(true)
    }
    fn list_pdo_sessions(&self) -> HashSet<String> {
        self.live.lock().unwrap().clone()
    }
    fn is_attached(&self, _name: &str) -> bool {
        false
    }
    fn capture(&self, _name: &str) -> Option<String> {
        None
    }
    fn send_keys(&self, name: &str, text: &str) {
        self.typed
            .lock()
            .unwrap()
            .push((name.to_string(), text.to_string()));
    }
    fn paste(&self, name: &str, text: &str) {
        self.send_keys(name, text);
    }
    fn paste_without_enter(&self, name: &str, text: &str) -> bool {
        self.send_keys(name, text);
        true
    }
    fn press_enter(&self, _name: &str) -> bool {
        true
    }
    fn attach_argv(&self, _name: &str, _ignore_size: bool) -> Vec<String> {
        Vec::new()
    }
    fn attach_command(&self, _name: &str) -> String {
        String::new()
    }
    fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
        false
    }
}

/// The declared waits the context asked for.
#[derive(Default)]
pub struct Waits(pub Mutex<Vec<(String, String, i64)>>);

impl DeclaredWaits for Waits {
    fn on_completion_not_released<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .push((run_id.to_string(), node_id.to_string(), iter));
        })
    }
}

/// The Projets, as a list; `broken` makes every read fail. The Projet each
/// provisioning read was asked for is recorded.
#[derive(Default)]
pub struct FakeProjects(
    pub Mutex<Vec<Project>>,
    pub Mutex<bool>,
    pub Mutex<Vec<Option<String>>>,
);

impl Projects for FakeProjects {
    fn project<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Project>, String>> {
        Box::pin(async move {
            if *self.1.lock().unwrap() {
                return Err("the workspace store is unreadable".to_string());
            }
            Ok(self.0.lock().unwrap().iter().find(|p| p.id == id).cloned())
        })
    }

    fn provisioning_rules<'a>(
        &'a self,
        project: Option<&'a str>,
        run: ProvisioningRules,
    ) -> BoxFuture<'a, Result<Vec<ScopedRules>, String>> {
        self.2.lock().unwrap().push(project.map(str::to_string));
        Box::pin(async move {
            Ok(vec![ScopedRules {
                scope: ProvisioningScope::Run,
                rules: run,
            }])
        })
    }
}

/// The skill bank, and the deliveries asked of it.
#[derive(Default)]
pub struct FakeSkills {
    pub bank: Mutex<BTreeMap<String, String>>,
    pub delivered: Mutex<Vec<(PathBuf, Vec<String>)>>,
    pub withdrawn: Mutex<Vec<(PathBuf, String)>>,
}

impl NodeSkills for FakeSkills {
    fn bank_names(&self) -> BoxFuture<'_, BTreeMap<String, String>> {
        Box::pin(async move { self.bank.lock().unwrap().clone() })
    }
    fn deliver(
        &self,
        worktree: &Path,
        _run_repo_root: &Path,
        _run_id: &str,
        skills: &[ActiveSkill],
    ) -> anyhow::Result<Vec<SkippedSkill>> {
        self.delivered.lock().unwrap().push((
            worktree.to_path_buf(),
            skills.iter().map(|s| s.id.clone()).collect(),
        ));
        Ok(Vec::new())
    }
    fn withdraw(&self, repo_root: &Path, run_id: &str) -> anyhow::Result<()> {
        self.withdrawn
            .lock()
            .unwrap()
            .push((repo_root.to_path_buf(), run_id.to_string()));
        Ok(())
    }
}

/// No staging profile was ever edited.
#[derive(Default)]
pub struct NoProfiles;

impl SandboxProfileRepository for NoProfiles {
    fn get<'a>(
        &'a self,
        _name: &'a str,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<Option<ProfileDiff>, StoreError>>
    {
        Box::pin(async { Ok(None) })
    }
    fn list(
        &self,
    ) -> pdo_sandbox::application::ports::BoxFuture<'_, Result<Vec<ProfileDiff>, StoreError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn upsert<'a>(
        &'a self,
        _name: &'a str,
        _profile: &'a ProfileDraft,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<ProfileDiff, StoreError>> {
        Box::pin(async { Err(StoreError("read-only in the tests".into())) })
    }
    fn delete<'a>(
        &'a self,
        _name: &'a str,
    ) -> pdo_sandbox::application::ports::BoxFuture<'a, Result<bool, StoreError>> {
        Box::pin(async { Ok(false) })
    }
}

/// The instance settings; `None` reads as an unreadable store.
#[derive(Default)]
pub struct Settings {
    pub defaults: Mutex<Option<InstanceDefaults>>,
    pub profiles: Mutex<BTreeMap<String, ResolvedCombo>>,
}

impl InstanceSettings for Settings {
    fn defaults(&self) -> BoxFuture<'_, Option<InstanceDefaults>> {
        Box::pin(async move { self.defaults.lock().unwrap().clone() })
    }
    fn agent_profiles(&self) -> BoxFuture<'_, BTreeMap<String, ResolvedCombo>> {
        Box::pin(async move { self.profiles.lock().unwrap().clone() })
    }
    fn profile_named<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<ResolvedCombo>, String>> {
        Box::pin(async move {
            Ok(self
                .profiles
                .lock()
                .unwrap()
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, combo)| combo.clone()))
        })
    }
}

/// The page mounts: every release recorded, every hold granted.
#[derive(Default)]
pub struct Pages(pub Mutex<Vec<String>>);

impl RunPages for Pages {
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>> {
        Box::pin(async { Box::new(()) as Box<dyn Send> })
    }
    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(run_id.to_string());
            Ok(())
        })
    }
}

/// The pipeline watcher: every Run directory registered.
#[derive(Default)]
pub struct Watcher(pub Mutex<Vec<PathBuf>>);

impl RunDirWatch for Watcher {
    fn watch(&self, run_dir: &Path) {
        self.0.lock().unwrap().push(run_dir.to_path_buf());
    }
}

/// A shared pipeline in memory: its YAML and its prompts by node.
pub type SharedPipeline = (String, HashMap<String, String>);

/// The shared pipelines, in memory: `id → (yaml, prompts)`.
#[derive(Default)]
pub struct Shared(pub Mutex<HashMap<String, SharedPipeline>>);

impl SourcePipelines for Shared {
    fn exists(&self, id: &str) -> bool {
        self.0.lock().unwrap().contains_key(id)
    }
    fn fingerprint(&self, id: &str) -> Option<String> {
        let shared = self.0.lock().unwrap();
        let (yaml, prompts) = shared.get(id)?;
        Some(pdo_pipeline::domain::fingerprint::fingerprint(
            yaml, prompts,
        ))
    }
    fn overwrite<'a>(
        &'a self,
        _actor: &'a Actor,
        id: &'a str,
        yaml: &'a str,
        prompts: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<(), OverwriteRefusal>> {
        let mut shared = self.0.lock().unwrap();
        let Some(entry) = shared.get_mut(id) else {
            return Box::pin(async { Err(OverwriteRefusal::NotFound) });
        };
        *entry = (yaml.to_string(), prompts.clone());
        Box::pin(async { Ok(()) })
    }
    fn last_version<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Option<pdo_pipeline::domain::version::PipelineVersionRef>> {
        let known = self.0.lock().unwrap().contains_key(id);
        Box::pin(async move {
            known.then(|| pdo_pipeline::domain::version::PipelineVersionRef {
                number: 1,
                title: "v1".into(),
            })
        })
    }
}

/// The Triggers, in memory: `(pipeline id, Trigger)`.
#[derive(Default)]
pub struct Triggers(pub Mutex<Vec<(String, PipelineTrigger)>>);

impl PipelineTriggers for Triggers {
    fn firing<'a>(
        &'a self,
        pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<PipelineTrigger>, String>> {
        let firing = self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| id == pipeline_id)
            .map(|(_, trigger)| trigger.clone())
            .collect();
        Box::pin(async move { Ok(firing) })
    }
}

/// An empty bank, as the pipeline context reads it.
pub struct NoBank;

impl pdo_pipeline::application::ports::SkillBank for NoBank {
    fn sidecar_of(
        &self,
        references: &[pdo_skill::domain::selection::SkillRef],
    ) -> (Vec<pdo_skill::domain::sidecar::SidecarSkill>, Vec<String>) {
        (
            Vec::new(),
            references.iter().map(|r| r.id.clone()).collect(),
        )
    }
    fn import_sidecar<'a>(
        &'a self,
        _actor: &'a Actor,
        _pipeline_name: &'a str,
        _references: Vec<pdo_skill::domain::selection::SkillRef>,
        _sidecar: &'a [pdo_skill::domain::sidecar::SidecarSkill],
    ) -> pdo_pipeline::application::ports::BoxFuture<
        'a,
        Result<
            pdo_skill::domain::sidecar::SidecarImportReport,
            pdo_skill::domain::error::SkillError,
        >,
    > {
        Box::pin(async { Ok(Default::default()) })
    }
}

/// An extension host that records every Operation it is asked about, and
/// refuses them all once told to; or only the reads in the Projets
/// `refuse_reads_in`.
#[derive(Default)]
pub struct Host {
    pub refuse: Mutex<bool>,
    pub refuse_reads_in: Mutex<Vec<String>>,
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
        let refused_projects = self.refuse_reads_in.lock().unwrap();
        if operation.is_read()
            && operation
                .projects
                .iter()
                .any(|p| refused_projects.contains(p))
        {
            Authorization::deny("the test host refuses this read")
        } else if *self.refuse.lock().unwrap() {
            Authorization::deny("the test host refuses everything")
        } else {
            Authorization::Allow
        }
    }
    fn publish(&self, _event: &serde_json::Value) {}
}

/// The context over fresh fakes, and the fakes.
pub struct World {
    pub runs: Arc<Runs>,
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    pub disk: Arc<Disk>,
    pub managers: Arc<Managers>,
    pub tmux: Arc<FakeTmux>,
    pub waits: Arc<Waits>,
    pub projects: Arc<FakeProjects>,
    pub skills: Arc<FakeSkills>,
    pub settings: Arc<Settings>,
    pub pages: Arc<Pages>,
    pub watcher: Arc<Watcher>,
    pub shared: Arc<Shared>,
    pub triggers: Arc<Triggers>,
    pub host: Arc<Host>,
    /// The daemon's repo root, a fresh directory.
    pub repo: tempfile::TempDir,
}

pub fn world() -> World {
    world_over(Store::default())
}

pub fn world_over(store: Store) -> World {
    let store = Arc::new(store);
    let bus = Arc::new(Bus::default());
    let disk = Arc::new(Disk::default());
    let managers = Arc::new(Managers::default());
    let tmux = Arc::new(FakeTmux::default());
    let waits = Arc::new(Waits::default());
    let projects = Arc::new(FakeProjects::default());
    let skills = Arc::new(FakeSkills::default());
    let settings = Arc::new(Settings::default());
    let pages = Arc::new(Pages::default());
    let watcher = Arc::new(Watcher::default());
    let shared = Arc::new(Shared::default());
    let triggers = Arc::new(Triggers::default());
    let host = Arc::new(Host::default());
    let repo = tempfile::tempdir().unwrap();
    let runs = Runs::new(
        RunsPorts {
            store: store.clone(),
            bus: bus.clone(),
            live_facts: disk.clone(),
            managers: managers.clone(),
            tmux: tmux.clone(),
            sessions: Arc::new(
                pdo_session::application::session_ledger::SessionLedger::new(
                    tmux.clone(),
                    host.clone(),
                ),
            ),
            waits: waits.clone(),
            projects: projects.clone(),
            skills: skills.clone(),
            sandbox_profiles: Arc::new(NoProfiles),
            settings: settings.clone(),
            bank: Arc::new(NoBank),
            pages: pages.clone(),
            run_dirs: watcher.clone(),
            self_writes: pdo_pipeline::adapters::self_writes::SelfWrites::new(),
            source_pipelines: shared.clone(),
            pipeline_triggers: triggers.clone(),
            extensions: host.clone(),
        },
        RunsOptions {
            daemon_port: 6172,
            allowed_ws_origins: Vec::new(),
            repo_root: repo.path().to_path_buf(),
            tmux_cmd_override: Some("exec true".into()),
            sandbox_home_override: Some(repo.path().join("home")),
            ..RunsOptions::default()
        },
    );
    World {
        runs,
        store,
        bus,
        disk,
        managers,
        tmux,
        waits,
        projects,
        skills,
        settings,
        pages,
        watcher,
        shared,
        triggers,
        host,
        repo,
    }
}

pub fn event(run_id: &str, kind: EventKind, node_id: Option<&str>) -> Event {
    Event {
        id: None,
        run_id: run_id.into(),
        ts: "2026-09-27T12:00:00.000Z".into(),
        kind,
        node_id: node_id.map(String::from),
        iter: node_id.map(|_| 1),
        payload: None,
    }
}

/// A `RunStarted` with one agent node, `worker`.
pub fn run_started(run_id: &str) -> Event {
    let mut started = event(run_id, EventKind::RunStarted, None);
    started.payload = Some(serde_json::json!({
        "pipeline_name": "p",
        "node_defs": [{
            "id": "worker", "node_type": "agent", "inputs": [], "outputs": [],
            "view_x": null, "view_y": null
        }],
        "edges": [],
    }));
    started
}

/// `start → worker → second → end`: two agent nodes in a row, both working in
/// the Run's shared worktree.
pub const RELAY: &str = r#"name: relay
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
  - id: worker
    name: Worker
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: second
    name: Second
    type: agent
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
  - id: end
    name: End
    type: end
    inputs:
      - name: result
edges:
  - source: { node: start, port: user_prompt }
    target: { node: worker, port: task }
  - source: { node: worker, port: out }
    target: { node: second, port: task }
  - source: { node: second, port: out }
    target: { node: end, port: result }
"#;

/// A started Run of the [`RELAY`] pipeline in the world's repository: its
/// run-scoped pipeline and prompts, its worktree, its `RunStarted`.
pub async fn seed_relay(w: &World, run_id: &str) {
    seed_relay_in(w, run_id, None).await
}

/// [`seed_relay`], the Run launched in the Projet `project` (ADR-0087).
pub async fn seed_relay_in(w: &World, run_id: &str, project: Option<&str>) {
    let repo = w.repo.path();
    let run_dir = repo.join(".pdo").join("runs").join(run_id);
    std::fs::create_dir_all(run_dir.join("worktree").join(".pdo").join("artifacts")).unwrap();
    std::fs::write(run_dir.join("pipeline.yaml"), RELAY).unwrap();
    let prompts = run_dir.join("pipeline.prompts");
    std::fs::create_dir_all(&prompts).unwrap();
    std::fs::write(prompts.join("worker.md"), "Do the work.\n").unwrap();
    std::fs::write(prompts.join("second.md"), "Do the rest.\n").unwrap();
    let mut started = event(run_id, EventKind::RunStarted, None);
    started.payload = Some(serde_json::json!({
        "pipeline_name": "relay",
        "input": "go",
        "target_repo": repo.to_string_lossy(),
        "project_id": project,
    }));
    w.runs.append_event().execute(&started).await.unwrap();
}

/// The world's repository as a git repository holding the `relay` pipeline.
pub fn relay_repository(w: &World) {
    let repo = w.repo.path();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(repo.join(".gitignore"), ".pdo/\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);
    let pipelines = repo.join(".pdo").join("pipelines");
    std::fs::create_dir_all(pipelines.join("relay.prompts")).unwrap();
    std::fs::write(pipelines.join("relay.yaml"), RELAY).unwrap();
    std::fs::write(pipelines.join("relay.prompts/worker.md"), "Do the work.\n").unwrap();
    std::fs::write(pipelines.join("relay.prompts/second.md"), "Do the rest.\n").unwrap();
}

/// Write the artifact `node_id`'s `out` port expects, where its agent would.
pub fn write_out(w: &World, run_id: &str, node_id: &str) {
    let dir = w
        .repo
        .path()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree/.pdo/artifacts")
        .join(node_id)
        .join("iter-1/out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("output.md"), "artifact\n").unwrap();
}

/// The kinds of the events `run_id`'s log holds, in order.
pub fn kinds(w: &World, run_id: &str) -> Vec<EventKind> {
    w.store
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.run_id == run_id)
        .map(|e| e.kind.clone())
        .collect()
}

/// Poll the log until `check` accepts it (the terminal tails run detached).
pub async fn eventually(w: &World, run_id: &str, what: &str, check: impl Fn(&[EventKind]) -> bool) {
    for _ in 0..200 {
        if check(&kinds(w, run_id)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "{what}: never happened; the log holds {:?}",
        kinds(w, run_id)
    );
}

/// A started Run of `yaml` (a variant of [`RELAY`]) in the world's repository.
pub async fn seed_run_of(w: &World, run_id: &str, yaml: &str) {
    seed_relay(w, run_id).await;
    let run_dir = w.repo.path().join(".pdo").join("runs").join(run_id);
    std::fs::write(run_dir.join("pipeline.yaml"), yaml).unwrap();
}
