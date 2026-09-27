//! The sandbox Dockerfiles on the local filesystem: the one implementation of
//! [`DockerfileStore`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::application::ports::DockerfileStore;
use crate::domain::image::{build_context_dir, default_dockerfile_path};

/// Dockerfile embarqué (source de vérité dans le binaire). Seedé sur disque au 1er usage.
/// `.gitattributes` épingle `eol=lf` sur ce fichier : `include_str!` embarque les octets
/// verbatim, donc un checkout CRLF changerait le hash (cf. D6 du plan #405).
pub const EMBEDDED_DOCKERFILE: &str = include_str!("../../assets/sandbox/Dockerfile");

/// The Dockerfiles under `<sandbox_root>` and wherever a profile or the env points.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsDockerfiles;

/// Écrit le Dockerfile `embedded` à son chemin **par défaut** si absent ; sinon **ne touche
/// à rien** (édition utilisateur préservée : une édition change le hash donc rebuild). Renvoie
/// le chemin dans les deux cas.
///
/// #431 : n'écrit **JAMAIS** ailleurs qu'à [`default_dockerfile_path`], même quand un tier
/// `profile`/`env` a désigné un autre chemin — écrire `EMBEDDED` dans un emplacement du repo
/// que l'utilisateur a simplement *pointé* serait une mutation non demandée de son repo.
pub fn seed_dockerfile(sandbox_root: &Path, embedded: &str) -> Result<PathBuf> {
    let path = default_dockerfile_path(sandbox_root);
    if path.exists() {
        return Ok(path); // édition utilisateur gagne
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to seed sandbox Dockerfile at {}", path.display()))?;
    }
    std::fs::write(&path, embedded.as_bytes())
        .with_context(|| format!("failed to seed sandbox Dockerfile at {}", path.display()))?;
    Ok(path)
}

impl DockerfileStore for FsDockerfiles {
    fn seed_default(&self, sandbox_root: &Path) -> Result<PathBuf> {
        seed_dockerfile(sandbox_root, EMBEDDED_DOCKERFILE)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn ensure_build_context(&self, sandbox_root: &Path) -> std::io::Result<PathBuf> {
        let dir = build_context_dir(sandbox_root);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// Stage 3 (ADR-0080): the adapter against a real temporary directory.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_the_embedded_dockerfile_once_and_keeps_a_user_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sandbox");
        let path = FsDockerfiles.seed_default(&root).unwrap();
        assert_eq!(path, default_dockerfile_path(&root));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EMBEDDED_DOCKERFILE);

        std::fs::write(&path, "FROM scratch\n").unwrap();
        FsDockerfiles.seed_default(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "FROM scratch\n");
    }

    #[test]
    fn a_directory_or_a_dangling_link_is_not_a_dockerfile() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!FsDockerfiles.is_file(tmp.path()));
        let link = tmp.path().join("Dockerfile");
        std::os::unix::fs::symlink(tmp.path().join("gone"), &link).unwrap();
        assert!(!FsDockerfiles.is_file(&link));
        std::fs::write(tmp.path().join("gone"), "FROM scratch\n").unwrap();
        assert!(FsDockerfiles.is_file(&link));
        assert_eq!(FsDockerfiles.read(&link).unwrap(), b"FROM scratch\n");
    }

    #[test]
    fn the_build_context_is_created_empty_beside_the_seed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = FsDockerfiles.ensure_build_context(tmp.path()).unwrap();
        assert_eq!(dir, build_context_dir(tmp.path()));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}
