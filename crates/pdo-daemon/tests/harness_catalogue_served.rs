//! #616 / ADR-0053 end-to-end: the daemon **deduces** a harness's model & effort
//! catalogue from the installed binary and **serves** it on `GET /settings`, with
//! the probed version — deduced, never hard-coded. A declared harness whose fake
//! binary prints an enumerated `--help` shows up with its models, its effort stops,
//! `has_effort: true`, and a `version`; the client renders that instead of knowing
//! a catalogue.
//!
//! Layer-3, through the real HTTP surface. Its own test binary (one process) so the
//! process-global `PDO_HARNESS_PROBE_PATH` (a `OnceLock`) is set once, before any
//! probe runs, pointing at a tempdir holding a self-contained fake binary.

use crate::common::TestDaemon;

/// Write a self-contained fake harness binary that answers `--version` and prints an
/// enumerated `--help`. `printf` is a `/bin/sh` builtin, so it runs even though the
/// probe restricts `PATH` to this dir.
#[cfg(unix)]
fn write_fake_binary(dir: &std::path::Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    let help = "  --model <m>  [gpt-5|gpt-5-codex|o4-mini]\n  --effort <e>  One of: min, low, medium, high, max";
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'probe-harness 1.402';;\n  --help) printf '%s' '{help}';;\nesac\n"
    );
    let bin = dir.join(name);
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn get_settings_serves_the_catalogue_deduced_from_the_binary() {
    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    // A tempdir holding the fake binary, wired as the harness probe PATH BEFORE the
    // daemon boots (so the boot probe and every settings fetch resolve it here).
    let bindir = tempfile::tempdir().unwrap();
    write_fake_binary(bindir.path(), "probe-harness");
    // SAFETY: set once, at the top of this dedicated single-test binary, before any
    // probe reads the `OnceLock`-cached probe path.
    unsafe {
        std::env::set_var("PDO_HARNESS_PROBE_PATH", bindir.path());
    }

    // The daemon's home carries a descriptor declaring the fake harness. `spawn_with
    // _home_override` roots `sandbox_home_roots` at the tempdir, which is where the
    // registry (and thus the settings view) reads `descriptors.yaml`.
    let daemon = TestDaemon::spawn_with_home_override(|home: &std::path::Path| {
        let dir = home.join(".pdo").join("harnesses");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("descriptors.yaml"),
            "harnesses:\n  probe-harness:\n    binary: probe-harness\n    launch: [\"exec\", \"probe-harness\", \"{prompt}\"]\n",
        )?;
        Ok(())
    }, None)
    .await
    .unwrap();

    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let harnesses = settings["harness_descriptors"]["harnesses"]
        .as_array()
        .expect("harness list present");
    let probe = harnesses
        .iter()
        .find(|h| h["name"] == "probe-harness")
        .expect("the declared harness resolves and is listed");

    // Installed (its binary is on the probe PATH), tagged as a disk descriptor.
    assert_eq!(probe["installed"], true, "the fake binary resolves on PATH");
    assert_eq!(probe["source"], "descriptor");

    // The catalogue is DEDUCED from the binary's `--help` — models and effort stops
    // both parsed, verbatim.
    let models: Vec<String> = probe["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(models, vec!["gpt-5", "gpt-5-codex", "o4-mini"]);
    let efforts: Vec<String> = probe["efforts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(efforts, vec!["min", "low", "medium", "high", "max"]);

    // The served effort-axis fact is true (the binary enumerates stops) — the
    // client greys off THIS, not a hard-coded map.
    assert_eq!(probe["has_effort"], true);

    // The probed version rides along for the picker's provenance line.
    assert_eq!(probe["version"], "probe-harness 1.402");
}
