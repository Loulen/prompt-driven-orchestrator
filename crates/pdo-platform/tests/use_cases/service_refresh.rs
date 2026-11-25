//! `pdo service refresh` against a recording host: the unit rewritten only
//! when its rendering changed, never a port probe, never an `enable`.

use std::path::{Path, PathBuf};

use pdo_kernel::extension_host::NoExtensions;
use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use pdo_platform::application::refresh_service::{
    RefreshService, RefreshServiceCommand, ServiceManager,
};
use pdo_platform::domain::service_unit::{
    build_path_env, launchd_plist_path, render_launchd_plist, render_systemd_unit,
    systemd_unit_path, PortState,
};

use crate::fakes::{FakeServiceHost, Host};

fn host(tmp: &Path) -> FakeServiceHost {
    // A port held by another process: refresh must not care, nor even ask.
    let mut host = FakeServiceHost::new(tmp.to_path_buf(), PortState::Foreign);
    host.home = tmp.join("home");
    host
}

/// The unit the host's binary renders, for `port`.
fn unit(exe: &str, port: u16) -> String {
    render_systemd_unit(
        Path::new(exe),
        port,
        None,
        Path::new("/home/user/.pdo/app"),
        &build_path_env(Path::new(exe), Some(Path::new("/opt/node/bin"))),
    )
}

fn seed(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn refresh(
    host: &FakeServiceHost,
    manager: ServiceManager,
    port: u16,
    dry_run: bool,
) -> anyhow::Result<String> {
    let mut out: Vec<u8> = Vec::new();
    RefreshService {
        host,
        extensions: &NoExtensions,
        manager,
    }
    .execute(
        &Actor::LocalOperator,
        RefreshServiceCommand {
            bind: None,
            port,
            dry_run,
        },
        &mut out,
    )?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn an_identical_unit_is_left_unchanged_with_no_write_and_no_reload() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = host(tmp.path());
    let path = systemd_unit_path(tmp.path());
    seed(&path, &unit("/home/user/.local/bin/pdo", 5172));
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();

    let out = refresh(&host, ServiceManager::Systemd, 5172, false).unwrap();

    assert_eq!(out, "unchanged\n");
    assert!(host.command_log().is_empty(), "no daemon-reload");
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(host.probes.get(), 0, "the port is never probed");
}

/// #699: a unit pointing into a versioned Cellar is rewritten to the stable
/// path, then systemd reloads it — and nothing else (no enable, no linger).
#[test]
fn a_unit_in_a_versioned_cellar_is_rewritten_to_the_stable_path_then_reloaded() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut host = host(tmp.path());
    host.exe = PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo");
    let path = systemd_unit_path(tmp.path());
    seed(
        &path,
        &unit("/home/linuxbrew/.linuxbrew/Cellar/pdo/2.90.0/bin/pdo", 5172),
    );

    let out = refresh(&host, ServiceManager::Systemd, 5172, false).unwrap();

    assert_eq!(
        out,
        format!(
            "rewrote {}\nsystemctl --user daemon-reload: ok\n",
            path.display()
        )
    );
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, unit("/home/linuxbrew/.linuxbrew/bin/pdo", 5172));
    assert!(!written.contains("Cellar"));
    assert_eq!(
        host.command_log(),
        vec![(
            "systemctl".to_string(),
            vec!["--user".to_string(), "daemon-reload".to_string()]
        )]
    );
    assert_eq!(host.probes.get(), 0, "the port is never probed");
}

#[test]
fn without_an_installed_unit_it_fails_pointing_at_service_install() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = host(tmp.path());
    for manager in [ServiceManager::Systemd, ServiceManager::Launchd] {
        for dry_run in [false, true] {
            let err = refresh(&host, manager, 5172, dry_run).unwrap_err();
            assert!(
                err.to_string().contains("run `pdo service install`"),
                "{err}"
            );
        }
    }
    assert!(!systemd_unit_path(tmp.path()).exists(), "never created");
    assert!(!launchd_plist_path(&tmp.path().join("home")).exists());
    assert!(host.command_log().is_empty());
    assert_eq!(host.probes.get(), 0);
}

#[test]
fn a_dry_run_shows_the_diff_and_writes_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = host(tmp.path());
    let path = systemd_unit_path(tmp.path());
    let old = unit("/home/user/.local/bin/pdo", 6183);
    seed(&path, &old);

    let out = refresh(&host, ServiceManager::Systemd, 5172, true).unwrap();

    assert!(out.starts_with("# pdo service refresh --dry-run (NO changes made)\n"));
    assert!(out.contains("- Environment=PDO_PORT=6183\n"), "{out}");
    assert!(out.contains("+ Environment=PDO_PORT=5172\n"), "{out}");
    assert!(out.contains("  KillMode=process\n"), "{out}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
    assert!(host.command_log().is_empty());
    assert_eq!(host.probes.get(), 0);

    // Already in line: the dry run says so too.
    seed(&path, &unit("/home/user/.local/bin/pdo", 5172));
    let out = refresh(&host, ServiceManager::Systemd, 5172, true).unwrap();
    assert_eq!(out, "unchanged\n");
}

/// Under launchd the plist is only written: the Update's restart (`bootout` +
/// `bootstrap`) has launchd read it again.
#[test]
fn a_launchd_plist_is_rewritten_without_any_launchctl() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = host(tmp.path());
    let home = tmp.path().join("home");
    let path = launchd_plist_path(&home);
    let render = |port| {
        render_launchd_plist(
            Path::new("/home/user/.local/bin/pdo"),
            port,
            None,
            Path::new("/home/user/.pdo/app"),
            &home,
            &build_path_env(
                Path::new("/home/user/.local/bin/pdo"),
                Some(Path::new("/opt/node/bin")),
            ),
        )
    };
    seed(&path, &render(6183));

    let out = refresh(&host, ServiceManager::Launchd, 5172, false).unwrap();

    assert_eq!(out, format!("rewrote {}\n", path.display()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), render(5172));
    assert!(host.command_log().is_empty(), "no launchctl at all");
    assert_eq!(host.probes.get(), 0);

    let out = refresh(&host, ServiceManager::Launchd, 5172, false).unwrap();
    assert_eq!(out, "unchanged\n");
}

#[test]
fn a_real_refresh_asks_to_update_the_service_a_dry_run_asks_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let host = host(tmp.path());
    seed(
        &systemd_unit_path(tmp.path()),
        &unit("/home/user/.local/bin/pdo", 6183),
    );
    let refusing = Host::refusing();
    let service = RefreshService {
        host: &host,
        extensions: refusing.as_ref(),
        manager: ServiceManager::Systemd,
    };
    let command = |dry_run| RefreshServiceCommand {
        bind: None,
        port: 5172,
        dry_run,
    };
    let mut out: Vec<u8> = Vec::new();
    service
        .execute(&Actor::LocalOperator, command(true), &mut out)
        .unwrap();
    assert!(refusing.asked().is_empty());
    let err = service
        .execute(&Actor::LocalOperator, command(false), &mut out)
        .unwrap_err();
    assert_eq!(err.to_string(), "operation refused: admins only");
    assert_eq!(
        refusing.asked(),
        vec![Operation::new(ResourceKind::Service, Verb::Update)]
    );
    assert!(host.command_log().is_empty());
}
