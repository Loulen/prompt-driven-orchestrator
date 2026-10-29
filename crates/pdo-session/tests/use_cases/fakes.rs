//! In-memory adapters of every port (ADR-0080, stage 2): the session use cases
//! are tested against them, fast and without tmux, a Run store or a terminal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{Authorization, RequestParts, SessionLaunch};
use pdo_kernel::extension_host::{ExtensionHost, NodeContribution};
use pdo_kernel::{Actor, Operation};

use pdo_session::application::hosted::{
    AdmissionSlot, HostedHarness, HostedPlaces, HostedStore, NotAdmitted, SessionAdmission,
};
use pdo_session::application::ports::*;
use pdo_session::domain::declared_wait::WaitFacts;
use pdo_session::domain::hosted::HostedRecord;
use pdo_session::domain::launch::ContainerExec;
use pdo_session::domain::pane::{PaneTarget, ReattachOutcome};
use pdo_session::domain::reaper::{NodeRunInfo, SweepTtls};

/// An extension host that refuses everything, or allows everything, and
/// remembers what it was asked; it gives every session launch
/// [`Host::contribution`] and keeps every event announced or published.
#[derive(Default)]
pub struct Host {
    pub refuse: bool,
    pub asked: Mutex<Vec<Operation>>,
    pub contribution: Mutex<NodeContribution>,
    pub launches: Mutex<Vec<SessionLaunch>>,
    /// Every event: `(on the bus, event)`; `false` for the extensions only.
    pub events: Mutex<Vec<(bool, serde_json::Value)>>,
}

impl Host {
    pub fn allowing() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn refusing() -> Arc<Self> {
        Arc::new(Self {
            refuse: true,
            ..Self::default()
        })
    }

    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }

    pub fn events(&self) -> Vec<(bool, serde_json::Value)> {
        self.events.lock().unwrap().clone()
    }
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }

    fn authorize(&self, _actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        if self.refuse {
            Authorization::deny("admins only")
        } else {
            Authorization::Allow
        }
    }

    fn publish(&self, event: &serde_json::Value) {
        self.events.lock().unwrap().push((false, event.clone()));
    }

    fn announce(&self, event: serde_json::Value) {
        self.events.lock().unwrap().push((true, event));
    }

    fn installed(&self) -> Vec<String> {
        vec!["fake".into()]
    }

    fn session_launch(&self, launch: &SessionLaunch) -> NodeContribution {
        self.launches.lock().unwrap().push(launch.clone());
        self.contribution.lock().unwrap().clone()
    }
}

/// A tmux server in memory: sessions are names, panes are strings.
#[derive(Default)]
pub struct FakeTmux {
    pub sessions: Mutex<BTreeMap<String, String>>,
    pub attached: Mutex<HashSet<String>>,
    pub created: Mutex<Vec<(String, PathBuf, String)>>,
    pub killed: Mutex<Vec<String>>,
    pub typed: Mutex<Vec<(String, String)>>,
    pub pasted: Mutex<Vec<(String, String)>>,
    /// What `paste_without_enter` wrote, per session (no Enter).
    pub pasted_without_enter: Mutex<Vec<(String, String)>>,
    /// `paste_without_enter` is refused.
    pub refuse_paste: Mutex<bool>,
    /// The next `new_session` fails with this message.
    pub fail_next_spawn: Mutex<Option<String>>,
    /// `kill` leaves the session alive.
    pub unkillable: Mutex<bool>,
    /// Runs once, inside the next `new_session`, before the session exists.
    #[allow(clippy::type_complexity)]
    pub on_next_spawn: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl FakeTmux {
    pub fn with_sessions(names: &[&str]) -> Arc<Self> {
        let tmux = Self::default();
        for name in names {
            tmux.sessions
                .lock()
                .unwrap()
                .insert(name.to_string(), format!("pane of {name}"));
        }
        Arc::new(tmux)
    }

    pub fn has(&self, name: &str) -> bool {
        self.sessions.lock().unwrap().contains_key(name)
    }
}

impl Tmux for FakeTmux {
    fn socket(&self) -> &str {
        "pdo-fake"
    }

    fn session_path(&self) -> String {
        "/usr/bin:/bin".into()
    }

    fn new_session(
        &self,
        name: &str,
        working_dir: &Path,
        script: &str,
        _what: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(message) = self.fail_next_spawn.lock().unwrap().take() {
            anyhow::bail!("{message}");
        }
        let hook = self.on_next_spawn.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        self.created
            .lock()
            .unwrap()
            .push((name.into(), working_dir.into(), script.into()));
        self.sessions
            .lock()
            .unwrap()
            .insert(name.into(), "Connecting...".into());
        Ok(())
    }

    fn kill(&self, name: &str) {
        self.killed.lock().unwrap().push(name.into());
        if !*self.unkillable.lock().unwrap() {
            self.sessions.lock().unwrap().remove(name);
        }
    }

    fn session_exists(&self, name: &str) -> bool {
        self.has(name)
    }

    fn server_alive(&self) -> Option<bool> {
        Some(!self.sessions.lock().unwrap().is_empty())
    }

    fn list_pdo_sessions(&self) -> HashSet<String> {
        self.sessions
            .lock()
            .unwrap()
            .keys()
            .filter(|n| n.starts_with("pdo-"))
            .cloned()
            .collect()
    }

    fn is_attached(&self, name: &str) -> bool {
        self.attached.lock().unwrap().contains(name)
    }

    fn capture(&self, name: &str) -> Option<String> {
        self.sessions.lock().unwrap().get(name).cloned()
    }

    fn send_keys(&self, name: &str, text: &str) {
        self.typed.lock().unwrap().push((name.into(), text.into()));
    }

    fn paste(&self, name: &str, text: &str) {
        self.pasted.lock().unwrap().push((name.into(), text.into()));
    }

    fn paste_without_enter(&self, name: &str, text: &str) -> bool {
        if *self.refuse_paste.lock().unwrap() {
            return false;
        }
        self.pasted_without_enter
            .lock()
            .unwrap()
            .push((name.into(), text.into()));
        true
    }

    fn attach_argv(&self, name: &str, ignore_size: bool) -> Vec<String> {
        let mut argv = vec![
            "tmux".to_string(),
            "attach".into(),
            "-t".into(),
            name.into(),
        ];
        if ignore_size {
            argv.push("ignore-size".into());
        }
        argv
    }

    fn attach_command(&self, name: &str) -> String {
        format!("tmux attach -t {name}")
    }

    fn set_client_ignore_size(&self, _client_pid: u32, _ignore_size: bool) -> bool {
        true
    }
}

/// The Run store, as the declared wait reads and writes it.
#[derive(Default)]
pub struct Waits {
    pub facts: Mutex<HashMap<(String, String), WaitFacts>>,
    pub unreadable: Mutex<bool>,
    pub awaiting: Mutex<Vec<(String, String, i64, serde_json::Value)>>,
    pub resumed: Mutex<Vec<(String, String, i64, serde_json::Value)>>,
    pub refuse_writes: Mutex<bool>,
}

impl Waits {
    pub fn with(run_id: &str, node_id: &str, facts: WaitFacts) -> Arc<Self> {
        let waits = Self::default();
        waits
            .facts
            .lock()
            .unwrap()
            .insert((run_id.into(), node_id.into()), facts);
        Arc::new(waits)
    }
}

impl NodeWaits for Waits {
    fn wait_facts<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<WaitFacts>, String>> {
        Box::pin(async move {
            if *self.unreadable.lock().unwrap() {
                return Err("pool timed out".into());
            }
            Ok(self
                .facts
                .lock()
                .unwrap()
                .get(&(run_id.to_string(), node_id.to_string()))
                .cloned())
        })
    }

    fn record_awaiting<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if *self.refuse_writes.lock().unwrap() {
                return Err("run is terminal".into());
            }
            self.awaiting
                .lock()
                .unwrap()
                .push((run_id.into(), node_id.into(), iter, payload));
            Ok(())
        })
    }

    fn record_resumed<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        iter: i64,
        payload: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.resumed
                .lock()
                .unwrap()
                .push((run_id.into(), node_id.into(), iter, payload));
            Ok(())
        })
    }
}

/// The Run facts of a pane, and a scripted re-attach.
pub struct Panes {
    pub target: Result<Option<PaneTarget>, RunLookup>,
    pub reattach: ReattachOutcome,
    pub reattached: Mutex<Vec<String>>,
    /// Session a successful re-attach brings back, in this tmux.
    pub tmux: Option<Arc<FakeTmux>>,
}

impl NodePanes for Panes {
    fn pane_target<'a>(
        &'a self,
        _run_id: &'a str,
        _node_id: &'a str,
        _iter: i64,
    ) -> BoxFuture<'a, Result<Option<PaneTarget>, RunLookup>> {
        Box::pin(async move { self.target.clone() })
    }

    fn reattach<'a>(
        &'a self,
        _run_id: &'a str,
        _node_id: &'a str,
        _iter: i64,
        session: &'a str,
    ) -> BoxFuture<'a, ReattachOutcome> {
        Box::pin(async move {
            self.reattached.lock().unwrap().push(session.into());
            if self.reattach == ReattachOutcome::Resumed {
                if let Some(tmux) = &self.tmux {
                    tmux.sessions
                        .lock()
                        .unwrap()
                        .insert(session.into(), "resumed pane".into());
                }
            }
            self.reattach.clone()
        })
    }
}

/// A `docker exec` that names its container.
pub struct Exec;

impl ContainerExec for Exec {
    fn exec_argv(&self, env: &[(String, String)]) -> Vec<String> {
        let mut argv = vec!["docker".to_string(), "exec".into()];
        for (k, v) in env {
            argv.push("-e".into());
            argv.push(format!("{k}={v}"));
        }
        argv.push("pdo-sbx-r1".into());
        argv
    }
}

/// The Runs a shell opens in.
pub struct Shells {
    pub eligible: bool,
    pub worktree: PathBuf,
    pub sandboxed: bool,
    pub lookup: Option<RunLookup>,
    pub sandbox_error: Option<String>,
    pub readied: Mutex<Vec<String>>,
}

impl RunShells for Shells {
    fn shell_target<'a>(
        &'a self,
        _run_id: &'a str,
    ) -> BoxFuture<'a, Result<ShellTarget, RunLookup>> {
        Box::pin(async move {
            if let Some(lookup) = &self.lookup {
                return Err(lookup.clone());
            }
            Ok(ShellTarget {
                eligible: self.eligible,
                worktree: self.worktree.clone(),
                sandbox: self
                    .sandboxed
                    .then(|| Box::new(Exec) as Box<dyn ContainerExec>),
            })
        })
    }

    fn ensure_sandbox_ready<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.readied.lock().unwrap().push(run_id.into());
            match &self.sandbox_error {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        })
    }
}

/// The owners of the live sessions.
pub struct Owners {
    pub facts: HashMap<(String, String), NodeRunInfo>,
    pub unreadable: bool,
    pub ttls: SweepTtls,
    pub asked: Mutex<Vec<(String, String)>>,
}

impl SessionOwners for Owners {
    fn owners<'a>(
        &'a self,
        owners: &'a [(String, String)],
    ) -> BoxFuture<'a, Result<Vec<Option<NodeRunInfo>>, String>> {
        Box::pin(async move {
            if self.unreadable {
                return Err("pool timed out".into());
            }
            self.asked.lock().unwrap().extend(owners.iter().cloned());
            Ok(owners.iter().map(|o| self.facts.get(o).cloned()).collect())
        })
    }

    fn ttls(&self) -> BoxFuture<'_, SweepTtls> {
        Box::pin(async move { self.ttls })
    }
}

/// The status bar's gauges.
pub struct Gauges {
    pub live: usize,
    pub cap: usize,
}

impl DaemonGauges for Gauges {
    fn live_sessions(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move { self.live })
    }

    fn session_cap(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move { self.cap })
    }

    fn service_health(&self) -> serde_json::Value {
        serde_json::json!({ "supervisor": "systemd", "persistent": false })
    }
}

/// A save the assistant asked for: the template id, its YAML, its prompts.
pub type Save = (String, String, HashMap<String, String>);

/// The templates the assistant reads and saves into.
pub struct Templates {
    pub root: PathBuf,
    pub saved: Mutex<Vec<Save>>,
    pub invalid: Option<TemplateSaveError>,
    /// The Actors whose assistant left a draft, and which.
    pub left: Mutex<Vec<(Actor, String)>>,
}

impl pdo_session::application::ports::Templates for Templates {
    fn path_of(&self, pipeline_id: &str) -> PathBuf {
        self.root.join(format!("{pipeline_id}.yaml"))
    }

    fn read<'a>(
        &'a self,
        _actor: &'a Actor,
        pipeline_id: &'a str,
    ) -> BoxFuture<'a, Result<TemplateDraft, TemplateSaveError>> {
        let draft = TemplateDraft {
            yaml: format!("name: {pipeline_id}\n"),
            prompts: Default::default(),
        };
        Box::pin(async move { Ok(draft) })
    }

    fn save<'a>(
        &'a self,
        _actor: &'a Actor,
        pipeline_id: &'a str,
        yaml: &'a str,
        prompts: &'a HashMap<String, String>,
    ) -> BoxFuture<'a, Result<TemplateSaved, TemplateSaveError>> {
        let saved = match &self.invalid {
            Some(e) => Err(e.clone()),
            None => {
                self.saved
                    .lock()
                    .unwrap()
                    .push((pipeline_id.into(), yaml.into(), prompts.clone()));
                Ok(TemplateSaved {
                    fields: vec!["name".to_string()],
                    version: None,
                })
            }
        };
        Box::pin(async move { saved })
    }

    fn left(&self, actor: &Actor, pipeline_id: &str) {
        self.left
            .lock()
            .unwrap()
            .push((actor.clone(), pipeline_id.to_string()));
    }
}

/// The assistant's harness.
pub struct Harness(pub pdo_harness::registry::HarnessDescriptor);

impl AssistantHarness for Harness {
    fn resolve(&self) -> BoxFuture<'_, pdo_harness::registry::HarnessDescriptor> {
        Box::pin(async move { self.0.clone() })
    }
}

/// An OS terminal window that records what it opened.
#[derive(Default)]
pub struct Window {
    pub opened: Mutex<Vec<(String, String)>>,
    pub fail: bool,
}

impl TerminalWindow for Window {
    fn open(&self, session: &str, attach: &str) -> anyhow::Result<String> {
        if self.fail {
            anyhow::bail!("no display");
        }
        self.opened
            .lock()
            .unwrap()
            .push((session.into(), attach.into()));
        Ok("kitty".into())
    }
}

/// The Run [`InP1`] puts in the Projet `p1`.
pub const RUN_IN_P1: &str = "20260912-100000-a11ce01";

/// The Projets of the Runs: [`RUN_IN_P1`] is in `p1`, every other Run in none.
pub struct InP1;

impl pdo_session::application::ports::RunProjects for InP1 {
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async move {
            if run_id == RUN_IN_P1 {
                vec!["p1".to_string()]
            } else {
                Vec::new()
            }
        })
    }
}

/// The records of the Sessions hébergées in memory, their state folders in a
/// temporary folder.
pub struct HostedRecords {
    pub records: Mutex<Vec<HostedRecord>>,
    pub root: tempfile::TempDir,
}

impl Default for HostedRecords {
    fn default() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            root: tempfile::tempdir().unwrap(),
        }
    }
}

impl HostedStore for HostedRecords {
    fn load(&self) -> Vec<HostedRecord> {
        self.records.lock().unwrap().clone()
    }

    fn save(&self, record: &HostedRecord) -> Result<(), String> {
        let mut records = self.records.lock().unwrap();
        match records.iter_mut().find(|r| r.id() == record.id()) {
            Some(stored) => *stored = record.clone(),
            None => records.push(record.clone()),
        }
        Ok(())
    }

    fn remove(&self, id: &str) {
        self.records.lock().unwrap().retain(|r| r.id() != id);
    }

    fn state_dir(&self, id: &str) -> PathBuf {
        self.root.path().join(id)
    }
}

/// Where a Session hébergée's folder resolves: one home, one Projet `p1`
/// whose primary repository is `repo`.
pub struct Places {
    pub home: Option<PathBuf>,
    pub repo: Option<PathBuf>,
}

impl HostedPlaces for Places {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn project_repo<'a>(&'a self, project: &'a str) -> BoxFuture<'a, Option<PathBuf>> {
        let repo = (project == "p1").then(|| self.repo.clone()).flatten();
        Box::pin(async move { repo })
    }
}

/// The harnesses: `claude` by default or by name, nothing else.
pub struct Harnesses;

impl HostedHarness for Harnesses {
    fn resolve<'a>(
        &'a self,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Option<pdo_harness::registry::HarnessDescriptor>> {
        let found = match name {
            None | Some("claude") => Some(pdo_harness::registry::claude()),
            Some(_) => None,
        };
        Box::pin(async move { found })
    }
}

/// The Cap: full or not, and how many slots were given back.
#[derive(Default)]
pub struct Cap {
    pub full: Mutex<bool>,
    pub released: Mutex<usize>,
}

impl SessionAdmission for Cap {
    fn admit(&self) -> BoxFuture<'_, Result<AdmissionSlot, NotAdmitted>> {
        let full = *self.full.lock().unwrap();
        Box::pin(async move {
            if full {
                Err(NotAdmitted::CapReached { live: 2, cap: 2 })
            } else {
                Ok(AdmissionSlot::holding(()))
            }
        })
    }

    fn released(&self) {
        *self.released.lock().unwrap() += 1;
    }
}
