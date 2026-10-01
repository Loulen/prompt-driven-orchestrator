//! #961 / ADR-0056 §1 ter end-to-end: the **vibe** catalogue is deduced from the
//! source its installation actually enumerates in — the `[[models]]` of its own
//! `config.toml` under its home — because its binary enumerates nothing: `--help`
//! declares neither `completion`, nor `help`, nor `--list-models`, nor an RPC mode,
//! and describes no model.
//!
//! Layer-3, through the real HTTP surface, against a fake binary **named `vibe`** on
//! the probe `PATH` and a `config.toml` planted under the daemon's home override: the
//! builtin vibe descriptor resolves to the fake, so what this test reads off
//! `GET /settings` is what the inspector's pickers render for a node pinned on `vibe`.
//!
//! The fake **hangs** on any argv but `--help`/`--version` (the measured `claude`
//! hazard): a guessed source would show in the trace and cost a visible timeout.
//!
//! The probe `PATH` is process-global, so this test serialises on
//! `HARNESS_PROBE_ENV_LOCK` with the other catalogue tests and installs its fake beside
//! theirs in the shared, process-wide dir `common::fake_harness_bindir`.

use crate::common::TestDaemon;

/// The real `vibe --help` of 2.25.8, verbatim.
const HELP: &str = include_str!("fixtures/catalogue/vibe-2.25.8-help.txt");
/// A real `~/.vibe/config.toml` written by vibe 2.25.8 (paths anonymised; no
/// secret — the key lives in `.env`).
const CONFIG: &str = include_str!("fixtures/catalogue/vibe-2.25.8-config.toml");

#[cfg(unix)]
fn write_fake_vibe(dir: &std::path::Path, version: &str) {
    use std::os::unix::fs::PermissionsExt;
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.trace\"\ncase \"$*\" in\n  '--version') printf '%s\\n' {};;\n  '--help') printf '%s' {};;\n  *) sleep 30;;\nesac\n",
        sh_single_quote(version),
        sh_single_quote(HELP),
    );
    let bin = dir.join("vibe");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_file(vibe_trace(dir));
}

#[cfg(unix)]
fn vibe_trace(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("vibe.trace")
}

fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn served_harness(settings: &serde_json::Value, harness: &str) -> serde_json::Value {
    settings["harness_descriptors"]["harnesses"]
        .as_array()
        .expect("harness list present")
        .iter()
        .find(|h| h["name"] == harness)
        .unwrap_or_else(|| panic!("{harness} is listed"))
        .clone()
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn vibes_catalogue_comes_from_its_config_file_and_it_has_no_effort_axis() {
    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    let bindir = crate::common::fake_harness_bindir();
    write_fake_vibe(&bindir, "vibe 2.25.8");

    // `vibe` is a **builtin** harness; its catalogue file lives under `~/.vibe`,
    // which the daemon resolves against its home override.
    let daemon = TestDaemon::spawn_with_home_override(
        |home| {
            std::fs::create_dir_all(home.join(".vibe"))?;
            std::fs::write(home.join(".vibe").join("config.toml"), CONFIG)?;
            Ok(())
        },
        None,
    )
    .await
    .unwrap();

    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // ADR-0056 §1 bis: nothing the binary does not declare is run.
    let trace = std::fs::read_to_string(vibe_trace(&bindir)).unwrap_or_default();
    let ran: Vec<&str> = trace.lines().collect();
    assert!(ran.contains(&"--help"), "{ran:?}");
    assert!(
        ran.iter()
            .all(|argv| *argv == "--help" || *argv == "--version"),
        "vibe declares no catalogue source in its binary — only --help/--version may run: {ran:?}"
    );

    let vibe = served_harness(&settings, "vibe");
    assert_eq!(vibe["source"], "builtin", "vibe is on the embedded floor");
    assert_eq!(vibe["installed"], true, "the fake binary resolves on PATH");
    assert_eq!(vibe["version"], "vibe 2.25.8");

    // AC: models = alias-or-name of each `[[models]]`, file order.
    assert_eq!(
        strings(&vibe["models"]),
        vec!["mistral-medium-3.5", "devstral-small", "local"]
    );
    // AC: no effort axis (greyed picker), no context window hint.
    assert_eq!(strings(&vibe["efforts"]), Vec::<String>::new());
    assert_eq!(vibe["has_effort"], false);
    assert_eq!(vibe["model_contexts"], serde_json::json!({}));
    assert_eq!(vibe["model_efforts"], serde_json::json!({}));
}

#[cfg(unix)]
#[tokio::test]
async fn a_vibe_never_launched_has_an_empty_offer_not_an_error() {
    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    let bindir = crate::common::fake_harness_bindir();
    write_fake_vibe(&bindir, "vibe 2.25.8");

    // No `.vibe/config.toml` under the home: the file source has nothing to read.
    let daemon = TestDaemon::spawn_with_home_override(|_home| Ok(()), None)
        .await
        .unwrap();
    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let vibe = served_harness(&settings, "vibe");
    assert_eq!(vibe["installed"], true);
    assert_eq!(strings(&vibe["models"]), Vec::<String>::new());
    assert_eq!(vibe["has_effort"], false);
}
