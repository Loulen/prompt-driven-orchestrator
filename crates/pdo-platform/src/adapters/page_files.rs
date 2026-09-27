//! The files behind the mounts, read from the local filesystem.
//!
//! Every component is resolved with `openat` beneath the already-open parent,
//! with `O_NOFOLLOW`: a symlink planted in a mounted directory never leads out
//! of it.

use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use tokio::io::AsyncReadExt;

use crate::application::ports::{BoxFuture, PageFile, PageFiles, StorageError};
use crate::domain::page_mount::{MountError, INDEX_FILE};

pub struct LocalPageFiles;

impl PageFiles for LocalPageFiles {
    fn canonical_directory(&self, directory: &Path) -> Result<PathBuf, MountError> {
        let canonical = directory.canonicalize().map_err(|error| {
            MountError::Invalid(format!("cannot mount `{}`: {error}", directory.display()))
        })?;
        if !canonical.is_dir() {
            return Err(MountError::Invalid(format!(
                "cannot mount `{}`: path is not a directory",
                canonical.display()
            )));
        }
        Ok(canonical)
    }

    fn read<'a>(
        &'a self,
        root: &'a Path,
        components: &'a [OsString],
    ) -> BoxFuture<'a, Result<Option<PageFile>, StorageError>> {
        Box::pin(async move {
            let Some((file, mime_path)) = open_file(root, components) else {
                return Ok(None);
            };
            let mut file = tokio::fs::File::from_std(file);
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)
                .await
                .map_err(|e| StorageError(e.to_string()))?;
            Ok(Some(PageFile {
                contents,
                mime_path,
            }))
        })
    }
}

/// The file `components` names beneath `root`, and the name its type is guessed
/// from. A directory serves its `index.html`.
fn open_file(root: &Path, components: &[OsString]) -> Option<(File, PathBuf)> {
    let mut directory = open_directory_path(root)?;
    let Some((last, parents)) = components.split_last() else {
        return open_index(&directory);
    };
    for component in parents {
        directory = open_at(&directory, component, true)?;
    }
    let file = open_at(&directory, last, false)?;
    let metadata = file.metadata().ok()?;
    if metadata.is_dir() {
        return open_index(&file);
    }
    if !metadata.is_file() {
        return None;
    }
    Some((file, PathBuf::from(last)))
}

fn open_directory_path(path: &Path) -> Option<File> {
    if !path.is_absolute() {
        return None;
    }
    let mut directory = File::open("/").ok()?;
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => directory = open_at(&directory, name, true)?,
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(directory)
}

fn open_index(directory: &File) -> Option<(File, PathBuf)> {
    let file = open_at(directory, OsStr::new(INDEX_FILE), false)?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    Some((file, PathBuf::from(INDEX_FILE)))
}

fn open_at(directory: &File, name: &OsStr, require_directory: bool) -> Option<File> {
    let name = CString::new(name.as_bytes()).ok()?;
    let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    if require_directory {
        flags |= libc::O_DIRECTORY;
    }
    // The returned descriptor is owned on success and immediately wrapped in
    // `File`; every path component is resolved beneath the already-open parent.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return None;
    }
    // SAFETY: `openat` returned a fresh owned descriptor and no other owner exists.
    Some(unsafe { File::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::page_mount::requested_components;

    async fn read(root: &Path, requested: &str) -> Option<PageFile> {
        let components = requested_components(requested)?;
        LocalPageFiles.read(root, &components).await.unwrap()
    }

    #[tokio::test]
    async fn requested_files_must_remain_inside_the_mount() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("inside.html"), "inside").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/index.html"), "sub index").unwrap();

        let inside = read(&root, "inside.html").await.unwrap();
        assert_eq!(inside.contents, b"inside");
        assert_eq!(inside.mime_path, PathBuf::from("inside.html"));
        let sub = read(&root, "sub").await.unwrap();
        assert_eq!(sub.contents, b"sub index");
        assert_eq!(sub.mime_path, PathBuf::from("index.html"));
        assert!(read(&root, "../../etc/passwd").await.is_none());
        assert!(read(&root, "missing.html").await.is_none());
        assert!(read(&root, "").await.is_none(), "no index at the root");
    }

    #[tokio::test]
    async fn a_symlink_never_leads_out_of_the_mount() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("link.txt"))
            .unwrap();
        assert!(read(&root, "link.txt").await.is_none());
    }

    #[test]
    fn only_existing_directories_are_mountable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            LocalPageFiles.canonical_directory(dir.path()).unwrap(),
            dir.path().canonicalize().unwrap()
        );
        assert!(LocalPageFiles.canonical_directory(&file).is_err());
        assert!(LocalPageFiles
            .canonical_directory(&dir.path().join("missing"))
            .is_err());
    }
}
