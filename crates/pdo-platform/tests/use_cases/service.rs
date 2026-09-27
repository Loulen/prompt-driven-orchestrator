//! `pdo service` against a recording host: no real systemctl/launchctl, no
//! `~/.config` touched.

use std::path::{Path, PathBuf};

use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::Actor;
use pdo_platform::application::install_service::{InstallService, InstallServiceCommand};
use pdo_platform::application::uninstall_service::UninstallService;
use pdo_platform::domain::service_unit::{render_systemd_unit, systemd_unit_path, PortState};

use crate::fakes::{FakeServiceHost, Host};

fn install(
    host: &FakeServiceHost,
    bind: Option<&str>,
    port: u16,
    dry_run: bool,
) -> anyhow::Result<String> {
    let mut out: Vec<u8> = Vec::new();
    InstallService {
        host,
        extensions: &NoExtensions,
    }
    .execute(
        &Actor::LocalOperator,
        InstallServiceCommand {
            bind: bind.map(|b| b.parse().unwrap()),
            port,
            dry_run,
        },
        &mut out,
    )?;
    Ok(String::from_utf8(out).unwrap())
}

/// #699: the unit carries the path it was GIVEN — Homebrew's stable `bin/pdo`
/// symlink — verbatim, never the resolved `Cellar/pdo/<version>/` target that
/// `brew upgrade` deletes. `--dry-run` on such an install shows `bin/pdo`.
#[test]
fn service_install_writes_the_given_stable_path_verbatim() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    host.exe = PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo");
    let s = install(&host, None, 5172, true).unwrap();
    if cfg!(target_os = "macos") {
        assert!(s.contains("<string>/home/linuxbrew/.linuxbrew/bin/pdo</string>"));
    } else {
        assert!(
            s.contains("ExecStart=/home/linuxbrew/.linuxbrew/bin/pdo daemon"),
            "the stable path, verbatim: {s}"
        );
        assert!(s.contains("Environment=PATH=/home/linuxbrew/.linuxbrew/bin:"));
    }
    assert!(!s.contains("Cellar"), "never the versioned target: {s}");
}

#[test]
fn service_dry_run_is_side_effect_free_and_prints_unit_and_plan() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    let s = install(&host, None, 6183, true).unwrap();
    if !cfg!(target_os = "macos") {
        assert!(s.contains("KillMode=process"), "unit missing KillMode: {s}");
        assert!(s.contains("Environment=PDO_PORT=6183"));
        assert!(!s.contains("Environment=PDO_BIND="));
        assert!(s.contains("ExecStart=/home/user/.local/bin/pdo daemon"));
        assert!(s.contains("WorkingDirectory=/home/user/.pdo/app"));
        // Command plan, in order.
        assert!(s.contains("systemctl --user daemon-reload"));
        assert!(s.contains("loginctl enable-linger user"));
        assert!(s.contains("systemctl --user enable --now pdo"));
    }
    assert!(host.command_log().is_empty(), "dry-run ran commands!");
    assert!(
        !systemd_unit_path(tmp.path()).exists(),
        "dry-run wrote the unit file!"
    );
}

#[test]
fn a_dry_run_asks_no_authorization_a_real_install_does() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    let refusing = Host::refusing();
    let mut out: Vec<u8> = Vec::new();
    let service = InstallService {
        host: &host,
        extensions: refusing.as_ref(),
    };
    let command = |dry_run| InstallServiceCommand {
        bind: None,
        port: 6183,
        dry_run,
    };
    service
        .execute(&Actor::LocalOperator, command(true), &mut out)
        .unwrap();
    let err = service
        .execute(&Actor::LocalOperator, command(false), &mut out)
        .unwrap_err();
    assert_eq!(err.to_string(), "operation refused: admins only");
    assert!(host.command_log().is_empty());
    let err = UninstallService {
        host: &host,
        extensions: refusing.as_ref(),
    }
    .execute(&Actor::LocalOperator, &mut out)
    .unwrap_err();
    assert!(err.to_string().contains("refused"));
}

#[test]
fn service_dry_run_carries_bind_and_the_harness_path_comment() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    let s = install(&host, Some("127.0.0.1"), 6183, true).unwrap();
    if cfg!(target_os = "macos") {
        assert!(s.contains("<key>PDO_BIND</key><string>127.0.0.1</string>"));
    } else {
        assert!(s.contains("Environment=PDO_BIND=127.0.0.1"));
    }
    assert!(s.contains("interactive shell PATH (ADR-0055)"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn service_install_writes_unit_and_runs_exact_systemctl_sequence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    install(&host, None, 6183, false).unwrap();

    // The unit file was written with byte-identical content.
    let written = std::fs::read_to_string(systemd_unit_path(tmp.path())).unwrap();
    let expected = render_systemd_unit(
        Path::new("/home/user/.local/bin/pdo"),
        6183,
        None,
        Path::new("/home/user/.pdo/app"),
        "/home/user/.local/bin:/opt/node/bin:/usr/local/bin:/usr/bin:/bin",
    );
    assert_eq!(written, expected);

    // The exact command sequence, in order (D4 install flow).
    assert_eq!(
        host.command_log(),
        vec![
            (
                "systemctl".into(),
                vec!["--user".into(), "daemon-reload".into()]
            ),
            (
                "loginctl".into(),
                vec!["enable-linger".into(), "user".into()]
            ),
            (
                "systemctl".into(),
                vec![
                    "--user".into(),
                    "enable".into(),
                    "--now".into(),
                    "pdo".into()
                ]
            ),
        ]
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn service_install_refuses_a_foreign_port_and_touches_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Foreign);
    let err = install(&host, None, 6183, false).unwrap_err();
    assert!(
        err.to_string().contains("refusing to install"),
        "expected loud refusal, got: {err}"
    );
    assert!(host.command_log().is_empty());
    assert!(!systemd_unit_path(tmp.path()).exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn service_install_is_idempotent_when_a_pdo_daemon_already_owns_the_port() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::PdoDaemon);
    install(&host, None, 6183, false).unwrap();
    // Unit is still written + enabled, but WITHOUT `--now` (no competitor).
    assert!(systemd_unit_path(tmp.path()).exists());
    let log = host.command_log();
    let enable = log
        .iter()
        .find(|(p, a)| p == "systemctl" && a.contains(&"enable".to_string()))
        .expect("expected an enable command");
    assert!(
        !enable.1.contains(&"--now".to_string()),
        "enable must NOT carry --now when a daemon already owns the port: {enable:?}"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn service_uninstall_disables_removes_and_reloads() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = FakeServiceHost::new(tmp.path().to_path_buf(), PortState::Free);
    // Seed a unit file so removal is observable.
    let unit_path = systemd_unit_path(tmp.path());
    std::fs::create_dir_all(unit_path.parent().unwrap()).unwrap();
    std::fs::write(&unit_path, "stub").unwrap();

    let mut out: Vec<u8> = Vec::new();
    UninstallService {
        host: &host,
        extensions: &NoExtensions,
    }
    .execute(&Actor::LocalOperator, &mut out)
    .unwrap();

    assert!(!unit_path.exists(), "unit file should be removed");
    assert_eq!(
        host.command_log(),
        vec![
            (
                "systemctl".into(),
                vec![
                    "--user".into(),
                    "disable".into(),
                    "--now".into(),
                    "pdo".into()
                ]
            ),
            (
                "systemctl".into(),
                vec!["--user".into(), "daemon-reload".into()]
            ),
        ]
    );
}
