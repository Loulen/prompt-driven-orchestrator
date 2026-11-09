//! The release channel (#24): the name this binary goes by, the version it
//! compares with the published releases, where it looks for them, and how an
//! Update replaces it.
//!
//! The vanilla `pdo` detects how it was installed (Homebrew, cargo-dist script).
//! A third-party distribution (ADR-0081) knows at compile time and declares it:
//! the detection is then never consulted, so a distribution binary can never run
//! the vanilla upgrade command and silently lose its extensions.

use super::install::InstallMethod;

/// The name of the vanilla binary. A channel named otherwise is a distribution:
/// the UI then shows its name and version next to the core's.
pub const VANILLA_CHANNEL_NAME: &str = "PDO";

/// The version of the core this binary is built on.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the vanilla binary looks for releases: the latest release of the
/// public repository, `Loulen/prompt-driven-orchestrator`. An operator can
/// still point the check elsewhere with `PDO_UPDATE_SOURCE_URL`.
pub const VANILLA_SOURCE_URL: Option<&str> =
    Some("https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest");

/// How an Update replaces the binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgrade {
    /// The vanilla binary: the install method is detected, and its command runs.
    Detected,
    /// Declared by a distribution: this command runs, verbatim. `None`: the
    /// distribution declares no command, and there is no Update.
    Declared(Option<String>),
}

/// The release channel of this binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateChannel {
    /// Shown in the UI (`PDO`, `acme-pdo`…).
    pub name: String,
    /// The version compared with the releases of `source_url`.
    pub installed_version: String,
    /// A GitHub-style `releases/latest` endpoint; `None`: no version check.
    pub source_url: Option<String>,
    pub upgrade: Upgrade,
}

impl UpdateChannel {
    /// The vanilla `pdo`: its name, the core's version, the vanilla source and
    /// the detected install method.
    pub fn vanilla() -> Self {
        Self {
            name: VANILLA_CHANNEL_NAME.to_string(),
            installed_version: CORE_VERSION.to_string(),
            source_url: VANILLA_SOURCE_URL.map(str::to_string),
            upgrade: Upgrade::Detected,
        }
    }

    /// A distribution's channel. A blank source or command counts as absent: an
    /// empty string is never fetched nor run.
    pub fn declared(
        name: impl Into<String>,
        installed_version: impl Into<String>,
        source_url: Option<String>,
        upgrade_command: Option<String>,
    ) -> Self {
        let present = |s: String| (!s.trim().is_empty()).then_some(s);
        Self {
            name: name.into(),
            installed_version: installed_version.into(),
            source_url: source_url.and_then(present),
            upgrade: Upgrade::Declared(upgrade_command.and_then(present)),
        }
    }

    /// The install method an Update delegates to and the command it runs. The
    /// detection (`detect`) is only consulted for the vanilla binary. The
    /// vanilla `unknown` method still has a command: the text telling the user
    /// what to do, which the apply gate never runs.
    pub fn upgrader(
        &self,
        detect: impl FnOnce() -> InstallMethod,
    ) -> (InstallMethod, Option<String>) {
        match &self.upgrade {
            Upgrade::Detected => {
                let method = detect();
                (method, method.manual_command().map(str::to_string))
            }
            Upgrade::Declared(command) => (InstallMethod::Declared, command.clone()),
        }
    }
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::vanilla()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_is_the_core_checking_the_public_repository_with_the_detected_method() {
        let v = UpdateChannel::vanilla();
        assert_eq!(v.name, "PDO");
        assert_eq!(v.installed_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            v.source_url.as_deref(),
            Some("https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest")
        );
        let (method, command) = v.upgrader(|| InstallMethod::Homebrew);
        assert_eq!(method, InstallMethod::Homebrew);
        assert_eq!(
            command.as_deref(),
            Some("brew update && brew upgrade Loulen/tap/pdo")
        );
        let (method, command) = v.upgrader(|| InstallMethod::Script);
        assert_eq!(method, InstallMethod::Script);
        assert!(command.unwrap().contains(
            "https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh"
        ));
    }

    #[test]
    fn a_declared_channel_never_asks_the_detection() {
        let c = UpdateChannel::declared("acme", "0.3.0", None, Some("acme-upgrade".into()));
        let (method, command) = c.upgrader(|| panic!("the detection must not run"));
        assert_eq!(method, InstallMethod::Declared);
        assert_eq!(command.as_deref(), Some("acme-upgrade"));
        let none = UpdateChannel::declared("acme", "0.3.0", None, None);
        assert_eq!(none.upgrader(|| InstallMethod::Script).1, None);
    }

    #[test]
    fn a_blank_source_or_command_is_absent() {
        let c = UpdateChannel::declared("acme", "0.3.0", Some("  ".into()), Some(String::new()));
        assert_eq!(c.source_url, None);
        assert_eq!(c.upgrade, Upgrade::Declared(None));
    }
}
