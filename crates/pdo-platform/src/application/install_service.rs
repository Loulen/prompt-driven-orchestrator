//! Install the daemon as an OS service (`pdo service install`, ADR-0019): a
//! `systemd --user` unit on Linux, a launchd LaunchAgent on macOS (best-effort),
//! so it starts at boot and survives logout.

use std::io::Write;
use std::net::IpAddr;

use anyhow::{Context, Result};
use pdo_kernel::authorization::authorize;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::ports::ServiceHost;
use crate::domain::service_unit::{
    build_path_env, describe_port_state, launchd_plist_path, render_launchd_plist,
    render_systemd_unit, service_addr, systemd_unit_path, PortState, LAUNCHD_LABEL,
};

pub struct InstallServiceCommand {
    /// Address the service daemon listens on; `None` keeps the default.
    pub bind: Option<IpAddr>,
    pub port: u16,
    /// Print the unit file + the exact commands, make NO changes. Still probes
    /// the port so the conflict guard is visible.
    pub dry_run: bool,
}

pub struct InstallService<'a> {
    pub host: &'a dyn ServiceHost,
    pub extensions: &'a dyn ExtensionHost,
}

impl InstallService<'_> {
    /// Install for `actor`, writing the trace to `out`. A dry run changes
    /// nothing, so it asks no authorization.
    pub fn execute(
        &self,
        actor: &Actor,
        command: InstallServiceCommand,
        out: &mut dyn Write,
    ) -> Result<()> {
        let InstallServiceCommand {
            bind,
            port,
            dry_run,
        } = command;
        if !dry_run {
            let operation = Operation::new(ResourceKind::Service, Verb::Create);
            authorize(self.extensions, actor, &operation)?;
        }
        if cfg!(target_os = "macos") {
            install_launchd(self.host, out, bind, port, dry_run)
        } else {
            install_systemd(self.host, out, bind, port, dry_run)
        }
    }
}

/// `pdo service install` — Linux systemd `--user` path.
fn install_systemd(
    env: &dyn ServiceHost,
    out: &mut dyn std::io::Write,
    bind: Option<IpAddr>,
    port: u16,
    dry_run: bool,
) -> Result<()> {
    let exe = env.current_exe()?;
    let working_dir = env.current_dir()?;
    let node_dir = env.node_dir();
    let path_env = build_path_env(&exe, node_dir.as_deref());
    let user = env.user().unwrap_or_else(|_| "$USER".to_string());
    let config_home = env.config_home()?;
    let unit_path = systemd_unit_path(&config_home);
    let unit = render_systemd_unit(&exe, port, bind, &working_dir, &path_env);
    let service_addr = service_addr(bind, port);
    let state = env.probe_port(service_addr);

    // `--now` only when the port is free: never start a competitor onto a port a
    // PDO daemon already owns.
    let start_now = state == PortState::Free;
    let enable_args: &[&str] = if start_now {
        &["--user", "enable", "--now", "pdo"]
    } else {
        &["--user", "enable", "pdo"]
    };

    if node_dir.is_none() {
        writeln!(
            out,
            "# WARNING: `node` was not found on $PATH — the unit's Environment=PATH \
             will lack the node dir and the service daemon may fail to spawn claude/node."
        )?;
    }

    if dry_run {
        writeln!(out, "# pdo service install --dry-run (NO changes made)")?;
        writeln!(out, "# {}", describe_port_state(service_addr, &state))?;
        if state == PortState::Foreign {
            writeln!(
                out,
                "# WARNING: a real install would REFUSE — enabling a unit here would \
                 crash-loop on EADDRINUSE. Stop the process on port {port} or pass --port."
            )?;
        } else if state == PortState::PdoDaemon {
            writeln!(
                out,
                "# NOTE: a PDO daemon already serves this port; a real install would enable \
                 the unit for boot WITHOUT starting a competing instance (no --now)."
            )?;
        }
        writeln!(out, "# --- unit file ({}) ---", unit_path.display())?;
        write!(out, "{unit}")?;
        writeln!(out, "# --- command plan ---")?;
        writeln!(out, "#   write unit to {}", unit_path.display())?;
        writeln!(out, "#   systemctl --user daemon-reload")?;
        writeln!(out, "#   loginctl enable-linger {user}")?;
        writeln!(out, "#   systemctl {}", enable_args.join(" "))?;
        return Ok(());
    }

    if state == PortState::Foreign {
        anyhow::bail!(
            "refusing to install: {}. The enabled unit's daemon would crash-loop on \
             EADDRINUSE (Restart=on-failure). Stop the process on port {port}, or install \
             with a free --port.",
            describe_port_state(service_addr, &state)
        );
    }
    if state == PortState::PdoDaemon {
        writeln!(
            out,
            "note: {} — enabling the unit for boot but NOT starting a competing instance.",
            describe_port_state(service_addr, &state)
        )?;
    }

    env.write_file(&unit_path, &unit)?;
    writeln!(out, "wrote {}", unit_path.display())?;

    run_checked(env, out, "systemctl", &["--user", "daemon-reload"])?;

    // `enable-linger` lets the user's units run without a login. On a
    // hardened/headless box it can fail — degrade with a hint, don't abort.
    match env.run_cmd("loginctl", &["enable-linger", &user]) {
        Ok(o) if o.success => writeln!(out, "loginctl enable-linger {user}: ok")?,
        Ok(o) => writeln!(
            out,
            "warning: `loginctl enable-linger {user}` failed ({}). Run `sudo loginctl \
             enable-linger {user}` so the daemon survives logout.",
            o.stderr.trim()
        )?,
        Err(e) => writeln!(
            out,
            "warning: could not run loginctl ({e}). Run `sudo loginctl enable-linger {user}`."
        )?,
    }

    run_checked(env, out, "systemctl", enable_args)?;

    writeln!(
        out,
        "\nPDO service installed. The daemon will now start at boot and survive logout."
    )?;
    if !start_now {
        writeln!(
            out,
            "It was enabled but not started (a daemon already owns port {port})."
        )?;
    }
    Ok(())
}

/// `pdo service install` — macOS launchd LaunchAgent path.
/// **Best-effort / untested on the Linux CI box** (generation is golden-tested).
fn install_launchd(
    env: &dyn ServiceHost,
    out: &mut dyn std::io::Write,
    bind: Option<IpAddr>,
    port: u16,
    dry_run: bool,
) -> Result<()> {
    let exe = env.current_exe()?;
    let working_dir = env.current_dir()?;
    let home = env.home()?;
    let node_dir = env.node_dir();
    let path_env = build_path_env(&exe, node_dir.as_deref());
    let plist_path = launchd_plist_path(&home);
    let plist = render_launchd_plist(&exe, port, bind, &working_dir, &home, &path_env);
    let service_addr = service_addr(bind, port);
    let state = env.probe_port(service_addr);
    let label = LAUNCHD_LABEL;

    if node_dir.is_none() {
        writeln!(
            out,
            "# WARNING: `node` was not found on $PATH — the plist's PATH will lack the \
             node dir and the service daemon may fail to spawn claude/node."
        )?;
    }

    if dry_run {
        writeln!(out, "# pdo service install --dry-run (NO changes made)")?;
        writeln!(out, "# {}", describe_port_state(service_addr, &state))?;
        if state == PortState::Foreign {
            writeln!(
                out,
                "# WARNING: a real install would REFUSE — the daemon would crash-loop on \
                 EADDRINUSE. Free port {port} or pass --port."
            )?;
        }
        writeln!(
            out,
            "# --- LaunchAgent plist ({}) ---",
            plist_path.display()
        )?;
        write!(out, "{plist}")?;
        writeln!(out, "# --- command plan ---")?;
        writeln!(out, "#   write plist to {}", plist_path.display())?;
        writeln!(
            out,
            "#   launchctl bootout gui/$(id -u)/{label}  (ignore error)"
        )?;
        writeln!(
            out,
            "#   launchctl bootstrap gui/$(id -u) {}",
            plist_path.display()
        )?;
        writeln!(
            out,
            "# NOTE: LaunchAgents do NOT run while logged out (no linger equivalent); \
             true headless needs a root LaunchDaemon (deferred, ADR-0019)."
        )?;
        return Ok(());
    }

    if state == PortState::Foreign {
        anyhow::bail!(
            "refusing to install: {}. Free the port or pass a free --port.",
            describe_port_state(service_addr, &state)
        );
    }

    env.write_file(&plist_path, &plist)?;
    writeln!(out, "wrote {}", plist_path.display())?;

    let uid = env
        .run_cmd("id", &["-u"])
        .map(|o| o.stdout.trim().to_string());
    let uid = match uid {
        Ok(u) if !u.is_empty() => u,
        _ => anyhow::bail!("could not determine uid via `id -u`"),
    };
    let target = format!("gui/{uid}");
    let bootout_target = format!("{target}/{label}");
    // Bootstrap is not idempotent — bootout first (ignore error), then bootstrap.
    let _ = env.run_cmd("launchctl", &["bootout", &bootout_target]);
    run_checked(
        env,
        out,
        "launchctl",
        &["bootstrap", &target, &plist_path.to_string_lossy()],
    )?;
    writeln!(
        out,
        "\nPDO LaunchAgent installed (best-effort). Note: it will NOT run while logged out."
    )?;
    Ok(())
}

/// Run a side-effect command, printing a one-line trace, and fail the install if
/// it returns non-zero (the steps that MUST succeed: `daemon-reload`,
/// `enable`, `bootstrap`).
pub(crate) fn run_checked(
    env: &dyn ServiceHost,
    out: &mut dyn std::io::Write,
    program: &str,
    args: &[&str],
) -> Result<()> {
    let o = env
        .run_cmd(program, args)
        .with_context(|| format!("failed to run `{program} {}`", args.join(" ")))?;
    if o.success {
        writeln!(out, "{program} {}: ok", args.join(" "))?;
        Ok(())
    } else {
        anyhow::bail!(
            "`{program} {}` failed: {}{}",
            args.join(" "),
            o.stdout.trim(),
            o.stderr.trim()
        )
    }
}
