//! How this binary was installed and is supervised, detected from the host — or
//! forced by the per-daemon overrides (the test seams, the FP container).

use std::path::PathBuf;

use super::env::resolve_config_home;
use crate::application::ports::Installation;
use crate::domain::install::{
    detect_install_method, detect_supervision, receipt_paths, stable_exe_path, InstallMethod,
    Supervision,
};

#[derive(Debug, Clone, Default)]
pub struct HostInstallation {
    method_override: Option<InstallMethod>,
    supervision_override: Option<Supervision>,
    config_home_override: Option<PathBuf>,
}

impl HostInstallation {
    pub fn new(
        method_override: Option<InstallMethod>,
        supervision_override: Option<Supervision>,
    ) -> Self {
        Self {
            method_override,
            supervision_override,
            config_home_override: None,
        }
    }

    /// Look for the cargo-dist receipt under `config_home` instead of
    /// `$XDG_CONFIG_HOME` / `~/.config`: a daemon whose home is overridden (the
    /// test seam) never reads the real one.
    pub fn with_config_home(mut self, config_home: Option<PathBuf>) -> Self {
        self.config_home_override = config_home;
        self
    }
}

impl Installation for HostInstallation {
    fn method(&self) -> InstallMethod {
        self.method_override.unwrap_or_else(|| {
            detect_install_method_from_env(
                self.config_home_override
                    .clone()
                    .or_else(resolve_config_home),
            )
        })
    }

    fn supervision(&self) -> Supervision {
        self.supervision_override.unwrap_or_else(|| {
            detect_supervision(
                std::env::var("INVOCATION_ID").ok().as_deref(),
                std::env::var("XPC_SERVICE_NAME").ok().as_deref(),
            )
        })
    }

    fn invoked_exe(&self) -> Option<PathBuf> {
        invoked_exe_path()
    }
}

/// Read the environment once and feed the pure detector.
fn detect_install_method_from_env(config_home: Option<PathBuf>) -> InstallMethod {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .unwrap_or_default();
    let receipt = config_home
        .map(|h| receipt_paths(&h).iter().any(|p| p.is_file()))
        .unwrap_or(false);
    let prefix = std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from);
    detect_install_method(&exe, receipt, prefix.as_deref())
}

/// `argv[0]`, `$PATH` and the cwd from the process, files probed with `is_file`
/// (which follows symlinks to answer "is there a binary here", while the
/// RETURNED path stays the unresolved one).
pub fn stable_exe_path_from_env() -> Option<PathBuf> {
    let argv0 = std::env::args_os().next()?.to_str()?.to_string();
    let path_env = std::env::var("PATH").ok();
    let cwd = std::env::current_dir().ok()?;
    stable_exe_path(&argv0, path_env.as_deref(), &cwd, &|p| p.is_file())
}

/// The binary a detached executor should call and a unit should point at: the
/// stable (unresolved) path when it can be derived, else the resolved
/// `current_exe`. Never `None` for a running process short of a broken `/proc`.
pub fn invoked_exe_path() -> Option<PathBuf> {
    stable_exe_path_from_env().or_else(|| std::env::current_exe().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_win_over_the_detection() {
        let forced =
            HostInstallation::new(Some(InstallMethod::Homebrew), Some(Supervision::Launchd));
        assert_eq!(forced.method(), InstallMethod::Homebrew);
        assert_eq!(forced.supervision(), Supervision::Launchd);
    }

    #[test]
    fn the_receipt_is_looked_for_under_the_overridden_config_home() {
        let config = tempfile::tempdir().unwrap();
        let install =
            HostInstallation::default().with_config_home(Some(config.path().to_path_buf()));
        let without = install.method();
        assert_ne!(without, InstallMethod::Script, "no receipt planted yet");
        let receipt = &receipt_paths(config.path())[0];
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        std::fs::write(receipt, "{}").unwrap();
        // A Homebrew path beats the receipt; anywhere else, the receipt says script.
        if without == InstallMethod::Unknown {
            assert_eq!(install.method(), InstallMethod::Script);
        }
    }

    #[test]
    fn the_test_binary_is_invoked_from_somewhere() {
        assert!(HostInstallation::default().invoked_exe().is_some());
    }
}
