//! `pdo::main_with(extensions)` (#23, ADR-0081): the whole `pdo` binary of a
//! distribution, driven here with the internal test extension.
//!
//! - the extensions of the binary rule on `pdo service` (the Opération of its
//!   install), as they do on the daemon's;
//! - the factory is called once by `daemon` and `service`, never by the
//!   agent's CLI (`complete`, `fail`, `run create`…).
//!
//! Each case runs `pdo::main_from` in a **child process**: this test binary,
//! re-run on [`main_from_child`] alone. In process, the case would read the
//! caller's `PDO_*` environment (a node session's `pdo complete` would reach
//! the real daemon) and could not capture its stderr or exit code. The child
//! gets a scratch `HOME`, no `PDO_*` variable but the ones the case sets, an
//! unreachable daemon, and a `PATH` whose `systemctl` refuses everything.

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use crate::test_extension::{Refusal, TestExtension};

/// The command line of the child, after the program name, split on `\x1f`.
const ARGS_ENV: &str = "PDO_TEST_MAIN_FROM_ARGS";
/// What the child's factory does: `panic`, or `count:<refusal>` (build the
/// test extension refusing `<refusal>`), or `count:incompatible` (build one
/// requiring an API this build does not implement).
const FACTORY_ENV: &str = "PDO_TEST_MAIN_FROM_FACTORY";
/// The child's last stderr line: how many times the factory was called.
const CALLS_MARKER: &str = "main_from factory calls: ";
/// The child's last stderr line but one: how many times `configure` was called.
const CONFIGURE_MARKER: &str = "main_from configure calls: ";
const FACTORY_PANIC: &str = "the factory of the extensions was called";
/// A daemon nothing listens on.
const NO_DAEMON: &str = "http://127.0.0.1:1";

/// The child's side: run `main_from` as the case asks, report the factory
/// calls, exit with its code. A no-op when run without a case (`--ignored`).
#[test]
#[ignore = "the child process of the other tests of this file"]
fn main_from_child() {
    let Ok(args) = std::env::var(ARGS_ENV) else {
        return;
    };
    let factory = std::env::var(FACTORY_ENV).unwrap_or_default();
    let args = std::iter::once("pdo").chain(args.split('\x1f'));
    let mut calls = 0;
    let mut configured = 0;
    let code = pdo::main_from_config(
        args,
        || {
            calls += 1;
            match factory.split_once(':') {
                Some(("count", "incompatible")) => TestExtension::new(Refusal::Nothing)
                    .0
                    .requiring_api("99.0.0")
                    .boxed(),
                Some(("count", refusal)) => TestExtension::new(Refusal::parse(refusal).unwrap())
                    .0
                    .boxed(),
                _ => panic!("{FACTORY_PANIC}"),
            }
        },
        |config| {
            configured += 1;
            config.with_release_channel(pdo::ReleaseChannel {
                name: "acme".into(),
                installed_version: "0.3.0".into(),
                source_url: None,
                upgrade_command: None,
            })
        },
    );
    eprintln!("{CONFIGURE_MARKER}{configured}");
    eprintln!("{CALLS_MARKER}{calls}");
    let code = (0..=u8::MAX)
        .find(|n| ExitCode::from(*n) == code)
        .expect("an exit code is a u8");
    std::process::exit(i32::from(code));
}

struct Child {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    home: tempfile::TempDir,
}

impl Child {
    /// How many times the factory was called, as the child reported it.
    fn calls(&self) -> usize {
        self.stderr
            .lines()
            .find_map(|l| l.strip_prefix(CALLS_MARKER))
            .unwrap_or_else(|| panic!("the child reported no factory calls:\n{self}"))
            .trim()
            .parse()
            .unwrap()
    }

    /// How many times `configure` was called, as the child reported it.
    fn configured(&self) -> usize {
        self.stderr
            .lines()
            .find_map(|l| l.strip_prefix(CONFIGURE_MARKER))
            .unwrap_or_else(|| panic!("the child reported no configure calls:\n{self}"))
            .trim()
            .parse()
            .unwrap()
    }
}

impl std::fmt::Display for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit={:?}\n--- stdout\n{}\n--- stderr\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}

/// Run `pdo <args>` through `main_from` in a child, the factory set to
/// `factory`, with `env` on top of the sandbox.
fn main_from(args: &[&str], factory: &str, env: &[(&str, &str)]) -> Child {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["systemctl", "loginctl", "launchctl"] {
        fake_failing_tool(&bin.join(tool));
    }
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "main_with::main_from_child",
        "--ignored",
        "--nocapture",
    ]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("PDO_") {
            cmd.env_remove(key);
        }
    }
    cmd.current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("PATH", &bin)
        .env("PDO_DAEMON_URL", NO_DAEMON)
        .env("PDO_UPDATE_CHECK", "0")
        .env(ARGS_ENV, args.join("\x1f"))
        .env(FACTORY_ENV, factory)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (code, stdout, stderr) = output_within(cmd, Duration::from_secs(60));
    Child {
        code,
        stdout,
        stderr,
        home,
    }
}

fn fake_failing_tool(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, "#!/bin/sh\necho \"fake $0 refused\" >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The output of `cmd`, killed past `limit` (a daemon that should have
/// refused to start would otherwise serve forever).
fn output_within(mut cmd: Command, limit: Duration) -> (Option<i32>, String, String) {
    use std::io::Read;
    let mut child = cmd.spawn().expect("spawn the child");
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let (out, err) = (out.join().unwrap(), err.join().unwrap());
    assert!(status.is_some(), "the child ran past {limit:?}:\n{err}");
    (status.and_then(|s| s.code()), out, err)
}

/// The Opération of `pdo service install` goes to the binary's extensions: a
/// refusal stops it with their reason, before anything is written. (A dry run
/// changes nothing, so it asks no authorization: the refusal is proven on a
/// real install, whose `systemctl` is a fake in the child's `PATH`.)
#[test]
fn a_service_install_is_refused_by_the_extensions_of_the_binary() {
    let child = main_from(
        &["service", "install", "--port", "0"],
        "count:service:create",
        &[],
    );
    assert_eq!(child.code, Some(1), "{child}");
    assert!(
        child
            .stderr
            .contains("the test extension refuses create service"),
        "the refusal carries the extension's reason:\n{child}"
    );
    assert_eq!(child.calls(), 1, "{child}");
    let unit = child.home.path().join(".config/systemd/user/pdo.service");
    assert!(!unit.exists(), "a refused install writes nothing:\n{child}");
    assert!(
        !child.stderr.contains("fake"),
        "no systemctl call:\n{child}"
    );
}

/// Without a refusal, `pdo service install --dry-run` goes through the same
/// entry point, with the extensions registered once, and prints the unit.
#[test]
fn a_service_install_dry_run_passes_with_the_extensions_of_the_binary() {
    let child = main_from(
        &["service", "install", "--dry-run", "--port", "0"],
        "count:none",
        &[],
    );
    assert_eq!(child.code, Some(0), "{child}");
    assert!(child.stdout.contains("ExecStart="), "{child}");
    assert_eq!(child.calls(), 1, "{child}");
}

/// `pdo daemon` serves with the binary's extensions: it builds them once, and
/// an extension it cannot honour stops it before it opens anything.
#[test]
fn the_daemon_builds_the_extensions_of_the_binary_once() {
    let child = main_from(
        &["daemon", "--bind", "127.0.0.1", "--port", "0"],
        "count:incompatible",
        &[],
    );
    assert_eq!(child.code, Some(1), "{child}");
    assert!(
        child.stderr.contains("requires extension API 99.0.0"),
        "{child}"
    );
    assert_eq!(child.calls(), 1, "{child}");
    assert_eq!(
        child.configured(),
        1,
        "`configure` shapes the daemon's Config:\n{child}"
    );
}

/// `pdo service status` asks nothing of the extensions, but the service arm
/// still builds them once: one rule for the whole `service` subcommand.
#[test]
fn the_service_status_builds_the_extensions_of_the_binary_once() {
    let child = main_from(&["service", "status"], "count:none", &[]);
    assert_eq!(child.calls(), 1, "{child}");
    assert_eq!(
        child.configured(),
        0,
        "only the daemon is configured:\n{child}"
    );
}

/// The agent's CLI never builds the extensions: the factory panics if called,
/// and each command still answers with its own exit code (no daemon: `1`).
#[test]
fn the_agent_cli_never_builds_the_extensions() {
    let session = [("PDO_RUN_ID", "no-such-run"), ("PDO_NODE_ID", "solo")];
    for args in [
        &["complete"][..],
        &["fail", "--reason", "x"],
        &["skip", "--reason", "x"],
        &["wait-user"],
        &["run", "create", "some-pipeline"],
    ] {
        let child = main_from(args, "panic", &session);
        assert!(!child.stderr.contains(FACTORY_PANIC), "{args:?}: {child}");
        assert_eq!(child.calls(), 0, "{args:?}: {child}");
        assert_eq!(child.configured(), 0, "{args:?}: {child}");
        assert_eq!(child.code, Some(1), "{args:?}: {child}");
    }
}

/// `--help` and a bad command line leave through `main_from` with clap's own
/// codes, the factory untouched.
#[test]
fn help_and_usage_errors_keep_clap_exit_codes() {
    let child = main_from(&["--help"], "panic", &[]);
    assert_eq!(child.code, Some(0), "{child}");
    assert!(child.stdout.contains("Usage: pdo <COMMAND>"), "{child}");
    let child = main_from(&["no-such-subcommand"], "panic", &[]);
    assert_eq!(child.code, Some(2), "{child}");
    assert!(child.stderr.contains("unrecognized subcommand"), "{child}");
    assert_eq!(child.calls(), 0, "{child}");
}
