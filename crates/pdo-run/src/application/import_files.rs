//! `ImportFiles`: files from the user's machine given to a node that holds a
//! live session (« Fichier importé en cours de Run »). They land in the Run's
//! Blackboard under `.pdo/artifacts/_attachments/<node-id>/` of its worktree:
//! never committed, removed with the Run. The agent is not told; the UI hands
//! the user a text to send it.

use std::io::Write;
use std::path::{Path, PathBuf};

use pdo_kernel::authorization::Denied;
use pdo_kernel::Actor;
use pdo_session::domain::declared_wait::{live_target, WaitRefusal};
use pdo_session::domain::names::node_session_name;
use pdo_workspace::adapters::git::worktrees::worktree_dir_for_run;
use pdo_workspace::domain::worktree_layout::RUN_ARTIFACTS_RELATIVE;

use crate::application::events::load_events;
use crate::application::operations::on_node;
use crate::application::paths::effective_repo_root;
use crate::domain::blackboard::{node_attachments_dir, suffixed_name};
use crate::domain::live_session::wait_facts;
use crate::domain::projection::project;
use crate::Runs;

/// One file of an import, its name already checked.
pub struct ImportFile {
    pub name: String,
    pub data: Vec<u8>,
}

/// One file once written: its final name (suffixed when the name was taken)
/// and its path relative to the Run's worktree root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// Where an import goes, once every precondition held: the node's live
/// iteration, and the worktree its files land in.
#[derive(Debug, Clone)]
pub struct ImportTarget {
    pub iter: i64,
    worktree: PathBuf,
    relative_dir: PathBuf,
}

/// Why nothing was imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    Refused(Denied),
    /// The node holds no session that could receive the files: why, and the
    /// iteration asked about.
    Session(WaitRefusal, i64),
    /// The Run's worktree is not on disk.
    WorktreeMissing,
    /// The Run's log could not be read.
    Storage(String),
}

impl Runs {
    /// The first step of an import, before its body is read: the Autorisation
    /// d'opération (`Node × import_files`), then the node's live session (the
    /// projection, then tmux), then the Run's worktree.
    pub async fn import_target(
        &self,
        actor: &Actor,
        run_id: &str,
        node_id: &str,
        iter: Option<i64>,
    ) -> Result<ImportTarget, ImportError> {
        self.authorize(actor, &on_node("import_files", run_id, node_id))
            .await
            .map_err(ImportError::Refused)?;
        let events = load_events(self, run_id)
            .await
            .map_err(|e| ImportError::Storage(e.to_string()))?;
        let run = project(&events);
        let facts = run.as_ref().map(|run| wait_facts(run, node_id));
        let iter = live_target(facts.as_ref(), iter)
            .map_err(|(refusal, iter)| ImportError::Session(refusal, iter))?;
        if !self
            .tmux
            .session_exists(&node_session_name(run_id, node_id, iter))
        {
            return Err(ImportError::Session(WaitRefusal::NodeSessionNotLive, iter));
        }
        let Some(run) = run else {
            return Err(ImportError::Session(WaitRefusal::RunNotFound, iter));
        };
        let worktree = worktree_dir_for_run(&effective_repo_root(self, &run), run_id);
        if !worktree.is_dir() {
            return Err(ImportError::WorktreeMissing);
        }
        Ok(ImportTarget {
            iter,
            worktree,
            relative_dir: node_attachments_dir(Path::new(RUN_ARTIFACTS_RELATIVE), node_id),
        })
    }
}

impl ImportTarget {
    /// The folder the files land in, relative to the worktree root.
    pub fn relative_dir(&self) -> &Path {
        &self.relative_dir
    }

    /// Write every file, in order. A name already present (on disk, or earlier
    /// in the same import) is never overwritten: the file takes the first free
    /// `-1`, `-2`… suffix. All or nothing: on a write error, what this call
    /// wrote is removed before the error is returned.
    pub fn write(&self, files: &[ImportFile]) -> std::io::Result<Vec<ImportedFile>> {
        let dir = self.worktree.join(&self.relative_dir);
        let created = !dir.exists();
        std::fs::create_dir_all(&dir)?;
        let mut written: Vec<ImportedFile> = Vec::new();
        let result = files
            .iter()
            .try_for_each(|file| self.write_one(&dir, file, &mut written));
        if let Err(e) = result {
            for file in &written {
                let _ = std::fs::remove_file(self.worktree.join(&file.path));
            }
            if created {
                // Only empty folders go: another node's imports stay.
                let _ = std::fs::remove_dir(&dir);
                if let Some(parent) = dir.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            return Err(e);
        }
        // Out of `git status` whatever the repository ignores: the Blackboard
        // is never committed anyway (the delivery leaves the artifacts out).
        if let Some(attachments) = dir.parent() {
            let ignore = attachments.join(".gitignore");
            if !ignore.exists() {
                let _ = std::fs::write(ignore, "*\n");
            }
        }
        Ok(written)
    }

    fn write_one(
        &self,
        dir: &Path,
        file: &ImportFile,
        written: &mut Vec<ImportedFile>,
    ) -> std::io::Result<()> {
        let mut n = 0;
        loop {
            let name = if n == 0 {
                file.name.clone()
            } else {
                suffixed_name(&file.name, n)
            };
            // `create_new` makes the "free name" check and the claim one step:
            // a concurrent import never overwrites this one.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dir.join(&name))
            {
                Ok(mut handle) => {
                    written.push(ImportedFile {
                        path: self.relative_dir.join(&name).to_string_lossy().into_owned(),
                        name,
                        size: file.data.len() as u64,
                    });
                    return handle.write_all(&file.data);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                // A concurrent import that failed took the empty folder back
                // between our creation and this file: make it again.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !dir.exists() => {
                    std::fs::create_dir_all(dir)?
                }
                Err(e) => return Err(e),
            }
        }
    }
}
