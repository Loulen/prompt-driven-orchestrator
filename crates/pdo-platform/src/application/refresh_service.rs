//! Bring the OS service unit in line with the binary in place (`pdo service
//! refresh`, ADR-0019): what the one-click Update runs once the new binary is
//! installed, and what an operator runs after updating by hand.
//!
//! Unlike `pdo service install`, it never probes the port (during an Update it
//! is held by the very daemon being updated), never enables the unit nor the
//! linger, never bootstraps a LaunchAgent, and never creates a unit: it
//! rewrites the one in place only when the rendered unit differs. That is how
//! a unit pointing into a versioned Cellar is repaired to the stable path
//! (#699), and how a new unit template reaches the service with the version
//! that brings it.

use std::io::Write;
use std::net::IpAddr;

use anyhow::Result;
use pdo_kernel::authorization::authorize;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::install_service::run_checked;
use super::ports::ServiceHost;
use crate::domain::service_unit::{
    build_path_env, launchd_plist_path, render_launchd_plist, render_systemd_unit,
    systemd_unit_path,
};
use crate::domain::unit_diff::unit_diff;

/// The service manager whose unit is refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    /// A `systemd --user` unit (Linux).
    Systemd,
    /// A launchd LaunchAgent plist (macOS).
    Launchd,
}

impl ServiceManager {
    /// The one this host runs: launchd on macOS, systemd elsewhere.
    pub fn of_this_host() -> Self {
        if cfg!(target_os = "macos") {
            ServiceManager::Launchd
        } else {
            ServiceManager::Systemd
        }
    }
}

pub struct RefreshServiceCommand {
    /// Address the service daemon listens on; `None` keeps the default.
    pub bind: Option<IpAddr>,
    pub port: u16,
    /// Show whether the unit would change, and the diff, writing nothing.
    pub dry_run: bool,
}

pub struct RefreshService<'a> {
    pub host: &'a dyn ServiceHost,
    pub extensions: &'a dyn ExtensionHost,
    pub manager: ServiceManager,
}

impl RefreshService<'_> {
    /// Refresh for `actor`, writing the trace to `out`: `unchanged`, or
    /// `rewrote <path>` (then `daemon-reload` under systemd). A dry run
    /// changes nothing, so it asks no authorization.
    pub fn execute(
        &self,
        actor: &Actor,
        command: RefreshServiceCommand,
        out: &mut dyn Write,
    ) -> Result<()> {
        let RefreshServiceCommand {
            bind,
            port,
            dry_run,
        } = command;
        if !dry_run {
            let operation = Operation::new(ResourceKind::Service, Verb::Update);
            authorize(self.extensions, actor, &operation)?;
        }
        let host = self.host;
        let exe = host.current_exe()?;
        let working_dir = host.current_dir()?;
        let node_dir = host.node_dir();
        let path_env = build_path_env(&exe, node_dir.as_deref());
        let (path, expected) = match self.manager {
            ServiceManager::Systemd => (
                systemd_unit_path(&host.config_home()?),
                render_systemd_unit(&exe, port, bind, &working_dir, &path_env),
            ),
            ServiceManager::Launchd => {
                let home = host.home()?;
                (
                    launchd_plist_path(&home),
                    render_launchd_plist(&exe, port, bind, &working_dir, &home, &path_env),
                )
            }
        };
        let Some(current) = host.read_file(&path)? else {
            anyhow::bail!(
                "no service unit installed at {}, run `pdo service install`",
                path.display()
            );
        };
        if current == expected {
            writeln!(out, "unchanged")?;
            return Ok(());
        }
        if node_dir.is_none() {
            writeln!(
                out,
                "warning: `node` was not found on $PATH — the unit's PATH will lack the node \
                 dir and the service daemon may fail to spawn claude/node."
            )?;
        }
        if dry_run {
            writeln!(out, "# pdo service refresh --dry-run (NO changes made)")?;
            writeln!(out, "# {} would be rewritten:", path.display())?;
            write!(out, "{}", unit_diff(&current, &expected))?;
            return Ok(());
        }
        host.write_file(&path, &expected)?;
        writeln!(out, "rewrote {}", path.display())?;
        // launchd rereads the plist when the job is bootstrapped again: the
        // restart that follows does it, not the refresh.
        if self.manager == ServiceManager::Systemd {
            run_checked(host, out, "systemctl", &["--user", "daemon-reload"])?;
        }
        Ok(())
    }
}
