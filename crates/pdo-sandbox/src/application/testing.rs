//! In-memory adapters for the use case tests (ADR-0080, stage 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use pdo_extension_api::Authorization;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation};

use super::ports::{
    BoxFuture, DockerCli, DockerOutput, DockerfileStore, HomeStaging, HostHome, ProfileReferents,
    Referents, SandboxProfileRepository, StoreError,
};
use crate::domain::image::default_dockerfile_path;
use crate::domain::profile::draft::ProfileDraft;
use crate::domain::profile::ProfileDiff;
use crate::domain::staging::StagedMount;

type Answer = dyn Fn(&[String]) -> std::io::Result<DockerOutput> + Send + Sync;

/// A docker that answers from a script and records every argv.
pub(crate) struct ScriptedDocker {
    answer: Box<Answer>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl ScriptedDocker {
    pub(crate) fn answering(
        answer: impl Fn(&[String]) -> std::io::Result<DockerOutput> + Send + Sync + 'static,
    ) -> Self {
        Self {
            answer: Box::new(answer),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Every invocation succeeds with an empty output.
    pub(crate) fn happy() -> Self {
        Self::answering(|_| Self::ok(""))
    }

    /// No docker binary at all.
    pub(crate) fn absent() -> Self {
        Self::answering(|_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)))
    }

    pub(crate) fn ok(stdout: &str) -> std::io::Result<DockerOutput> {
        Ok(DockerOutput {
            success: true,
            status: "exit status: 0".into(),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    pub(crate) fn fail(code: i32, stderr: &str) -> std::io::Result<DockerOutput> {
        Ok(DockerOutput {
            success: false,
            status: format!("exit status: {code}"),
            stdout: String::new(),
            stderr: stderr.into(),
        })
    }

    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    /// The first word of every invocation, in order.
    pub(crate) fn verbs(&self) -> Vec<String> {
        self.calls().into_iter().map(|c| c[0].clone()).collect()
    }
}

impl DockerCli for ScriptedDocker {
    fn run(&self, args: &[String]) -> std::io::Result<DockerOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        (self.answer)(args)
    }
}

/// Dockerfiles in memory: one file per path, and a record of what was seeded.
#[derive(Default)]
pub(crate) struct InMemoryDockerfiles {
    files: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    embedded: Vec<u8>,
    seeded: Mutex<Vec<PathBuf>>,
    contexts: Mutex<Vec<PathBuf>>,
}

impl InMemoryDockerfiles {
    /// The embedded Dockerfile is `embedded`; nothing is on disk yet.
    pub(crate) fn seeded(embedded: &[u8]) -> Self {
        Self {
            embedded: embedded.to_vec(),
            ..Self::default()
        }
    }

    pub(crate) fn seeded_roots(&self) -> Vec<PathBuf> {
        self.seeded.lock().unwrap().clone()
    }

    pub(crate) fn build_contexts(&self) -> Vec<PathBuf> {
        self.contexts.lock().unwrap().clone()
    }
}

impl DockerfileStore for InMemoryDockerfiles {
    fn seed_default(&self, sandbox_root: &Path) -> Result<PathBuf> {
        let path = default_dockerfile_path(sandbox_root);
        self.files
            .lock()
            .unwrap()
            .entry(path.clone())
            .or_insert_with(|| self.embedded.clone());
        self.seeded.lock().unwrap().push(sandbox_root.to_path_buf());
        Ok(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn ensure_build_context(&self, sandbox_root: &Path) -> std::io::Result<PathBuf> {
        let dir = crate::domain::image::build_context_dir(sandbox_root);
        self.contexts.lock().unwrap().push(dir.clone());
        Ok(dir)
    }
}

/// A staged home in memory: which Runs have a staging, and what was done to it.
#[derive(Default)]
pub(crate) struct InMemoryStaging {
    staged: Mutex<Vec<String>>,
    events: Mutex<Vec<String>>,
}

impl InMemoryStaging {
    pub(crate) fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
}

impl HomeStaging for InMemoryStaging {
    fn exists(&self, _sandbox_root: &Path, run_id: &str) -> bool {
        self.staged.lock().unwrap().iter().any(|r| r == run_id)
    }

    fn prepare(
        &self,
        _home_root: &Path,
        sandbox_root: &Path,
        entries: &[String],
        run_id: &str,
    ) -> Result<PathBuf> {
        self.staged.lock().unwrap().push(run_id.to_string());
        self.record(format!("prepare {run_id} [{}]", entries.join(",")));
        Ok(sandbox_root.join(run_id))
    }

    fn ensure_mount_anchors(&self, _sandbox_root: &Path, run_id: &str) -> Result<()> {
        self.record(format!("anchors {run_id}"));
        Ok(())
    }

    fn extra_mounts(
        &self,
        sandbox_root: &Path,
        run_id: &str,
        host_home: &Path,
        entries: &[String],
    ) -> Vec<StagedMount> {
        entries
            .iter()
            .filter(|e| !e.starts_with(".claude"))
            .map(|e| StagedMount {
                source: sandbox_root.join(run_id).join("home").join(e),
                target: host_home.join(e),
            })
            .collect()
    }

    fn merge_back(&self, _home_root: &Path, _sandbox_root: &Path, run_id: &str) -> Result<()> {
        self.record(format!("merge_back {run_id}"));
        Ok(())
    }

    fn teardown(&self, _sandbox_root: &Path, run_id: &str) -> Result<()> {
        self.staged.lock().unwrap().retain(|r| r != run_id);
        self.record(format!("teardown {run_id}"));
        Ok(())
    }
}

/// Profiles in memory.
#[derive(Default)]
pub(crate) struct InMemoryProfiles {
    rows: Mutex<BTreeMap<String, ProfileDiff>>,
}

impl InMemoryProfiles {
    pub(crate) fn seed(&self, diff: ProfileDiff) {
        self.rows.lock().unwrap().insert(diff.name.clone(), diff);
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.rows.lock().unwrap().keys().cloned().collect()
    }
}

impl SandboxProfileRepository for InMemoryProfiles {
    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<ProfileDiff>, StoreError>> {
        Box::pin(async move { Ok(self.rows.lock().unwrap().get(name).cloned()) })
    }

    fn list(&self) -> BoxFuture<'_, Result<Vec<ProfileDiff>, StoreError>> {
        Box::pin(async move { Ok(self.rows.lock().unwrap().values().cloned().collect()) })
    }

    fn upsert<'a>(
        &'a self,
        name: &'a str,
        profile: &'a ProfileDraft,
    ) -> BoxFuture<'a, Result<ProfileDiff, StoreError>> {
        Box::pin(async move {
            let diff = ProfileDiff {
                name: name.to_string(),
                disabled: profile.disabled.clone(),
                extras: profile.extras.clone(),
                env: profile.env.clone(),
                image: profile.image.clone(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            };
            self.rows
                .lock()
                .unwrap()
                .insert(name.to_string(), diff.clone());
            Ok(diff)
        })
    }

    fn delete<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<bool, StoreError>> {
        Box::pin(async move { Ok(self.rows.lock().unwrap().remove(name).is_some()) })
    }
}

/// A home whose content is a fixed list of paths.
pub(crate) struct FakeHome {
    pub(crate) home: Option<PathBuf>,
    pub(crate) present: Vec<PathBuf>,
}

impl FakeHome {
    pub(crate) fn at(home: &str, present: &[&str]) -> Self {
        Self {
            home: Some(PathBuf::from(home)),
            present: present.iter().map(PathBuf::from).collect(),
        }
    }
}

impl HostHome for FakeHome {
    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn exists(&self, path: &Path) -> bool {
        self.present.iter().any(|p| p == path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.exists(path)
    }
}

/// Referents fixed in advance.
#[derive(Default)]
pub(crate) struct FixedReferents(pub(crate) Referents);

impl ProfileReferents for FixedReferents {
    fn referents<'a>(&'a self, _name: &'a str) -> BoxFuture<'a, Referents> {
        Box::pin(async move { self.0.clone() })
    }
}

/// An extension host that refuses every Operation, and records what it was asked.
#[derive(Default)]
pub(crate) struct Refusing {
    asked: Mutex<Vec<(Actor, Operation)>>,
}

impl Refusing {
    pub(crate) fn asked(&self) -> Vec<(Actor, Operation)> {
        self.asked.lock().unwrap().clone()
    }
}

impl ExtensionHost for Refusing {
    fn identify(&self, _request: &pdo_extension_api::RequestParts) -> Actor {
        Actor::LocalOperator
    }

    fn authorize(&self, actor: &Actor, operation: &Operation) -> Authorization {
        self.asked
            .lock()
            .unwrap()
            .push((actor.clone(), operation.clone()));
        Authorization::deny("admins only")
    }

    fn publish(&self, _event: &serde_json::Value) {}
}
