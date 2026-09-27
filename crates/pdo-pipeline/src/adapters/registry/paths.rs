//! Where the instance registry lives.

use std::path::{Path, PathBuf};

/// `$HOME`, when set.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The instance registry: `~/.pdo/pipelines`. The daemon's integration test
/// binary (`it-*`, debug build) and this crate's own tests keep it inside
/// their temporary repository instead; everything else is identical.
pub fn instance_pipeline_dir(repo_root: &Path) -> Option<PathBuf> {
    let integration_test_binary = cfg!(debug_assertions)
        && std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(|name| name.to_owned()))
            .and_then(|name| name.to_str().map(str::to_owned))
            .is_some_and(|name| name.starts_with("it-"));
    if cfg!(test) || integration_test_binary {
        return Some(repo_root.join(".pdo").join("pipelines"));
    }
    home_dir().map(|home| home.join(".pdo").join("pipelines"))
}

/// The file of the pipeline `id`: in the instance registry, or under the
/// repository when the registry cannot be determined.
pub fn resolve_pipeline_path(repo_root: &Path, id: &str) -> PathBuf {
    instance_pipeline_dir(repo_root)
        .unwrap_or_else(|| repo_root.join(".pdo").join("pipelines"))
        .join(format!("{id}.yaml"))
}
