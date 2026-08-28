//! #629 / ADR-0056 end-to-end: the **copilot** catalogue is deduced from the installed
//! copilot binary, from the sources that binary actually enumerates in.
//!
//! #616 read only `--help`. copilot 1.0.80 describes `--model` in prose there, so the
//! reader saw nothing and copilot was served the "no catalogue" fallback while a
//! catalogue existed — the bug this ticket closes. The binary enumerates its ids in
//! its generated completion script (`copilot completion bash`) and, in prose, in its
//! settings topic (`copilot help config`).
//!
//! Layer-3, through the real HTTP surface, against a fake binary **named `copilot`**
//! on the probe `PATH`: the builtin copilot descriptor resolves to it, so what this
//! test reads off `GET /settings` is what the inspector's model picker renders for a
//! node pinned on `copilot`.
//!
//! ONE test in its own binary: it sets the process-global `PDO_HARNESS_PROBE_PATH`
//! (a `OnceLock`) and `PDO_CATALOGUE_VERSION_TTL_MS`, which two concurrent tests
//! could not share safely.

use crate::common::TestDaemon;

/// The completion script copilot generates: a `case` on the previous word, one arm
/// per value-taking flag, choices in a `compgen -W` list. Verbatim shape, abridged
/// list. Deliberately carries `auto` — copilot's automatic selector, which the
/// settings topic does not list — so the assertions can tell the two sources apart.
fn completion_script(models: &str) -> String {
    format!(
        r#"    case "$prev" in
        --model)
            COMPREPLY=( $(compgen -W "{models}" -- "$cur") )
            return 0
            ;;
        --effort|--reasoning-effort)
            COMPREPLY=( $(compgen -W "none minimal low medium high xhigh max" -- "$cur") )
            return 0
            ;;
    esac
"#
    )
}

/// The settings topic copilot prints: keys in backticks, allowed values bulleted and
/// quoted underneath. The lower-preference source; it must lose to the script above.
const HELP_CONFIG: &str = "Configuration Settings:

  `model`: AI model to use for Copilot CLI; can be changed with /model command.
    - \"from-help-config\"

  `contextTier`: context window tier.
";

/// `--help`, which for copilot 1.0.80 enumerates the effort stops but not the models.
/// Its `Commands:` block is what licenses the probe to ask for the two richer sources
/// at all (ADR-0056): a binary that declares neither is never asked for them.
const HELP: &str = "Options:
  --model <model>   Set the AI model to use (use 'auto' to let Copilot pick)
  --effort <level>  Set the reasoning effort level (choices: \"none\", \"max\")

Commands:
  completion <shell>   Generate a shell completion script
  help [topic]         Display help information
";

/// Write an executable fake `copilot` that answers `--version` and each catalogue
/// source. `printf` is a `/bin/sh` builtin, so it runs even though the probe restricts
/// `PATH` to this dir. Overwrites in place, which is how the test simulates the
/// package manager auto-updating the binary under a running daemon.
#[cfg(unix)]
fn write_fake_copilot(dir: &std::path::Path, version: &str, models: &str) {
    use std::os::unix::fs::PermissionsExt;
    let arm =
        |argv: &str, out: &str| format!("  '{argv}') printf '%s' {};;\n", sh_single_quote(out));
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n  '--version') printf '%s\\n' {};\n    ;;\n{}{}{}  *) printf 'error: unknown command\\n' >&2; exit 1;;\nesac\n",
        sh_single_quote(version),
        arm("completion bash", &completion_script(models)),
        arm("help config", HELP_CONFIG),
        arm("--help", HELP),
    );
    let bin = dir.join("copilot");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Single-quote `s` for `/bin/sh`, closing and reopening around embedded quotes.
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn served_models(settings: &serde_json::Value, harness: &str) -> Vec<String> {
    settings["harness_descriptors"]["harnesses"]
        .as_array()
        .expect("harness list present")
        .iter()
        .find(|h| h["name"] == harness)
        .unwrap_or_else(|| panic!("{harness} is listed"))["models"]
        .as_array()
        .expect("models array served")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
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

#[cfg(unix)]
#[tokio::test]
async fn copilots_catalogue_is_deduced_from_its_binary_and_re_read_when_it_updates() {
    let _probe_env = crate::HARNESS_PROBE_ENV_LOCK.lock().await;
    let bindir = tempfile::tempdir().unwrap();
    write_fake_copilot(
        bindir.path(),
        "GitHub Copilot CLI 1.0.80.",
        "auto claude-opus-5 gpt-5.5 kimi-k2.7-code",
    );
    // SAFETY: set once, at the top of this dedicated single-test binary, before any
    // probe reads the `OnceLock`-cached probe path or the TTL.
    unsafe {
        std::env::set_var("PDO_HARNESS_PROBE_PATH", bindir.path());
        // The production window is a minute (ADR-0053 §3). Collapse it so the
        // re-probe-on-version-change contract is provable in milliseconds.
        std::env::set_var(pdo_daemon::CATALOGUE_VERSION_TTL_MS_ENV, "1");
    }

    // No descriptor to seed: `copilot` is a **builtin** harness, and its descriptor
    // names the binary `copilot` — which now resolves to the fake on the probe PATH.
    let daemon = TestDaemon::spawn_with_home_override(|_home| Ok(()), None)
        .await
        .unwrap();

    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // AC #1 / #6: copilot is served a REAL catalogue, deduced from the binary — no
    // longer routed to the "no catalogue" free-text fallback.
    let copilot = served_harness(&settings, "copilot");
    assert_eq!(
        copilot["installed"], true,
        "the fake binary resolves on PATH"
    );
    assert_eq!(
        served_models(&settings, "copilot"),
        vec!["auto", "claude-opus-5", "gpt-5.5", "kimi-k2.7-code"],
        "the ids come from the binary's own enumeration"
    );

    // AC #2 / ADR-0056: the machine-generated completion script outranks the settings
    // prose. Both were offered by the fake; only the script's answer may be served.
    assert!(
        !served_models(&settings, "copilot")
            .iter()
            .any(|m| m == "from-help-config"),
        "the generated source wins over the prose one"
    );

    // The effort axis and the probed version ride along, as #616 established.
    assert_eq!(copilot["has_effort"], true);
    assert_eq!(
        copilot["efforts"].as_array().unwrap().len(),
        7,
        "copilot's seven effort stops"
    );
    assert_eq!(copilot["version"], "GitHub Copilot CLI 1.0.80.");

    // AC #5: the binary auto-updates under the running daemon and its list changes.
    // No restart, no manual re-probe — the next read past the version window follows.
    write_fake_copilot(
        bindir.path(),
        "GitHub Copilot CLI 1.1.0.",
        "auto claude-opus-6 gpt-6",
    );
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let settings: serde_json::Value = reqwest::get(format!("{}/settings", daemon.url()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        served_models(&settings, "copilot"),
        vec!["auto", "claude-opus-6", "gpt-6"],
        "a changed version invalidates the cached catalogue and re-probes"
    );
    assert_eq!(
        served_harness(&settings, "copilot")["version"],
        "GitHub Copilot CLI 1.1.0."
    );
}
