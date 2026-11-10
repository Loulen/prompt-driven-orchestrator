//! How this binary was installed, who supervises the daemon, and the stable path
//! of the binary — pure detections over what the environment says (#697, #699).
//!
//! "Unknown" is declared, never guessed (ADR-0045): the Update button is absent
//! with the reason shown, never falls back to a guess.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The `pdo` formula of the tap `Loulen/homebrew-tap`, which cargo-dist publishes
/// from the public repository. The manual command names it in full so `brew`
/// never resolves another tap's `pdo`.
pub const HOMEBREW_COMMAND: &str = "brew update && brew upgrade Loulen/tap/pdo";
/// Re-running the cargo-dist installer of the public repository's latest release
/// replaces `~/.local/bin/pdo` the supported way.
pub const SCRIPT_COMMAND: &str = "curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh | sh";
/// No installer to delegate to: the user rebuilds, PDO does not update itself.
pub const UNKNOWN_COMMAND: &str = "Build from source, then restart the daemon.";

/// How this binary got here — what an Update delegates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum InstallMethod {
    Homebrew,
    Script,
    Unknown,
    /// Declared by a distribution's release channel (#24): never detected, and
    /// its command is the channel's, not one of the constants above.
    Declared,
}

impl InstallMethod {
    /// The exact command the user would type (and the Update button runs) for a
    /// detected method. `None` for [`InstallMethod::Declared`]: the release
    /// channel carries that command.
    pub fn manual_command(self) -> Option<&'static str> {
        match self {
            InstallMethod::Homebrew => Some(HOMEBREW_COMMAND),
            InstallMethod::Script => Some(SCRIPT_COMMAND),
            InstallMethod::Unknown => Some(UNKNOWN_COMMAND),
            InstallMethod::Declared => None,
        }
    }

    /// The method a `PDO_INSTALL_METHOD` value forces, if it names one. Never
    /// `declared`: only a distribution's channel declares its method.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "homebrew" => Some(InstallMethod::Homebrew),
            "script" => Some(InstallMethod::Script),
            "unknown" => Some(InstallMethod::Unknown),
            _ => None,
        }
    }

    /// The lowercase wire name (`homebrew` / `script` / `unknown` / `declared`).
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMethod::Homebrew => "homebrew",
            InstallMethod::Script => "script",
            InstallMethod::Unknown => "unknown",
            InstallMethod::Declared => "declared",
        }
    }
}

/// Pure detection over (resolved binary path, cargo-dist receipt presence, Homebrew
/// prefix). Order matters: the Cellar is the strongest evidence (the path IS the
/// formula's), the receipt only says the script ran once; `unknown` otherwise.
pub fn detect_install_method(
    exe_path: &Path,
    receipt_exists: bool,
    brew_prefix: Option<&Path>,
) -> InstallMethod {
    let components: Vec<&str> = exe_path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let under_cellar = components.contains(&"Cellar");
    let under_prefix = brew_prefix
        .filter(|p| !p.as_os_str().is_empty())
        .is_some_and(|p| exe_path.starts_with(p));
    if under_cellar || under_prefix {
        return InstallMethod::Homebrew;
    }
    if receipt_exists {
        return InstallMethod::Script;
    }
    InstallMethod::Unknown
}

/// Where cargo-dist writes its install receipt: `$XDG_CONFIG_HOME/<app>/<app>-receipt.json`
/// (falling back to `~/.config/<app>/`). The app is the Cargo **package** (`pdo-daemon`,
/// see `dist-workspace`'s note in `Cargo.toml`); the `pdo` spelling is kept as a second
/// candidate so a future package rename does not silently turn every script install
/// into `unknown`.
pub fn receipt_paths(config_home: &Path) -> [PathBuf; 2] {
    [
        config_home
            .join("pdo-daemon")
            .join("pdo-daemon-receipt.json"),
        config_home.join("pdo").join("pdo-receipt.json"),
    ]
}

/// Who restarts the daemon after an update — or nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum Supervision {
    Systemd,
    Launchd,
    None,
}

impl Supervision {
    /// The lowercase wire name (`systemd` / `launchd` / `none`).
    pub fn as_str(self) -> &'static str {
        match self {
            Supervision::Systemd => "systemd",
            Supervision::Launchd => "launchd",
            Supervision::None => "none",
        }
    }
}

/// Pure detection from the environment a supervisor sets on its children:
/// `INVOCATION_ID` (systemd), `XPC_SERVICE_NAME` (launchd, not the shell's
/// `0` placeholder). Empty values count as absent.
pub fn detect_supervision(
    invocation_id: Option<&str>,
    xpc_service_name: Option<&str>,
) -> Supervision {
    if invocation_id.is_some_and(|v| !v.trim().is_empty()) {
        return Supervision::Systemd;
    }
    if xpc_service_name.is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && v != "0"
    }) {
        return Supervision::Launchd;
    }
    Supervision::None
}

/// The path the user **invoked** — `argv[0]` made absolute — without following
/// symlinks. This is what a service unit must point at: Homebrew's `bin/pdo` is a
/// symlink into `Cellar/pdo/<version>/`, and `brew upgrade` deletes that target, so a
/// unit written from `/proc/self/exe` stops starting on the very first upgrade.
///
/// Pure over (`argv[0]`, `$PATH`, cwd):
/// * absolute `argv[0]` → as is;
/// * `argv[0]` with a slash (`./pdo`, `bin/pdo`) → joined to `cwd`, `.`/`..` folded
///   lexically (no filesystem resolution);
/// * bare name → the first `$PATH` dir holding a file of that name (`exists` is
///   evaluated by the caller through `dir_has_file`, so this stays testable), else
///   `None` — the caller falls back to the resolved path.
pub fn stable_exe_path(
    argv0: &str,
    path_env: Option<&str>,
    cwd: &Path,
    dir_has_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if argv0.is_empty() {
        return None;
    }
    let candidate = Path::new(argv0);
    if candidate.is_absolute() {
        return Some(normalize_lexically(candidate));
    }
    if argv0.contains('/') {
        return Some(normalize_lexically(&cwd.join(candidate)));
    }
    let path_env = path_env?;
    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let dir = if dir.is_absolute() {
            dir
        } else {
            cwd.join(dir)
        };
        let full = dir.join(argv0);
        if dir_has_file(&full) {
            return Some(normalize_lexically(&full));
        }
    }
    None
}

/// Fold `.` and `..` components without touching the filesystem — the whole point
/// is NOT to resolve symlinks.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cellar_path_is_homebrew() {
        let p = Path::new("/home/linuxbrew/.linuxbrew/Cellar/pdo/1.58.1/bin/pdo");
        assert_eq!(
            detect_install_method(p, false, None),
            InstallMethod::Homebrew
        );
        let mac = Path::new("/opt/homebrew/Cellar/pdo/1.58.1/bin/pdo");
        assert_eq!(
            detect_install_method(mac, true, None),
            InstallMethod::Homebrew
        );
    }

    #[test]
    fn brew_prefix_match_is_homebrew_even_without_cellar() {
        let p = Path::new("/opt/homebrew/bin/pdo");
        assert_eq!(
            detect_install_method(p, false, Some(Path::new("/opt/homebrew"))),
            InstallMethod::Homebrew
        );
        // An empty prefix must not match everything.
        assert_eq!(
            detect_install_method(Path::new("/usr/local/bin/pdo"), false, Some(Path::new(""))),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn receipt_present_is_script_otherwise_unknown() {
        let p = Path::new("/home/u/.local/bin/pdo");
        assert_eq!(detect_install_method(p, true, None), InstallMethod::Script);
        assert_eq!(
            detect_install_method(p, false, None),
            InstallMethod::Unknown
        );
        assert_eq!(
            detect_install_method(p, false, Some(Path::new("/opt/homebrew"))),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn manual_command_matches_the_method() {
        assert_eq!(
            InstallMethod::Homebrew.manual_command(),
            Some("brew update && brew upgrade Loulen/tap/pdo")
        );
        assert!(InstallMethod::Script.manual_command().unwrap().contains(
            "Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh"
        ));
        assert!(InstallMethod::Unknown
            .manual_command()
            .unwrap()
            .starts_with("Build from source"));
        // A distribution's channel carries its own command.
        assert_eq!(InstallMethod::Declared.manual_command(), None);
    }

    #[test]
    fn a_forced_method_is_parsed_by_its_wire_name() {
        assert_eq!(
            InstallMethod::parse(" homebrew "),
            Some(InstallMethod::Homebrew)
        );
        assert_eq!(InstallMethod::parse("script"), Some(InstallMethod::Script));
        assert_eq!(
            InstallMethod::parse("unknown"),
            Some(InstallMethod::Unknown)
        );
        assert_eq!(InstallMethod::parse("npm"), None);
        assert_eq!(InstallMethod::parse("declared"), None);
        assert_eq!(InstallMethod::Script.as_str(), "script");
        assert_eq!(Supervision::Launchd.as_str(), "launchd");
    }

    #[test]
    fn stable_exe_path_keeps_the_invoked_path_unresolved() {
        let cwd = Path::new("/home/u/work");
        let none = |_: &Path| false;
        // Absolute argv[0]: verbatim — the Homebrew symlink, not the Cellar target.
        assert_eq!(
            stable_exe_path("/home/linuxbrew/.linuxbrew/bin/pdo", None, cwd, &none),
            Some(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"))
        );
        // Relative with a slash: joined to cwd, `.`/`..` folded lexically.
        assert_eq!(
            stable_exe_path("./bin/../bin/pdo", None, cwd, &none),
            Some(PathBuf::from("/home/u/work/bin/pdo"))
        );
        // Bare name: the first PATH dir that has it wins; the dir is not canonicalised.
        let has = |p: &Path| p == Path::new("/opt/homebrew/bin/pdo");
        assert_eq!(
            stable_exe_path(
                "pdo",
                Some("/usr/local/bin:/opt/homebrew/bin:/home/u/.local/bin"),
                cwd,
                &has
            ),
            Some(PathBuf::from("/opt/homebrew/bin/pdo"))
        );
        // Bare name found nowhere (or no PATH): None → the caller falls back.
        assert_eq!(stable_exe_path("pdo", Some("/usr/bin"), cwd, &none), None);
        assert_eq!(stable_exe_path("pdo", None, cwd, &has), None);
        assert_eq!(
            stable_exe_path("", Some("/opt/homebrew/bin"), cwd, &has),
            None
        );
    }

    #[test]
    fn receipt_paths_name_the_cargo_package_first() {
        let paths = receipt_paths(Path::new("/home/u/.config"));
        assert_eq!(
            paths[0],
            Path::new("/home/u/.config/pdo-daemon/pdo-daemon-receipt.json")
        );
        assert_eq!(paths[1], Path::new("/home/u/.config/pdo/pdo-receipt.json"));
    }

    #[test]
    fn supervision_from_env() {
        assert_eq!(
            detect_supervision(Some("abc123"), None),
            Supervision::Systemd
        );
        assert_eq!(
            detect_supervision(None, Some("com.pdo.daemon")),
            Supervision::Launchd
        );
        // The shell's `XPC_SERVICE_NAME=0` placeholder is not launchd.
        assert_eq!(detect_supervision(None, Some("0")), Supervision::None);
        assert_eq!(detect_supervision(Some(""), Some("")), Supervision::None);
        assert_eq!(detect_supervision(None, None), Supervision::None);
    }
}
