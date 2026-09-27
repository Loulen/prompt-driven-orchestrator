//! Copying a skill folder into a snapshot or a worktree, symlinks kept.

use std::path::Path;

use anyhow::{Context, Result};

/// Recursive copy that reproduces symlinks as symlinks (never dereferenced) and
/// refuses nothing else: a skill folder is plain files.
pub(crate) fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for entry in std::fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&from)?;
            symlink_any(&target, &to)?;
        } else if meta.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn symlink_dir(target: impl AsRef<Path>, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
pub(crate) fn symlink_dir(target: impl AsRef<Path>, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn symlink_any(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_any(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}
