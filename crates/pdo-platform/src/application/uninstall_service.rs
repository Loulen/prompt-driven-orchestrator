//! Remove the OS service (`pdo service uninstall`): stop, disable, and remove
//! the unit. Idempotent: an absent unit is not an error.

use std::io::Write;

use anyhow::Result;
use pdo_kernel::authorization::authorize;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::install_service::run_checked;
use super::ports::ServiceHost;
use crate::domain::service_unit::{launchd_plist_path, systemd_unit_path, LAUNCHD_LABEL};

pub struct UninstallService<'a> {
    pub host: &'a dyn ServiceHost,
    pub extensions: &'a dyn ExtensionHost,
}

impl UninstallService<'_> {
    pub fn execute(&self, actor: &Actor, out: &mut dyn Write) -> Result<()> {
        let operation = Operation::new(ResourceKind::Service, Verb::Delete);
        authorize(self.extensions, actor, &operation)?;
        if cfg!(target_os = "macos") {
            uninstall_launchd(self.host, out)
        } else {
            uninstall_systemd(self.host, out)
        }
    }
}

/// `pdo service uninstall` — Linux systemd `--user` path. Leaves linger (it's
/// user-global, not this unit's to revoke).
fn uninstall_systemd(env: &dyn ServiceHost, out: &mut dyn std::io::Write) -> Result<()> {
    let config_home = env.config_home()?;
    let unit_path = systemd_unit_path(&config_home);

    // Tolerate a non-zero exit (the unit may be absent) so uninstall is idempotent.
    match env.run_cmd("systemctl", &["--user", "disable", "--now", "pdo"]) {
        Ok(o) if o.success => writeln!(out, "systemctl --user disable --now pdo: ok")?,
        Ok(o) => writeln!(
            out,
            "systemctl --user disable --now pdo: {} (continuing)",
            o.stderr.trim()
        )?,
        Err(e) => writeln!(out, "warning: could not run systemctl ({e})")?,
    }

    env.remove_file(&unit_path)?;
    writeln!(out, "removed {}", unit_path.display())?;

    run_checked(env, out, "systemctl", &["--user", "daemon-reload"])?;
    writeln!(
        out,
        "\nPDO service uninstalled (linger left intact — it is user-global)."
    )?;
    Ok(())
}

/// `pdo service uninstall` — macOS launchd path.
fn uninstall_launchd(env: &dyn ServiceHost, out: &mut dyn std::io::Write) -> Result<()> {
    let home = env.home()?;
    let plist_path = launchd_plist_path(&home);
    let label = LAUNCHD_LABEL;

    let uid = env
        .run_cmd("id", &["-u"])
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    if !uid.is_empty() {
        let _ = env.run_cmd("launchctl", &["bootout", &format!("gui/{uid}/{label}")]);
    }
    env.remove_file(&plist_path)?;
    writeln!(out, "removed {}", plist_path.display())?;
    writeln!(out, "\nPDO LaunchAgent uninstalled.")?;
    Ok(())
}
