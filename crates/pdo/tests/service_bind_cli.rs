//! Layer 3a coverage for the service-install bind contract in #769.

fn dry_run(extra_args: &[&str]) -> String {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdo"))
        .current_dir(tempdir.path())
        .args(["service", "install", "--dry-run", "--port", "0"])
        .args(extra_args)
        .env("PDO_BIND", "192.0.2.10")
        .output()
        .expect("run service install --dry-run");
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("dry-run output is UTF-8")
}

#[test]
fn service_install_persists_only_an_explicit_bind() {
    let without_flag = dry_run(&[]);
    assert!(
        !without_flag.contains("PDO_BIND"),
        "PDO_BIND from the caller environment must not enter the service definition"
    );

    let with_flag = dry_run(&["--bind", "127.0.0.1"]);
    assert!(with_flag.contains("Environment=PDO_BIND=127.0.0.1"));
    assert!(!with_flag.contains("Environment=PDO_BIND=192.0.2.10"));
}
