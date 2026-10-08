//! In-memory adapters of every port of the trigger context, and the context
//! assembled over them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pdo_extension_api::{Authorization, RequestParts, TriggerRef};
use pdo_kernel::event_bus::{BroadcastEventBus, EventBus};
use pdo_kernel::extension_host::{ExtensionHost, TriggerSuspension};
use pdo_kernel::{Actor, Operation};
use pdo_platform::domain::audit::{ActorHint, NewAuditEntry};
use pdo_trigger::application::create_trigger::CreateTriggerRequest;
use pdo_trigger::application::ports::{
    AuditTrail, BoxFuture, GuardRunner, PipelineFacts, Repositories, RunFromTrigger,
    SandboxProfiles, Stored, TriggerPipelines, TriggerProjects, TriggerRepository, TriggerRuns,
    TriggerSettings,
};
use pdo_trigger::domain::fire_decision::GuardResult;
use pdo_trigger::domain::trigger::{FireRecord, NewTrigger, Trigger, TriggerFire, UpdateTrigger};
use pdo_trigger::{Triggers, TriggersOptions, TriggersPorts};
use pdo_workspace::domain::branches::FetchError;
use tokio::sync::broadcast;

/// The Triggers, in memory, newest first.
#[derive(Default)]
pub struct InMemoryTriggers {
    pub triggers: Mutex<Vec<Trigger>>,
    pub fires: Mutex<Vec<TriggerFire>>,
    next: Mutex<u32>,
}

impl InMemoryTriggers {
    pub fn one(&self, id: &str) -> Option<Trigger> {
        self.triggers
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }

    pub fn all(&self) -> Vec<Trigger> {
        self.triggers.lock().unwrap().clone()
    }

    pub fn fires_of(&self, id: &str) -> Vec<TriggerFire> {
        let fires = self.fires.lock().unwrap();
        fires
            .iter()
            .rev()
            .filter(|f| f.trigger_id == id)
            .cloned()
            .collect()
    }
}

fn apply(t: &mut Trigger, e: UpdateTrigger) {
    macro_rules! set {
        ($($field:ident),*) => { $(if let Some(v) = e.$field { t.$field = v; })* };
    }
    set!(
        name,
        pipeline_id,
        pipeline_name,
        target_repo,
        target_repos,
        source_branch,
        input_template,
        variables,
        cron,
        guard_command,
        overlap_policy,
        max_concurrent,
        sandbox,
        harness,
        agent_choice,
        skills,
        auto_name,
        next_fire_at,
        enabled
    );
    if let Some(editor) = e.last_editor {
        t.last_editor = Some(editor);
    }
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

impl TriggerRepository for InMemoryTriggers {
    fn init(&self) -> BoxFuture<'_, Stored<()>> {
        ready(Ok(()))
    }

    fn create(&self, new: NewTrigger) -> BoxFuture<'_, Stored<Trigger>> {
        let mut next = self.next.lock().unwrap();
        *next += 1;
        let trigger = Trigger {
            id: format!("trg-{}", *next),
            name: new.name,
            pipeline_id: new.pipeline_id,
            pipeline_name: new.pipeline_name,
            target_repo: new.target_repo,
            target_repos: new.target_repos,
            source_branch: new.source_branch,
            input_template: new.input_template,
            variables: new.variables,
            cron: new.cron,
            guard_command: new.guard_command,
            overlap_policy: new.overlap_policy,
            max_concurrent: new.max_concurrent,
            sandbox: new.sandbox,
            harness: new.harness,
            agent_choice: new.agent_choice,
            skills: new.skills,
            auto_name: new.auto_name,
            enabled: true,
            next_fire_at: new.next_fire_at,
            last_fired_at: None,
            last_outcome: None,
            attributes: Default::default(),
            last_editor: new.last_editor,
            suspension: None,
        };
        self.triggers.lock().unwrap().insert(0, trigger.clone());
        ready(Ok(trigger))
    }

    fn get<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Option<Trigger>>> {
        ready(Ok(self.one(id)))
    }

    fn list(&self) -> BoxFuture<'_, Stored<Vec<Trigger>>> {
        ready(Ok(self.all()))
    }

    fn delete<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<bool>> {
        let mut triggers = self.triggers.lock().unwrap();
        let before = triggers.len();
        triggers.retain(|t| t.id != id);
        ready(Ok(triggers.len() < before))
    }

    fn update<'a>(&'a self, id: &'a str, edit: UpdateTrigger) -> BoxFuture<'a, Stored<()>> {
        if let Some(t) = self
            .triggers
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.id == id)
        {
            apply(t, edit);
        }
        ready(Ok(()))
    }

    fn due<'a>(&'a self, now: &'a str) -> BoxFuture<'a, Stored<Vec<Trigger>>> {
        let now = chrono::DateTime::parse_from_rfc3339(now).unwrap();
        let due = self
            .all()
            .into_iter()
            .filter(|t| t.enabled)
            .filter(|t| {
                t.next_fire_at
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|at| at <= now)
            })
            .collect();
        ready(Ok(due))
    }

    fn set_next_fire<'a>(
        &'a self,
        id: &'a str,
        next_fire_at: Option<&'a str>,
    ) -> BoxFuture<'a, Stored<()>> {
        if let Some(t) = self
            .triggers
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.id == id)
        {
            t.next_fire_at = next_fire_at.map(str::to_string);
        }
        ready(Ok(()))
    }

    fn record_fire<'a>(&'a self, id: &'a str, record: &'a FireRecord) -> BoxFuture<'a, Stored<()>> {
        let mut fires = self.fires.lock().unwrap();
        let fire = TriggerFire {
            id: fires.len() as i64 + 1,
            trigger_id: id.to_string(),
            ts: pdo_kernel::clock::now_iso(),
            outcome: record.outcome.clone(),
            reason: record.reason.clone(),
            run_id: record.run_id.clone(),
            guard_stdout: record.guard_stdout.clone(),
            guard_stderr: record.guard_stderr.clone(),
            guard_exit_code: record.guard_exit_code,
            source: record.source.clone(),
        };
        fires.push(fire);
        if let Some(t) = self
            .triggers
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.id == id)
        {
            t.last_outcome = Some(record.outcome.clone());
        }
        ready(Ok(()))
    }

    fn fires<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Stored<Vec<TriggerFire>>> {
        ready(Ok(self.fires_of(id)))
    }
}

/// A guard that answers what it is told, and remembers what it ran.
pub struct ScriptedGuard {
    pub answer: Mutex<GuardResult>,
    pub ran: Mutex<Vec<(String, PathBuf, Duration)>>,
}

impl Default for ScriptedGuard {
    fn default() -> Self {
        Self {
            answer: Mutex::new(GuardResult::Pass {
                stdout: "from the guard".into(),
            }),
            ran: Mutex::new(Vec::new()),
        }
    }
}

impl GuardRunner for ScriptedGuard {
    fn run<'a>(
        &'a self,
        command: &'a str,
        cwd: &'a Path,
        timeout: Duration,
    ) -> BoxFuture<'a, GuardResult> {
        self.ran
            .lock()
            .unwrap()
            .push((command.to_string(), cwd.to_path_buf(), timeout));
        ready(self.answer.lock().unwrap().clone())
    }
}

/// The pause and the guard timeout.
pub struct Settings {
    pub paused: Mutex<bool>,
    pub guard_timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            paused: Mutex::new(false),
            guard_timeout: Duration::from_secs(7),
        }
    }
}

impl TriggerSettings for Settings {
    fn paused(&self) -> BoxFuture<'_, Result<bool, String>> {
        ready(Ok(*self.paused.lock().unwrap()))
    }

    fn set_paused(&self, paused: bool) -> BoxFuture<'_, Result<(), String>> {
        *self.paused.lock().unwrap() = paused;
        ready(Ok(()))
    }

    fn guard_timeout(&self) -> BoxFuture<'_, Duration> {
        ready(self.guard_timeout)
    }
}

/// The pipelines: `(id, name, prompt_required)`, and ids whose document is broken.
#[derive(Default)]
pub struct Pipelines {
    pub known: Mutex<Vec<(String, String, bool)>>,
    pub broken: Mutex<Vec<String>>,
}

impl TriggerPipelines for Pipelines {
    fn resolve(&self, pipeline_id: &str) -> Option<Result<PipelineFacts, String>> {
        if self.broken.lock().unwrap().iter().any(|b| b == pipeline_id) {
            return Some(Err("bad yaml".into()));
        }
        self.known
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _, _)| id == pipeline_id)
            .map(|(_, name, prompt_required)| {
                Ok(PipelineFacts {
                    name: name.clone(),
                    prompt_required: *prompt_required,
                })
            })
    }
}

/// The repositories: every absolute path under `/repos/` is one; the fetches
/// are remembered.
#[derive(Default)]
pub struct Repos {
    pub fetched: Mutex<Vec<Vec<String>>>,
    pub fetch_error: Mutex<Option<FetchError>>,
}

pub const TARGET_REPO_REQUIRED: &str = "target_repo is required";

impl Repositories for Repos {
    fn required(&self, field: Option<&str>) -> Result<PathBuf, String> {
        match field.map(str::trim) {
            Some(p) if p.starts_with("/repos/") => Ok(PathBuf::from(p)),
            Some(p) if !p.is_empty() => Err(format!("not a git repository: {p}")),
            _ => Err(TARGET_REPO_REQUIRED.to_string()),
        }
    }

    fn refresh<'a>(&'a self, targets: &'a [String]) -> BoxFuture<'a, Option<FetchError>> {
        self.fetched.lock().unwrap().push(targets.to_vec());
        ready(self.fetch_error.lock().unwrap().clone())
    }
}

/// The Projets of the repositories: those `owners` says, none otherwise.
#[derive(Default)]
pub struct Projets {
    pub owners: Mutex<std::collections::BTreeMap<String, String>>,
}

impl TriggerProjects for Projets {
    fn of_repository<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Vec<String>> {
        ready(
            self.owners
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .into_iter()
                .collect(),
        )
    }
}

/// The staging profiles: `off`, `minimal`, `full`, and `team`.
pub struct Profiles;

impl SandboxProfiles for Profiles {
    fn validate<'a>(&'a self, raw: &'a str) -> BoxFuture<'a, Result<(), String>> {
        let raw = raw.trim();
        ready(match raw {
            "" | "off" | "minimal" | "full" | "team" => Ok(()),
            other => Err(format!("unknown sandbox profile `{other}`")),
        })
    }
}

/// The Runs: how many live per Trigger, and the fires they received.
#[derive(Default)]
pub struct Runs {
    pub live: Mutex<Vec<(String, usize)>>,
    pub started: Mutex<Vec<RunFromTrigger>>,
    pub refusal: Mutex<Option<String>>,
}

impl TriggerRuns for Runs {
    fn live_count<'a>(&'a self, trigger_id: &'a str) -> BoxFuture<'a, usize> {
        let live = self.live.lock().unwrap();
        ready(
            live.iter()
                .find(|(id, _)| id == trigger_id)
                .map_or(0, |(_, n)| *n),
        )
    }

    fn start(&self, fire: RunFromTrigger) -> BoxFuture<'_, Result<String, String>> {
        if let Some(reason) = self.refusal.lock().unwrap().clone() {
            return ready(Err(reason));
        }
        let mut started = self.started.lock().unwrap();
        started.push(fire);
        ready(Ok(format!("run-{}", started.len())))
    }
}

/// The audit journal.
#[derive(Default)]
pub struct Audit {
    pub entries: Mutex<Vec<NewAuditEntry>>,
}

impl AuditTrail for Audit {
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, ()> {
        self.entries.lock().unwrap().push(entry);
        ready(())
    }
}

/// A host that records what it is asked and refuses when told to (every
/// mutation, never a read; or the reads in one Projet); it
/// suspends the Triggers of `suspended` (API 0.8), as the extension `test`.
#[derive(Default)]
pub struct Host {
    pub refuse: Mutex<bool>,
    /// Refuses the reads in this Projet (CORE-5), and only them.
    pub refuse_reads_in: Mutex<Option<String>>,
    pub asked: Mutex<Vec<(Actor, Operation)>>,
    pub suspended: Mutex<std::collections::BTreeMap<String, String>>,
    pub suspension_asks: Mutex<Vec<TriggerRef>>,
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
        let refused_projet = self.refuse_reads_in.lock().unwrap().clone();
        if operation.is_read() && refused_projet.is_some_and(|p| operation.projects.contains(&p)) {
            return Authorization::Deny {
                reason: "the test host refuses this read".into(),
            };
        }
        if *self.refuse.lock().unwrap() && !operation.is_read() {
            Authorization::Deny {
                reason: "the test host refuses everything".into(),
            }
        } else {
            Authorization::Allow
        }
    }

    fn publish(&self, _event: &serde_json::Value) {}

    fn trigger_suspension(&self, trigger: &TriggerRef) -> Option<TriggerSuspension> {
        self.suspension_asks.lock().unwrap().push(trigger.clone());
        self.suspended
            .lock()
            .unwrap()
            .get(&trigger.id)
            .map(|reason| TriggerSuspension {
                extension: "test".into(),
                reason: reason.clone(),
            })
    }
}

/// The context over fresh fakes, and the fakes.
pub struct World {
    pub triggers: Arc<Triggers>,
    pub store: Arc<InMemoryTriggers>,
    pub guard: Arc<ScriptedGuard>,
    pub settings: Arc<Settings>,
    pub pipelines: Arc<Pipelines>,
    pub repos: Arc<Repos>,
    pub projets: Arc<Projets>,
    pub runs: Arc<Runs>,
    pub audit: Arc<Audit>,
    pub host: Arc<Host>,
    pub bus: broadcast::Receiver<serde_json::Value>,
}

/// A prompt-optional pipeline every test can fire.
pub const PIPE: &str = "watch-pipe";
/// The repository every test names.
pub const REPO: &str = "/repos/app";

pub fn world() -> World {
    world_with(TriggersOptions {
        repo_root: PathBuf::from("/daemon/root"),
        panic_on_trigger_name: None,
    })
}

pub fn world_with(options: TriggersOptions) -> World {
    let store = Arc::new(InMemoryTriggers::default());
    let guard = Arc::new(ScriptedGuard::default());
    let settings = Arc::new(Settings::default());
    let pipelines = Arc::new(Pipelines::default());
    pipelines.known.lock().unwrap().extend([
        (PIPE.into(), PIPE.into(), false),
        ("weekly-pipe".into(), "Weekly".into(), false),
        ("needs-prompt".into(), "Needs prompt".into(), true),
    ]);
    let repos = Arc::new(Repos::default());
    let projets = Arc::new(Projets::default());
    let runs = Arc::new(Runs::default());
    let audit = Arc::new(Audit::default());
    let host = Arc::new(Host::default());
    let bus = Arc::new(BroadcastEventBus::new(64));
    let rx = bus.subscribe();
    let triggers = Triggers::new(
        TriggersPorts {
            store: store.clone(),
            guards: guard.clone(),
            settings: settings.clone(),
            pipelines: pipelines.clone(),
            repositories: repos.clone(),
            projects: projets.clone(),
            sandbox_profiles: Arc::new(Profiles),
            runs: runs.clone(),
            audit: audit.clone(),
            bus,
            extensions: host.clone(),
        },
        options,
    );
    World {
        triggers,
        store,
        guard,
        settings,
        pipelines,
        repos,
        projets,
        runs,
        audit,
        host,
        bus: rx,
    }
}

impl World {
    pub fn refuse(&self) {
        *self.host.refuse.lock().unwrap() = true;
    }

    /// Have the extension suspend `id`, saying `reason`.
    pub fn suspend(&self, id: &str, reason: &str) {
        self.host
            .suspended
            .lock()
            .unwrap()
            .insert(id.to_string(), reason.to_string());
    }

    /// Every message published on the bus so far.
    pub fn announced(&mut self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        while let Ok(message) = self.bus.try_recv() {
            out.push(message);
        }
        out
    }

    /// Create a Trigger as the local operator, from the UI; its id.
    pub async fn create(&self, req: CreateTriggerRequest) -> String {
        self.triggers
            .create(&Actor::LocalOperator, ActorHint::Ui, req)
            .await
            .expect("the creation passes")
            .id
    }

    /// Create the standard Trigger (`T`, `watch-pipe`, Monday 04:00).
    pub async fn create_standard(&self) -> String {
        self.create(request(serde_json::json!({}))).await
    }
}

/// A create request: the standard fields, overridden by `extra`.
pub fn request(extra: serde_json::Value) -> CreateTriggerRequest {
    let mut body = serde_json::json!({
        "name": "T",
        "pipeline_id": PIPE,
        "cron": "0 4 * * 1",
        "input_template": "x",
        "target_repo": REPO,
    });
    for (k, v) in extra.as_object().unwrap() {
        if v.is_null() {
            body.as_object_mut().unwrap().remove(k);
        } else {
            body[k] = v.clone();
        }
    }
    serde_json::from_value(body).unwrap()
}

/// The guard of what the Trigger `id` announces, outside any Projet: its read.
pub fn read_guard(id: &str) -> serde_json::Value {
    serde_json::json!({ "resource": "trigger", "verb": "read", "target": id, "projects": [] })
}
