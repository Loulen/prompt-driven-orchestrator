//! The release channel (#24): the name this binary goes by, the version it
//! compares with the published releases, where it looks for them, and how an
//! Update replaces it.
//!
//! The vanilla `pdo` detects how it was installed (Homebrew, cargo-dist script).
//! A third-party distribution (ADR-0081) knows at compile time and declares it:
//! the detection is then never consulted, so a distribution binary can never run
//! the vanilla upgrade command and silently lose its extensions.

use std::sync::Arc;

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

/// Who supplies the identifier a channel presents to its release source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialOrigin {
    /// The operator's environment (`PDO_GITHUB_TOKEN`), when the binary
    /// declares no provider: the daemon and the update command already share it.
    Operator,
    /// A provider the binary declares: its identifier is handed to the update
    /// command only, never left in the environment of what the update restarts.
    Binary,
}

/// The identifier a channel presents, as a `Bearer`, to its release source
/// (`latest` and the release notes), and hands to the update command under
/// `PDO_GITHUB_TOKEN`. Asked again at every check and every Update: the answer
/// may change during the daemon's life. A blank answer counts as none.
///
/// It is never printed: [`std::fmt::Debug`] shows its origin only.
#[derive(Clone)]
pub struct ReleaseCredential {
    provide: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    origin: CredentialOrigin,
}

impl ReleaseCredential {
    /// The provider a binary declares.
    pub fn declared(provide: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            provide: Arc::new(provide),
            origin: CredentialOrigin::Binary,
        }
    }

    /// The operator's identifier, read by `provide` (the environment, in the
    /// daemon) when the binary declares no provider.
    pub fn operator(provide: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            provide: Arc::new(provide),
            origin: CredentialOrigin::Operator,
        }
    }

    /// The identifier now; `None` when there is none (or only blanks).
    pub fn current(&self) -> Option<String> {
        (self.provide)().filter(|t| !t.trim().is_empty())
    }

    pub fn origin(&self) -> CredentialOrigin {
        self.origin
    }
}

impl std::fmt::Debug for ReleaseCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReleaseCredential({:?})", self.origin)
    }
}

/// Two credentials are the same when they share their provider.
impl PartialEq for ReleaseCredential {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.provide, &other.provide) && self.origin == other.origin
    }
}

impl Eq for ReleaseCredential {}

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
    /// The identifier presented to the source and handed to the update
    /// command; `None`: none.
    pub credential: Option<ReleaseCredential>,
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
            credential: None,
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
            credential: None,
        }
    }

    /// The identifier to present now, if any.
    pub fn release_token(&self) -> Option<String> {
        self.credential
            .as_ref()
            .and_then(ReleaseCredential::current)
    }

    /// Whether the identifier is the binary's own, so the update command alone
    /// receives it.
    pub fn credential_is_declared(&self) -> bool {
        self.credential
            .as_ref()
            .is_some_and(|c| c.origin() == CredentialOrigin::Binary)
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

    #[test]
    fn the_credential_is_asked_at_every_read_and_a_blank_one_is_none() {
        let value = Arc::new(std::sync::Mutex::new(Some("first".to_string())));
        let read = value.clone();
        let mut c = UpdateChannel::declared("acme", "0.3.0", None, None);
        assert_eq!(c.release_token(), None);
        assert!(!c.credential_is_declared());
        c.credential = Some(ReleaseCredential::declared(move || {
            read.lock().unwrap().clone()
        }));
        assert_eq!(c.release_token().as_deref(), Some("first"));
        assert!(c.credential_is_declared());
        *value.lock().unwrap() = Some("second".into());
        assert_eq!(c.release_token().as_deref(), Some("second"));
        *value.lock().unwrap() = Some("  ".into());
        assert_eq!(c.release_token(), None);
        *value.lock().unwrap() = None;
        assert_eq!(c.release_token(), None);
    }

    #[test]
    fn an_operator_credential_is_not_the_binarys_and_none_is_ever_printed() {
        let c = ReleaseCredential::operator(|| Some("s3cr3t".into()));
        assert_eq!(c.origin(), CredentialOrigin::Operator);
        let mut channel = UpdateChannel::vanilla();
        channel.credential = Some(c.clone());
        assert!(!channel.credential_is_declared());
        assert!(!format!("{channel:?}").contains("s3cr3t"));
        assert_eq!(c, c.clone());
        assert_ne!(c, ReleaseCredential::operator(|| Some("s3cr3t".into())));
    }
}
