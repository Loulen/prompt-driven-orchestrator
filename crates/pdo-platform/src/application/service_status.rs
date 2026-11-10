//! Show the OS service's status (`pdo service status`). Read-only: never
//! mutates, so it is no Operation, and a non-zero exit (inactive or missing
//! unit) is not an error.

use std::io::Write;

use anyhow::Result;

use super::ports::ServiceHost;
use crate::domain::service_unit::LAUNCHD_LABEL;

pub struct ServiceStatus<'a> {
    pub host: &'a dyn ServiceHost,
}

impl ServiceStatus<'_> {
    pub fn execute(&self, out: &mut dyn Write) -> Result<()> {
        if cfg!(target_os = "macos") {
            status_launchd(self.host, out)
        } else {
            status_systemd(self.host, out)
        }
    }
}

/// `pdo service status` — Linux. Read-only: prints the unit's status and never
/// mutates state. A non-zero exit (inactive/missing unit) is not an error.
fn status_systemd(env: &dyn ServiceHost, out: &mut dyn std::io::Write) -> Result<()> {
    match env.run_cmd("systemctl", &["--user", "--no-pager", "status", "pdo"]) {
        Ok(o) => {
            write!(out, "{}", o.stdout)?;
            if !o.stderr.is_empty() {
                write!(out, "{}", o.stderr)?;
            }
        }
        Err(e) => writeln!(out, "could not run systemctl ({e})")?,
    }
    Ok(())
}

/// `pdo service status` — macOS launchd path. Read-only.
fn status_launchd(env: &dyn ServiceHost, out: &mut dyn std::io::Write) -> Result<()> {
    let label = LAUNCHD_LABEL;
    let uid = env
        .run_cmd("id", &["-u"])
        .map(|o| o.stdout.trim().to_string())
        .unwrap_or_default();
    let target = format!("gui/{uid}/{label}");
    match env.run_cmd("launchctl", &["print", &target]) {
        Ok(o) => {
            write!(out, "{}", o.stdout)?;
            if !o.stderr.is_empty() {
                write!(out, "{}", o.stderr)?;
            }
        }
        Err(e) => writeln!(out, "could not run launchctl ({e})")?,
    }
    Ok(())
}
