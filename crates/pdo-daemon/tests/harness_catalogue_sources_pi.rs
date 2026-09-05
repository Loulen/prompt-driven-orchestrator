//! #705 / ADR-0056 (as amended by #702) end-to-end: the **pi** catalogue is deduced
//! from the installed pi binary, from the source that binary actually enumerates in —
//! its generated model table (`pi --list-models`) for the models, its `--help` for
//! the `--thinking` levels.
//!
//! pi 0.85.1 declares neither a `completion` nor a `help` subcommand, so the
//! completion-script and settings-topic sources are never run (the fake makes them
//! **hang**, the measured `claude` hazard). It does declare `--list-models`, so that
//! one is. Models are offered as `provider/model`, aliases included, each with its
//! context window; the value stays free-text pass-through.
//!
//! Layer-3, through the real HTTP surface, against a fake binary **named `pi`** on
//! the probe `PATH`: the builtin pi descriptor resolves to it, so what this test
//! reads off `GET /settings` is what the inspector's pickers render for a node pinned
//! on `pi`.
//!
//! The probe `PATH` is process-global (a `OnceLock`), so this test serialises on
//! `HARNESS_PROBE_ENV_LOCK` with the other catalogue tests and installs its fake beside
//! theirs only for its own duration.

use crate::common::TestDaemon;

/// The real `pi --help` of 0.85.1, verbatim — the fixture beside copilot's shapes.
const HELP: &str = include_str!("fixtures/catalogue/pi-0.85.1-help.txt");
/// The real `pi --list-models` of 0.85.1, abridged (alias rows + a few concrete ones).
const LIST_MODELS: &str = include_str!("fixtures/catalogue/pi-0.85.1-list-models.txt");

/// Write an executable fake `pi` that answers `--version`, `--help` and
/// `--list-models`, and **hangs** on anything else — so an unadvertised source
/// (`completion bash`, `help config`) being run would time the probe out visibly.
#[cfg(unix)]
fn write_fake_pi(dir: &std::path::Path, version: &str, list_models: &str) {
    use std::os::unix::fs::PermissionsExt;
    let arm =
        |argv: &str, out: &str| format!("  '{argv}') printf '%s' {};;\n", sh_single_quote(out));
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  '--version') printf '%s\\n' {};\n    ;;\n{}{}  *) sleep 30;;\nesac\n",
        sh_single_quote(version),
        arm("--help", HELP),
        arm("--list-models", list_models),
    );
    let bin = dir.join("pi");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Single-quote `s` for `/bin/sh`, closing and reopening around embedded quotes.
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
async fn pis_catalogue_comes_from_its_model_table_and_its_thinking_line() {
    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    // The probe PATH is a process-global `OnceLock`: the first catalogue test to run
    // fixes it. Install the fake in THAT dir when it is already set (the sibling
    // copilot test's tempdir), else in a fresh one this test sets.
    let bindir: std::path::PathBuf = match std::env::var_os("PDO_HARNESS_PROBE_PATH") {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let dir = std::env::temp_dir().join(format!("pdo-pi-probe-{}", std::process::id()));
            // SAFETY: set under the lock, before any probe reads the `OnceLock`.
            unsafe {
                std::env::set_var("PDO_HARNESS_PROBE_PATH", &dir);
                std::env::set_var(pdo_daemon::CATALOGUE_VERSION_TTL_MS_ENV, "1");
            }
            dir
        }
    };
    // A sibling test may have dropped its tempdir after fixing the PATH: recreate.
    std::fs::create_dir_all(&bindir).unwrap();
    write_fake_pi(&bindir, "0.85.1", LIST_MODELS);

    // No descriptor to seed: `pi` is a **builtin** harness whose binary is `pi` —
    // which now resolves to the fake on the probe PATH.
    let daemon = TestDaemon::spawn_with_home_override(|_home| Ok(()), None)
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // ADR-0056 §1 bis: neither unadvertised subcommand was run — the fake would have
    // hung 30 s on each; the whole settings read must come back well inside that.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "an unadvertised source must never be run: {:?}",
        started.elapsed()
    );

    let pi = served_harness(&settings, "pi");
    assert_eq!(pi["source"], "builtin", "pi is on the embedded floor");
    assert_eq!(pi["installed"], true, "the fake binary resolves on PATH");
    assert_eq!(pi["version"], "0.85.1");

    // AC: models = `provider/model`, aliases included, from `--list-models`.
    let models = strings(&pi["models"]);
    assert_eq!(models[0], "openrouter/~anthropic/claude-fable-latest");
    assert!(models
        .iter()
        .any(|m| m == "openrouter/anthropic/claude-sonnet-4.5"));
    assert_eq!(models.len(), 17, "one offer per table row");
    // …each with its context window, served beside the id.
    assert_eq!(
        pi["model_contexts"]["openrouter/anthropic/claude-opus-4"],
        "200K"
    );
    assert_eq!(
        pi["model_contexts"]["openrouter/~anthropic/claude-sonnet-latest"],
        "1M"
    );

    // AC: efforts = the `--thinking` line of `--help`.
    assert_eq!(
        strings(&pi["efforts"]),
        vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(pi["has_effort"], true);
}
