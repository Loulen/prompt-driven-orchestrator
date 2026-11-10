//! #616 / ADR-0053 end-to-end: the daemon **deduces** a harness's model & effort
//! catalogue from the installed binary and **serves** it on `GET /settings`, with
//! the probed version — deduced, never hard-coded. A declared harness whose fake
//! binary prints an enumerated `--help` shows up with its models, its effort stops,
//! `has_effort: true`, and a `version`; the client renders that instead of knowing
//! a catalogue.
//!
//! Layer-3, through the real HTTP surface. The probe `PATH` is process-global, so
//! this test serialises on `HARNESS_PROBE_ENV_LOCK` with the other catalogue tests and
//! installs its self-contained fake binary in the shared, process-wide dir
//! `common::fake_harness_bindir` fixes first on that PATH.
//!
//! « Refresh models » (`POST /settings/harnesses/{name}/refresh`) probes a
//! catalogue again at the same version, answers the new entry, and announces it
//! on `/ws`.

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
    // The process-wide fake-binary dir, wired FIRST on the harness probe PATH before
    // the daemon boots (so the boot probe and every settings fetch resolve it here).
    // Shared with the other catalogue tests and kept alive for the whole process:
    // see `common::fake_harness_bindir` for why a dropped tempdir here broke every
    // session spawned later in this binary.
    let bindir = crate::common::fake_harness_bindir();
    write_fake_binary(&bindir, "probe-harness");

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

/// A fake harness whose `--help` prints `<name>-help.txt` beside it, read with shell
/// builtins only: the test changes what it offers without changing its version.
#[cfg(unix)]
fn write_reading_binary(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let help = dir.join(format!("{name}-help.txt"));
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' '{name} 2.0';;\n  --help) while IFS= read -r line; do printf '%s\\n' \"$line\"; done < '{}';;\nesac\n",
        help.display()
    );
    let bin = dir.join(name);
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    help
}

#[cfg(unix)]
fn offered(models: &[&str]) -> String {
    format!("  --model <m>  [{}]\n", models.join("|"))
}

#[cfg(unix)]
fn models_of(entry: &serde_json::Value) -> Vec<String> {
    entry["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn refresh_models_reprobes_at_the_same_version_and_tells_every_page() {
    use futures_util::StreamExt;

    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    let bindir = crate::common::fake_harness_bindir();
    let help = write_reading_binary(&bindir, "refresh-harness");
    std::fs::write(&help, offered(&["alpha", "beta"])).unwrap();

    let daemon = TestDaemon::spawn_with_home_override(|home: &std::path::Path| {
        let dir = home.join(".pdo").join("harnesses");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("descriptors.yaml"),
            "harnesses:\n  refresh-harness:\n    binary: refresh-harness\n    launch: [\"exec\", \"refresh-harness\", \"{prompt}\"]\n",
        )?;
        Ok(())
    }, None)
    .await
    .unwrap();
    let entry_of = |settings: &serde_json::Value| {
        settings["harness_descriptors"]["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == "refresh-harness")
            .cloned()
            .expect("the declared harness is listed")
    };
    let before = entry_of(&daemon.get_json("/settings").await);
    assert_eq!(models_of(&before), vec!["alpha", "beta"]);
    assert!(before["probed_at"].is_string(), "{before}");

    // The binary now offers another model, at the same version: the page keeps
    // serving the cached catalogue…
    std::fs::write(&help, offered(&["alpha", "beta", "gamma"])).unwrap();
    let cached = entry_of(&daemon.get_json("/settings").await);
    assert_eq!(models_of(&cached), vec!["alpha", "beta"]);

    // …until a refresh probes it again, and every page hears of it.
    let mut ws = daemon.connect_ws().await.unwrap();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/settings/harnesses/refresh-harness/refresh",
            daemon.url()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let refreshed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(models_of(&refreshed), vec!["alpha", "beta", "gamma"]);
    assert_eq!(refreshed["version"], "refresh-harness 2.0");
    assert!(refreshed["probed_at"].as_str() >= before["probed_at"].as_str());
    let after = entry_of(&daemon.get_json("/settings").await);
    assert_eq!(models_of(&after), vec!["alpha", "beta", "gamma"]);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, ws.next())
            .await
            .expect("harness_catalogue_changed never came on /ws");
        let Some(Ok(msg)) = frame else {
            panic!("/ws closed before harness_catalogue_changed");
        };
        let Ok(text) = msg.to_text() else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        if value["type"] == "harness_catalogue_changed" {
            assert_eq!(value["harness"], "refresh-harness");
            break;
        }
    }

    // A harness PDO does not resolve: 404, nothing probed.
    let resp = client
        .post(format!("{}/settings/harnesses/ghost/refresh", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}
