//! In-memory adapters of every port (ADR-0080, stage 2): the use cases are
//! tested against them, fast and without SQLite, processes or the network.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pdo_extension_api::{ActorDisplay, Authorization, RequestParts};
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};

use pdo_platform::application::ports::*;
use pdo_platform::domain::attempt::UpdateAttempt;
use pdo_platform::domain::audit::{AuditEntry, AuditFilter, NewAuditEntry};
use pdo_platform::domain::install::{InstallMethod, Supervision};
use pdo_platform::domain::page_mount::{MountError, PageMount};
use pdo_platform::domain::release::Release;
use pdo_platform::domain::service_unit::PortState;
use pdo_platform::domain::update_check::UpdateCache;
use pdo_platform::domain::update_plan::UpdatePlan;

/// The rule of a [`Host`] that refuses some Operations only.
type Rule = Box<dyn Fn(&Actor, &Operation) -> bool + Send + Sync>;

/// An extension host that refuses everything, allows everything, or refuses
/// what its rule says.
pub struct Host {
    pub refuse: bool,
    pub asked: Mutex<Vec<Operation>>,
    refuses: Option<Rule>,
}

impl Host {
    pub fn allowing() -> Arc<Self> {
        Arc::new(Self {
            refuse: false,
            asked: Mutex::new(Vec::new()),
            refuses: None,
        })
    }

    pub fn refusing() -> Arc<Self> {
        Arc::new(Self {
            refuse: true,
            asked: Mutex::new(Vec::new()),
            refuses: None,
        })
    }

    /// Refuses what `rule` says, with the sentence « You can't. ».
    pub fn refusing_when(
        rule: impl Fn(&Actor, &Operation) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            refuse: false,
            asked: Mutex::new(Vec::new()),
            refuses: Some(Box::new(rule)),
        })
    }

    pub fn asked(&self) -> Vec<Operation> {
        self.asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for Host {
    fn identify(&self, _request: &RequestParts) -> Actor {
        Actor::LocalOperator
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.asked.lock().unwrap().push(operation.clone());
        if self.refuse {
            Authorization::deny("admins only")
        } else if self
            .refuses
            .as_ref()
            .is_some_and(|rule| rule(actor, operation))
        {
            Authorization::deny("not a member").saying("You can't.")
        } else {
            Authorization::Allow
        }
    }

    fn publish(&self, _event: &serde_json::Value) {}

    /// Names the identified `alice` « Alice ».
    fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
        matches!(actor, Actor::Identified { id, .. } if id == "alice")
            .then(|| ActorDisplay::new("Alice"))
    }
}

#[derive(Default)]
pub struct Pages(pub Mutex<BTreeMap<String, PageMount>>);

impl PageMountRepository for Pages {
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async { Ok(()) })
    }
    fn insert<'a>(&'a self, mount: &'a PageMount) -> BoxFuture<'a, Result<(), InsertError>> {
        Box::pin(async move {
            let mut map = self.0.lock().unwrap();
            if map.contains_key(&mount.name) {
                return Err(InsertError::Duplicate);
            }
            map.insert(mount.name.clone(), mount.clone());
            Ok(())
        })
    }
    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().get(name).cloned()) })
    }
    fn list(&self) -> BoxFuture<'_, Result<Vec<PageMount>, StorageError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().values().cloned().collect()) })
    }
    fn remove<'a>(
        &'a self,
        name: &'a str,
    ) -> BoxFuture<'a, Result<Option<PageMount>, StorageError>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().remove(name)) })
    }
    fn remove_for_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<u64, StorageError>> {
        Box::pin(async move {
            let mut map = self.0.lock().unwrap();
            let before = map.len();
            map.retain(|_, m| m.run_id.as_deref() != Some(run_id));
            Ok((before - map.len()) as u64)
        })
    }
    fn place<'a>(
        &'a self,
        name: &'a str,
        owner: Option<&'a Actor>,
        project_id: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            if let Some(mount) = self.0.lock().unwrap().get_mut(name) {
                mount.owner = owner.cloned();
                mount.project_id = project_id.map(str::to_string);
            }
            Ok(())
        })
    }
}

/// Every directory is mountable as itself; files are served from a map.
#[derive(Default)]
pub struct Files(pub BTreeMap<String, Vec<u8>>);

impl PageFiles for Files {
    fn canonical_directory(&self, directory: &Path) -> Result<PathBuf, MountError> {
        if directory.as_os_str() == "/missing" {
            return Err(MountError::Invalid("cannot mount `/missing`".into()));
        }
        Ok(directory.to_path_buf())
    }
    fn exists<'a>(&'a self, root: &'a Path, components: &'a [OsString]) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let mut path = root.to_path_buf();
            path.extend(components);
            self.0.contains_key(&path.display().to_string())
        })
    }
    fn read<'a>(
        &'a self,
        root: &'a Path,
        components: &'a [OsString],
    ) -> BoxFuture<'a, Result<Option<PageFile>, StorageError>> {
        Box::pin(async move {
            let mut path = root.to_path_buf();
            path.extend(components);
            Ok(self.0.get(&path.display().to_string()).map(|c| PageFile {
                contents: c.clone(),
                mime_path: PathBuf::from(components.last().cloned().unwrap_or_default()),
            }))
        })
    }
}

/// The Runs: a set of live sessions, a journal of the commands they issued.
#[derive(Default)]
pub struct FakeRuns {
    pub live_sessions: Vec<(String, String)>,
    pub commands: Mutex<Vec<(String, String, String, serde_json::Value)>>,
    pub journal_fails: bool,
    pub live: usize,
    /// The Runs that exist, with their nodes.
    pub nodes: BTreeMap<String, Vec<String>>,
    /// The Projets each Run was launched in.
    pub projects: BTreeMap<String, Vec<String>>,
    /// Who created each Run.
    pub creators: BTreeMap<String, Actor>,
}

impl Runs for FakeRuns {
    fn verify<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let known = self
                .live_sessions
                .iter()
                .any(|(r, n)| r == run_id && n == node_id);
            known
                .then_some(())
                .ok_or_else(|| format!("session claim refused: run `{run_id}` does not exist"))
        })
    }
    fn record_command<'a>(
        &'a self,
        run_id: &'a str,
        node_id: &'a str,
        command: &'a str,
        page_mount: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if self.journal_fails {
                return Err("event log is locked".into());
            }
            self.commands.lock().unwrap().push((
                run_id.into(),
                node_id.into(),
                command.into(),
                page_mount,
            ));
            Ok(())
        })
    }
    fn count_live(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move { self.live })
    }
    fn node_ids<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Option<Vec<String>>> {
        Box::pin(async move { self.nodes.get(run_id).cloned() })
    }
    fn projects_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Vec<String>> {
        Box::pin(async move { self.projects.get(run_id).cloned().unwrap_or_default() })
    }
    fn creator_of_run<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Option<Actor>> {
        Box::pin(async move { self.creators.get(run_id).cloned() })
    }
}

#[derive(Default)]
pub struct Audit {
    pub rows: Mutex<Vec<NewAuditEntry>>,
    /// The Actor of the request each row was recorded in, as the SQLite
    /// adapter reads it.
    pub actors: Mutex<Vec<Option<Actor>>>,
    pub fails: bool,
}

impl AuditLog for Audit {
    fn init(&self) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async { Ok(()) })
    }
    fn record(&self, entry: NewAuditEntry) -> BoxFuture<'_, Result<(), StorageError>> {
        Box::pin(async move {
            if self.fails {
                return Err(StorageError("database is locked".into()));
            }
            self.rows.lock().unwrap().push(entry);
            self.actors
                .lock()
                .unwrap()
                .push(pdo_kernel::actor::current());
            Ok(())
        })
    }
    fn list<'a>(
        &'a self,
        filter: &'a AuditFilter,
    ) -> BoxFuture<'a, Result<Vec<AuditEntry>, StorageError>> {
        Box::pin(async move {
            let rows = self.rows.lock().unwrap();
            let actors = self.actors.lock().unwrap();
            Ok(rows
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, e)| {
                    filter.target_kind.is_none() || e.target_kind == filter.target_kind
                })
                .take(filter.limit as usize)
                .map(|(i, e)| AuditEntry {
                    id: i as i64 + 1,
                    ts: "t".into(),
                    actor_hint: e.actor_hint.clone(),
                    actor: actors[i].clone(),
                    actor_display: None,
                    action: e.action.clone(),
                    target_kind: e.target_kind.clone(),
                    target_id: e.target_id.clone(),
                    before: e.before.clone(),
                    after: e.after.clone(),
                })
                .collect())
        })
    }
}

pub struct Setting(pub AtomicBool);

impl UpdateCheckSetting for Setting {
    fn enabled(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { self.0.load(Ordering::SeqCst) })
    }
}

/// A release source answering from fixed values, counting its calls.
pub struct Source {
    pub latest: Mutex<Result<String, String>>,
    pub releases: Mutex<Result<Vec<Release>, String>>,
    pub latest_calls: AtomicUsize,
    pub releases_calls: AtomicUsize,
}

impl Source {
    pub fn new(latest: Result<&str, &str>, releases: Result<Vec<Release>, &str>) -> Self {
        Self {
            latest: Mutex::new(latest.map(str::to_string).map_err(str::to_string)),
            releases: Mutex::new(releases.map_err(str::to_string)),
            latest_calls: AtomicUsize::new(0),
            releases_calls: AtomicUsize::new(0),
        }
    }
}

impl ReleaseSource for Source {
    fn latest<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.latest_calls.fetch_add(1, Ordering::SeqCst);
            self.latest.lock().unwrap().clone()
        })
    }
    fn releases<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, Result<Vec<Release>, String>> {
        Box::pin(async move {
            self.releases_calls.fetch_add(1, Ordering::SeqCst);
            self.releases.lock().unwrap().clone()
        })
    }
}

#[derive(Default)]
pub struct Journal {
    pub unavailable: bool,
    pub cache: Mutex<Option<UpdateCache>>,
    pub attempt: Mutex<Option<UpdateAttempt>>,
    pub scripts: Mutex<BTreeMap<String, String>>,
    pub logs: Mutex<BTreeMap<String, String>>,
}

impl UpdateJournal for Journal {
    fn is_available(&self) -> bool {
        !self.unavailable
    }
    fn read_cache(&self) -> Option<UpdateCache> {
        self.cache.lock().unwrap().clone()
    }
    fn write_cache(&self, cache: &UpdateCache) -> Result<(), String> {
        *self.cache.lock().unwrap() = Some(cache.clone());
        Ok(())
    }
    fn read_attempt(&self) -> Option<UpdateAttempt> {
        self.attempt.lock().unwrap().clone()
    }
    fn write_attempt(&self, attempt: &UpdateAttempt) -> Result<(), String> {
        *self.attempt.lock().unwrap() = Some(attempt.clone());
        Ok(())
    }
    fn attempt_path(&self) -> PathBuf {
        PathBuf::from("/home/u/.pdo/update/last-attempt.json")
    }
    fn log_path(&self, attempt_id: &str) -> PathBuf {
        PathBuf::from(format!("/home/u/.pdo/update/{attempt_id}.log"))
    }
    fn write_script(&self, attempt_id: &str, script: &str) -> Result<PathBuf, String> {
        self.scripts
            .lock()
            .unwrap()
            .insert(attempt_id.into(), script.into());
        Ok(PathBuf::from(format!(
            "/home/u/.pdo/update/{attempt_id}.sh"
        )))
    }
    fn append_log(&self, attempt_id: &str, line: &str) {
        let mut logs = self.logs.lock().unwrap();
        let log = logs.entry(attempt_id.into()).or_default();
        log.push_str(line);
        log.push('\n');
    }
    fn read_log(&self, attempt_id: &str) -> Result<Option<String>, String> {
        Ok(self.logs.lock().unwrap().get(attempt_id).cloned())
    }
}

/// An executor whose process exits at once with `exit_code`, or fails to spawn.
pub struct Executor {
    pub exit_code: Option<i32>,
    pub plans: Mutex<Vec<UpdatePlan>>,
}

struct Exited(Option<i32>);

impl RunningExecutor for Exited {
    fn pid(&self) -> u32 {
        4242
    }
    fn wait(self: Box<Self>) -> Option<i32> {
        self.0
    }
}

impl UpdateExecutor for Executor {
    fn spawn(&self, plan: &UpdatePlan, _script: &Path) -> Result<Box<dyn RunningExecutor>, String> {
        self.plans.lock().unwrap().push(plan.clone());
        match self.exit_code {
            Some(code) => Ok(Box::new(Exited(Some(code)))),
            None => Err("cannot spawn the update executor: no sh".into()),
        }
    }
}

pub struct FixedInstallation(pub InstallMethod, pub Supervision);

impl Installation for FixedInstallation {
    fn method(&self) -> InstallMethod {
        self.0
    }
    fn supervision(&self) -> Supervision {
        self.1
    }
    fn invoked_exe(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"))
    }
}

/// A recording `ServiceHost`: fixed identity, configurable port state, real
/// file writes into an injected (temp) config home, and a command log.
pub struct FakeServiceHost {
    pub exe: PathBuf,
    pub cwd: PathBuf,
    pub node_dir: Option<PathBuf>,
    pub config_home: PathBuf,
    pub home: PathBuf,
    pub user: String,
    pub port_state: PortState,
    pub commands: std::cell::RefCell<Vec<(String, Vec<String>)>>,
}

impl FakeServiceHost {
    pub fn new(config_home: PathBuf, port_state: PortState) -> Self {
        Self {
            exe: PathBuf::from("/home/user/.local/bin/pdo"),
            cwd: PathBuf::from("/home/user/.pdo/app"),
            node_dir: Some(PathBuf::from("/opt/node/bin")),
            config_home,
            home: PathBuf::from("/home/user"),
            user: "user".to_string(),
            port_state,
            commands: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn command_log(&self) -> Vec<(String, Vec<String>)> {
        self.commands.borrow().clone()
    }
}

impl ServiceHost for FakeServiceHost {
    fn current_exe(&self) -> anyhow::Result<PathBuf> {
        Ok(self.exe.clone())
    }
    fn current_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(self.cwd.clone())
    }
    fn node_dir(&self) -> Option<PathBuf> {
        self.node_dir.clone()
    }
    fn config_home(&self) -> anyhow::Result<PathBuf> {
        Ok(self.config_home.clone())
    }
    fn home(&self) -> anyhow::Result<PathBuf> {
        Ok(self.home.clone())
    }
    fn user(&self) -> anyhow::Result<String> {
        Ok(self.user.clone())
    }
    fn probe_port(&self, _addr: SocketAddr) -> PortState {
        self.port_state.clone()
    }
    fn write_file(&self, path: &Path, contents: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
        Ok(())
    }
    fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    fn run_cmd(&self, program: &str, args: &[&str]) -> anyhow::Result<CmdOutcome> {
        self.commands.borrow_mut().push((
            program.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        Ok(CmdOutcome {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}
