//! The instance registry on disk: `<id>.yaml` and `<id>.prompts/<node>.md`
//! under `~/.pdo/pipelines`. Every write marks its path first, so the
//! watcher does not announce the daemon's own saves as outside edits.

mod entries;
mod files;
mod paths;
mod unify;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::application::ports::{CreateFailure, PipelineRegistry, RegistryFile};
use crate::domain::pipeline::{self, PipelineDef};

pub use files::{pipeline_files, pipeline_prompts, read_prompts_dir};
pub use paths::{home_dir, instance_pipeline_dir, resolve_pipeline_path};
pub use unify::unify_legacy_registries;

use super::self_writes::SelfWrites;

/// The message of a registry whose directory cannot be determined.
pub const NO_REGISTRY: &str = "cannot determine instance pipeline directory";

/// The instance registry of a daemon started in `repo_root`.
pub struct FsPipelineRegistry {
    repo_root: PathBuf,
    self_writes: SelfWrites,
}

impl FsPipelineRegistry {
    pub fn new(repo_root: impl Into<PathBuf>, self_writes: SelfWrites) -> Self {
        Self {
            repo_root: repo_root.into(),
            self_writes,
        }
    }

    fn write_marked(&self, path: &Path, content: &str) -> std::io::Result<()> {
        self.self_writes.mark(path);
        std::fs::write(path, content)
    }
}

impl PipelineRegistry for FsPipelineRegistry {
    fn dir(&self) -> Option<PathBuf> {
        instance_pipeline_dir(&self.repo_root)
    }

    fn path_of(&self, id: &str) -> PathBuf {
        resolve_pipeline_path(&self.repo_root, id)
    }

    fn unify_legacy(&self) -> Result<(), String> {
        unify_legacy_registries(&self.repo_root)
    }

    fn files(&self) -> Vec<RegistryFile> {
        self.dir()
            .map(|dir| pipeline_files(&dir))
            .unwrap_or_default()
    }

    fn exists(&self, id: &str) -> bool {
        self.path_of(id).exists()
    }

    fn read(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(self.path_of(id)).ok()
    }

    fn prompts(&self, id: &str) -> HashMap<String, String> {
        pipeline_prompts(&self.path_of(id))
    }

    fn write(&self, id: &str, yaml: &str) -> Result<(), String> {
        self.write_marked(&self.path_of(id), yaml)
            .map_err(|e| format!("write failed: {e}"))
    }

    fn write_prompts(&self, id: &str, prompts: &HashMap<String, String>) {
        let path = self.path_of(id);
        for (node_id, content) in prompts {
            let prompt_path = pipeline::canonical_prompt_path(&path, node_id);
            if let Some(parent) = prompt_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = self.write_marked(&prompt_path, content) {
                warn!("failed to write prompt for {node_id}: {e}");
            }
        }
    }

    /// A save carries the prompts of the nodes the editor still knows about and
    /// says nothing about the ones it dropped, so without this the `.md` of a
    /// deleted node survives forever: invisible in the UI, and enough to make
    /// the pipeline's portable document un-importable. The saved YAML is the
    /// authority — not the keys of the request.
    fn prune_prompts(&self, id: &str, saved: &PipelineDef) {
        let path = self.path_of(id);
        let on_disk = pipeline_prompts(&path);
        let (_, orphans) = pipeline::split_live_prompts(saved, &on_disk);
        for node_id in orphans {
            let prompt_path = pipeline::canonical_prompt_path(&path, &node_id);
            self.self_writes.mark(&prompt_path);
            match std::fs::remove_file(&prompt_path) {
                Ok(()) => info!("Removed prompt of deleted node {node_id}"),
                Err(e) => warn!("failed to remove prompt for deleted node {node_id}: {e}"),
            }
        }
    }

    fn create(&self, id: &str, yaml: &str) -> Result<PathBuf, CreateFailure> {
        let Some(dir) = self.dir() else {
            return Err(CreateFailure::Io(NO_REGISTRY.to_string()));
        };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{id}.yaml"));
        // The client already knows about this file: it asked for it. Without the
        // mark, the watcher re-announced it as an outside change and an editor
        // opened on it within the debounce showed a conflict (#823).
        self.self_writes.mark(&path);
        let write_result = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, yaml.as_bytes()));
        match write_result {
            Ok(()) => Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(CreateFailure::Exists),
            Err(e) => Err(CreateFailure::Io(e.to_string())),
        }
    }

    fn relocate(&self, from: &str, to: &str, yaml: &str) -> Result<PathBuf, String> {
        let dir = self.dir().ok_or_else(|| NO_REGISTRY.to_string())?;
        let path = self.path_of(from);
        self.self_writes.mark(&path);
        let new_path = entries::move_pipeline_entry(&dir, &path, to, yaml)?;
        self.self_writes.mark(&new_path);
        Ok(new_path)
    }

    fn publish(
        &self,
        id: &str,
        pipeline: &PipelineDef,
        prompts: &HashMap<String, String>,
    ) -> Result<PathBuf, String> {
        let dir = self.dir().ok_or_else(|| NO_REGISTRY.to_string())?;
        entries::write_pipeline_atomically(&dir, id, pipeline, prompts)
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        let path = self.path_of(id);
        let prompts_dir = path.with_extension("prompts");
        if prompts_dir.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&prompts_dir) {
                warn!("failed to remove prompts dir for {id}: {e}");
            }
        }
        std::fs::remove_file(&path).map_err(|e| format!("delete failed: {e}"))
    }
}

/// Stage 3 (ADR-0080): the registry against a real directory. This crate's
/// test build keeps the registry at `<repo>/.pdo/pipelines`.
#[cfg(test)]
mod tests {
    use super::*;

    const PIPELINE: &str = "name: review\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: agent\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n";

    fn registry() -> (tempfile::TempDir, FsPipelineRegistry, SelfWrites) {
        let repo = tempfile::tempdir().unwrap();
        let writes = SelfWrites::new();
        let registry = FsPipelineRegistry::new(repo.path(), writes.clone());
        (repo, registry, writes)
    }

    fn dir(repo: &tempfile::TempDir) -> PathBuf {
        repo.path().join(".pdo/pipelines")
    }

    fn parsed(yaml: &str) -> PipelineDef {
        pipeline::parse_pipeline(yaml).unwrap().pipeline
    }

    #[test]
    fn the_files_are_the_yaml_files_only_and_carry_their_content() {
        let (repo, registry, _) = registry();
        std::fs::create_dir_all(dir(&repo)).unwrap();
        std::fs::write(dir(&repo).join("readme.md"), "not a pipeline").unwrap();
        std::fs::write(dir(&repo).join("notes.txt"), "not a pipeline").unwrap();
        std::fs::write(dir(&repo).join("real-pipe.yaml"), PIPELINE).unwrap();

        let files = registry.files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "real-pipe");
        assert_eq!(files[0].yaml.as_deref(), Some(PIPELINE));
        assert!(files[0].modified.is_some());
    }

    /// #823 — the file a client just asked us to create is not an outside
    /// change: without the mark, the watcher re-announced it and an editor
    /// opened on it within the debounce showed a conflict.
    #[test]
    fn create_marks_its_own_write_and_refuses_an_existing_file() {
        let (repo, registry, writes) = registry();
        let path = registry.create("fresh", PIPELINE).unwrap();
        assert_eq!(path, dir(&repo).join("fresh.yaml"));
        assert!(writes.is_recent(&path));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PIPELINE);
        assert_eq!(
            registry.create("fresh", "name: other\n"),
            Err(CreateFailure::Exists)
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PIPELINE);
    }

    #[test]
    fn prompts_live_under_the_canonical_sidecar_and_orphans_are_pruned() {
        let (repo, registry, writes) = registry();
        registry.create("review", PIPELINE).unwrap();
        let prompts = HashMap::from([
            ("worker".to_string(), "do the work".to_string()),
            ("gone".to_string(), "a node the YAML dropped".to_string()),
        ]);
        registry.write_prompts("review", &prompts);
        let worker = dir(&repo).join("review.prompts/worker.md");
        assert_eq!(std::fs::read_to_string(&worker).unwrap(), "do the work");
        assert!(writes.is_recent(&worker));
        assert_eq!(registry.prompts("review").len(), 2);

        registry.prune_prompts("review", &parsed(PIPELINE));
        assert_eq!(
            registry.prompts("review"),
            HashMap::from([("worker".to_string(), "do the work".to_string())])
        );
        assert!(!dir(&repo).join("review.prompts/gone.md").exists());
    }

    #[test]
    fn relocate_moves_the_file_and_its_prompts_and_marks_both_ends() {
        let (repo, registry, writes) = registry();
        registry.create("old-name", PIPELINE).unwrap();
        registry.write_prompts(
            "old-name",
            &HashMap::from([("worker".to_string(), "kept".to_string())]),
        );
        let moved = registry
            .relocate("old-name", "new-name", "name: New name\n")
            .unwrap();
        assert_eq!(moved, dir(&repo).join("new-name.yaml"));
        assert!(!dir(&repo).join("old-name.yaml").exists());
        assert!(!dir(&repo).join("old-name.prompts").exists());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "name: New name\n");
        assert_eq!(registry.prompts("new-name")["worker"], "kept");
        assert!(writes.is_recent(&dir(&repo).join("old-name.yaml")));
        assert!(writes.is_recent(&moved));
    }

    #[test]
    fn publish_writes_the_live_prompts_and_refuses_a_taken_id() {
        let (repo, registry, writes) = registry();
        let prompts = HashMap::from([
            ("worker".to_string(), "hi".to_string()),
            ("orphan".to_string(), "never seeded".to_string()),
        ]);
        let path = registry
            .publish("copy", &parsed(PIPELINE), &prompts)
            .unwrap();
        assert_eq!(path, dir(&repo).join("copy.yaml"));
        assert_eq!(
            parsed(&std::fs::read_to_string(&path).unwrap()).name,
            "review"
        );
        assert_eq!(
            registry.prompts("copy"),
            HashMap::from([("worker".to_string(), "hi".to_string())])
        );
        // A publish is not the daemon editing an open document: the watcher
        // announces the new entry.
        assert!(!writes.is_recent(&path));
        assert!(registry
            .publish("copy", &parsed(PIPELINE), &prompts)
            .is_err());
    }

    #[test]
    fn remove_deletes_the_file_and_its_prompts() {
        let (repo, registry, _) = registry();
        registry.create("doomed", PIPELINE).unwrap();
        registry.write_prompts(
            "doomed",
            &HashMap::from([("worker".to_string(), "x".to_string())]),
        );
        registry.remove("doomed").unwrap();
        assert!(!registry.exists("doomed"));
        assert!(!dir(&repo).join("doomed.prompts").exists());
        assert!(registry
            .remove("doomed")
            .unwrap_err()
            .starts_with("delete failed:"));
    }

    #[test]
    fn unification_folds_the_retired_registries_once_and_keeps_collisions() {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let target = repo.path().join("home-registry/.pdo/pipelines");
        let repo_registry = repo.path().join(".pdo/pipelines");
        std::fs::create_dir_all(&repo_registry).unwrap();
        std::fs::write(repo_registry.join("review.yaml"), "name: Repo review\n").unwrap();
        std::fs::write(repo_registry.join("same.yaml"), "name: Same\n").unwrap();
        std::fs::create_dir(repo_registry.join("review.prompts")).unwrap();
        std::fs::write(
            repo_registry.join("review.prompts/worker.md"),
            "repo prompt",
        )
        .unwrap();
        let legacy = home.path().join(".pdo/library/pipelines");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("review.yaml"), "name: Library review\n").unwrap();
        std::fs::create_dir(legacy.join("review.prompts")).unwrap();
        std::fs::write(legacy.join("review.prompts/worker.md"), "library prompt").unwrap();
        // An identical legacy copy is the same pipeline: not copied twice.
        std::fs::write(legacy.join("same.yaml"), "name: Same\n").unwrap();

        unify::unify_into(repo.path(), &target, Some(home.path())).unwrap();

        let read = |name: &str| std::fs::read_to_string(target.join(name)).unwrap();
        assert_eq!(read("review.yaml"), "name: Repo review\n");
        assert_eq!(read("review.prompts/worker.md"), "repo prompt");
        assert_eq!(read("review-2.yaml"), "name: Library review\n");
        assert_eq!(read("review-2.prompts/worker.md"), "library prompt");
        assert_eq!(read("same.yaml"), "name: Same\n");
        assert!(!target.join("same-2.yaml").exists());

        // Once per repository: a legacy file that appears later stays out.
        std::fs::write(legacy.join("late.yaml"), "name: Late\n").unwrap();
        unify::unify_into(repo.path(), &target, Some(home.path())).unwrap();
        assert!(!target.join("late.yaml").exists());
    }
}
