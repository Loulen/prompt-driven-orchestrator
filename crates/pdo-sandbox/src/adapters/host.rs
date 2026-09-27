//! What the sandbox reads from the host process: its home, its uid/gid, its own
//! binary. The only readers of `$HOME`, `getuid(2)` and `current_exe` in the
//! context; the tests inject their values instead.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::application::ports::HostHome;

/// The daemon's home: the per-daemon override (a test seam) when set, else `$HOME`,
/// read at each call.
#[derive(Debug, Clone, Default)]
pub struct EnvHome {
    override_home: Option<PathBuf>,
}

impl EnvHome {
    pub fn new(override_home: Option<PathBuf>) -> Self {
        Self { override_home }
    }
}

impl HostHome for EnvHome {
    fn home(&self) -> Option<PathBuf> {
        match &self.override_home {
            Some(home) => Some(home.clone()),
            None => std::env::var("HOME").ok().map(PathBuf::from),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// `(home_root, sandbox_root)` = `($HOME, $HOME/.pdo/sandbox)`. `None` si `HOME`
/// est absent. Câblé par le daemon (#407) ; les unit tests injectent des temp
/// dirs et bypassent ce résolveur (pas de swap `HOME` → pas de mutex global).
pub fn default_roots_from_env() -> Option<(PathBuf, PathBuf)> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    Some(roots_under(&home))
}

/// `(home, home/.pdo/sandbox)`: where a home keeps the per-Run stagings.
pub fn roots_under(home: &Path) -> (PathBuf, PathBuf) {
    (home.to_path_buf(), home.join(".pdo").join("sandbox"))
}

/// uid hôte via `getuid(2)` (syscall, pas d'env → pas de suffixe `_from_env`).
pub fn host_uid() -> u32 {
    // SAFETY: `getuid` a la signature `() -> uid_t`, ne prend aucun argument, ne déréférence
    // aucun pointeur et réussit toujours (POSIX : « The getuid() function shall always be
    // successful »). Aucune précondition à satisfaire.
    unsafe { libc::getuid() }
}

/// gid hôte via `getgid(2)`.
pub fn host_gid() -> u32 {
    // SAFETY: idem [`host_uid`] — `getgid` ne prend aucun argument et réussit toujours.
    unsafe { libc::getgid() }
}

/// Chemin du binaire `pdo` courant à bind-monter dans le conteneur (`/usr/local/bin/pdo:ro`).
pub fn pdo_bin_path() -> Result<PathBuf> {
    std::env::current_exe()
        .context("failed to determine the pdo binary path to bind-mount into the sandbox container")
}

/// Stage 3 (ADR-0080): the adapter against the real filesystem and process.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_wins_over_home() {
        let home = EnvHome::new(Some(PathBuf::from("/tmp/override-home")));
        assert_eq!(home.home(), Some(PathBuf::from("/tmp/override-home")));
    }

    #[test]
    fn a_dangling_symlink_exists_but_is_not_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(tmp.path().join("gone"), &link).unwrap();
        let home = EnvHome::default();
        assert!(home.exists(&link));
        assert!(!home.is_file(&link));
        assert!(!home.exists(&tmp.path().join("nothing")));
    }

    #[test]
    fn the_stagings_live_under_the_home() {
        assert_eq!(
            roots_under(Path::new("/home/u")),
            (
                PathBuf::from("/home/u"),
                PathBuf::from("/home/u/.pdo/sandbox")
            )
        );
    }

    #[test]
    fn the_host_identity_is_this_process() {
        assert_eq!(host_uid(), unsafe { libc::getuid() });
        assert_eq!(host_gid(), unsafe { libc::getgid() });
        assert!(pdo_bin_path().unwrap().is_file());
    }
}
