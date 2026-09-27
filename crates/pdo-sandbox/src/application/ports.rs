//! What the sandbox use cases need from the outside world. The composition root
//! injects the adapters as `Arc<dyn Port>` (D23); a use case test injects in-memory
//! ones.

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use anyhow::Result;

use crate::domain::profile::draft::ProfileDraft;
use crate::domain::profile::ProfileDiff;
use crate::domain::staging::StagedMount;

/// A boxed, sendable future: what an async port method returns (a trait object cannot
/// have `async fn`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What one `docker` invocation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerOutput {
    /// The exit code was 0 — the ONLY verdict. `docker pull` writes its progress on
    /// stderr when it succeeds, so stderr is never a failure signal on its own.
    pub success: bool,
    /// The exit status as the reasons name it (`exit status: 1`).
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

/// The docker CLI (ADR-0030): every image, container and exec operation of the
/// sandbox goes through it. Implemented by
/// [`crate::adapters::docker_cli::ProcessDocker`].
pub trait DockerCli: Send + Sync {
    /// Run `docker <args>` to completion. **Blocking** — async callers wrap it in
    /// `spawn_blocking`. `Err` only when the binary could not be spawned at all: an
    /// [`std::io::ErrorKind::NotFound`] means docker is not installed.
    fn run(&self, args: &[String]) -> std::io::Result<DockerOutput>;
}

/// Where the sandbox Dockerfiles live on disk: the seeded reference copy, the file a
/// profile or the env points at, and the empty build context. Implemented by
/// [`crate::adapters::dockerfiles::FsDockerfiles`].
pub trait DockerfileStore: Send + Sync {
    /// Seed the embedded Dockerfile at its default location
    /// ([`crate::domain::image::default_dockerfile_path`]) when absent, never anywhere
    /// else; an existing file (a user edit) is left alone. Returns the path.
    fn seed_default(&self, sandbox_root: &Path) -> Result<PathBuf>;
    /// Whether `path` is a regular file (a symlink to one counts, a dangling one or a
    /// directory does not).
    fn is_file(&self, path: &Path) -> bool;
    /// The raw bytes of the Dockerfile — the exact input of both the hash and the build.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    /// Create the dedicated, EMPTY build context and return it
    /// ([`crate::domain::image::build_context_dir`]).
    fn ensure_build_context(&self, sandbox_root: &Path) -> std::io::Result<PathBuf>;
}

/// The staged home of a sandboxed Run on disk (#404, #432, #708). Implemented by
/// [`crate::adapters::staging::FsStaging`].
pub trait HomeStaging: Send + Sync {
    /// Whether the Run's staging directory exists.
    fn exists(&self, sandbox_root: &Path, run_id: &str) -> bool;
    /// Materialise the frozen entry list and anchor the fixed mounts.
    fn prepare(
        &self,
        home_root: &Path,
        sandbox_root: &Path,
        entries: &[String],
        run_id: &str,
    ) -> Result<PathBuf>;
    /// The sources of the fixed mounts exist (empty when nothing filled them).
    fn ensure_mount_anchors(&self, sandbox_root: &Path, run_id: &str) -> Result<()>;
    /// The `$HOME`-exception mounts the frozen entry list produces, for what is really
    /// on disk.
    fn extra_mounts(
        &self,
        sandbox_root: &Path,
        run_id: &str,
        host_home: &Path,
        entries: &[String],
    ) -> Vec<StagedMount>;
    /// Copy the staged transcripts back to the host. Best-effort.
    fn merge_back(&self, home_root: &Path, sandbox_root: &Path, run_id: &str) -> Result<()>;
    /// Delete the Run's staging. Best-effort.
    fn teardown(&self, sandbox_root: &Path, run_id: &str) -> Result<()>;
}

/// A storage failure, with the message the operator sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// The materialised staging profiles (ADR-0031 §2): the user's diffs, keyed by name.
/// Implemented by [`crate::adapters::sqlite_profiles::SqliteProfiles`].
pub trait SandboxProfileRepository: Send + Sync {
    /// The stored diff of `name`, `None` when the profile was never edited.
    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<ProfileDiff>, StoreError>>;
    /// Every stored diff, by name.
    fn list(&self) -> BoxFuture<'_, Result<Vec<ProfileDiff>, StoreError>>;
    /// Store `profile` as the diff of `name` — creating the row (editing a virtual
    /// default *is* materialising it) or replacing it whole. Returns the stored row.
    fn upsert<'a>(
        &'a self,
        name: &'a str,
        profile: &'a ProfileDraft,
    ) -> BoxFuture<'a, Result<ProfileDiff, StoreError>>;
    /// Drop the row of `name`; `false` when there was none.
    fn delete<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<bool, StoreError>>;
}

/// The host filesystem as the profile editor sees it: the daemon's home, and what
/// exists under it. Implemented by [`crate::adapters::host::EnvHome`].
pub trait HostHome: Send + Sync {
    /// The daemon's home — the per-daemon override, else `$HOME`; `None` when unknown.
    fn home(&self) -> Option<PathBuf>;
    /// Whether anything is at `path`, a dangling symlink included.
    fn exists(&self, path: &Path) -> bool;
    /// Whether `path` is a regular file (following symlinks).
    fn is_file(&self, path: &Path) -> bool;
}

/// A Trigger that names a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerReferent {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// A live Run launched with a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReferent {
    pub run_id: String,
    pub pipeline_name: Option<String>,
    pub name: Option<String>,
}

/// Who still points at a staging profile (ADR-0031 §7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Referents {
    /// The resolved instance default names it.
    pub instance_default: bool,
    pub triggers: Vec<TriggerReferent>,
    /// Live Runs that froze it at start, and are therefore unaffected by a delete.
    pub runs: Vec<RunReferent>,
}

/// The contexts downstream of the sandbox that store a profile name: the instance
/// settings, the Triggers and the Runs. Implemented where they live.
pub trait ProfileReferents: Send + Sync {
    fn referents<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Referents>;
}
