//! The real host of `pdo service`: env lookups, a TCP probe, file writes, and
//! `std::process::Command`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use super::env::{resolve_config_home, resolve_node_dir};
use super::host_installation::stable_exe_path_from_env;
use crate::application::ports::{CmdOutcome, ServiceHost};
use crate::domain::service_unit::{connect_addr, PortState};

/// Production `ServiceHost`: real env lookups, real TCP probe, real
/// `std::process::Command`.
pub struct RealServiceHost;

impl ServiceHost for RealServiceHost {
    fn current_exe(&self) -> Result<PathBuf> {
        // #699: the unit points at the binary by the path the user INVOKED —
        // Homebrew's stable `bin/pdo` symlink, never the `Cellar/pdo/<version>/`
        // target it resolves to, which `brew upgrade` deletes (and with it the
        // service's ability to restart). Resolved path only as a fallback.
        if let Some(stable) = stable_exe_path_from_env() {
            return Ok(stable);
        }
        let exe = std::env::current_exe().context("failed to determine current executable path")?;
        Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
    }

    fn current_dir(&self) -> Result<PathBuf> {
        std::env::current_dir().context("failed to determine current directory")
    }

    fn node_dir(&self) -> Option<PathBuf> {
        resolve_node_dir()
    }

    fn config_home(&self) -> Result<PathBuf> {
        resolve_config_home().context(
            "neither $XDG_CONFIG_HOME nor $HOME is set: cannot locate the systemd user dir",
        )
    }

    fn home(&self) -> Result<PathBuf> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("$HOME is not set")
    }

    fn user(&self) -> Result<String> {
        std::env::var("USER").context("$USER is not set")
    }

    fn probe_port(&self, addr: SocketAddr) -> PortState {
        use std::net::TcpStream;
        if std::net::TcpListener::bind(addr).is_ok() {
            return PortState::Free;
        }
        let addr = connect_addr(addr);
        if TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err() {
            return PortState::Foreign;
        }
        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
        {
            Ok(c) => c,
            Err(_) => return PortState::Foreign,
        };
        match client.get(format!("http://{addr}/sessions")).send() {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().unwrap_or_default();
                // The `/sessions` payload carries `cap` + `version` — markers no
                // random service would emit together.
                if body.contains("\"cap\"") && body.contains("\"version\"") {
                    PortState::PdoDaemon
                } else {
                    PortState::Foreign
                }
            }
            _ => PortState::Foreign,
        }
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("failed to remove {}", path.display())),
        }
    }

    fn run_cmd(&self, program: &str, args: &[&str]) -> Result<CmdOutcome> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to spawn `{program}`"))?;
        Ok(CmdOutcome {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_service_probe_detects_a_listener_on_another_local_address() {
        let listener = std::net::TcpListener::bind("127.0.1.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = RealServiceHost.probe_port(SocketAddr::from(([0, 0, 0, 0], port)));
        assert_eq!(state, PortState::Foreign);
    }

    #[test]
    fn a_free_port_is_free_and_files_come_and_go() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert_eq!(
            RealServiceHost.probe_port(SocketAddr::from(([127, 0, 0, 1], port))),
            PortState::Free
        );
        let dir = tempfile::tempdir().unwrap();
        let unit = dir.path().join("systemd/user/pdo.service");
        RealServiceHost.write_file(&unit, "unit").unwrap();
        assert_eq!(std::fs::read_to_string(&unit).unwrap(), "unit");
        RealServiceHost.remove_file(&unit).unwrap();
        RealServiceHost
            .remove_file(&unit)
            .expect("an absent file is removed already");
        let echo = RealServiceHost
            .run_cmd("sh", &["-c", "echo out; echo err >&2"])
            .unwrap();
        assert!(echo.success);
        assert_eq!(
            (echo.stdout.as_str(), echo.stderr.as_str()),
            ("out\n", "err\n")
        );
    }
}
