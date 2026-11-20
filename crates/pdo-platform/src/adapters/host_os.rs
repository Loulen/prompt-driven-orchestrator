//! The system the daemon runs on, as a bug report states it (`GET /settings`
//! → `host_os`): `Linux 6.8.0-107-generic (x86_64)`. Read once: the kernel
//! does not change under a running daemon.

use std::sync::OnceLock;

/// The daemon's system: its name, its release when the host tells it, its
/// architecture.
pub fn host_os() -> String {
    static OS: OnceLock<String> = OnceLock::new();
    OS.get_or_init(|| {
        describe_os(
            std::env::consts::OS,
            release().as_deref(),
            std::env::consts::ARCH,
        )
    })
    .clone()
}

/// `linux`, `6.8.0`, `x86_64` → `Linux 6.8.0 (x86_64)`.
fn describe_os(os: &str, release: Option<&str>, arch: &str) -> String {
    let name = match os {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        "freebsd" => "FreeBSD",
        other => other,
    };
    match release.map(str::trim).filter(|r| !r.is_empty()) {
        Some(release) => format!("{name} {release} ({arch})"),
        None => format!("{name} ({arch})"),
    }
}

/// The kernel's release: `/proc` on Linux, `uname -r` elsewhere on Unix.
fn release() -> Option<String> {
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        return Some(release.trim().to_string());
    }
    if cfg!(unix) {
        let out = std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()?;
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_system_its_release_and_its_architecture() {
        assert_eq!(
            describe_os("linux", Some("6.8.0-107-generic\n"), "x86_64"),
            "Linux 6.8.0-107-generic (x86_64)"
        );
        assert_eq!(
            describe_os("macos", Some("24.1.0"), "aarch64"),
            "macOS 24.1.0 (aarch64)"
        );
        assert_eq!(describe_os("windows", None, "x86_64"), "Windows (x86_64)");
        assert_eq!(describe_os("haiku", Some(" "), "x86"), "haiku (x86)");
    }

    #[test]
    fn reads_the_host() {
        assert!(!host_os().is_empty());
    }
}
