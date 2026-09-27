//! The binary probe (#616, #629, ADR-0053, ADR-0056): runs an installed
//! harness for its version and its offered catalogue. The one adapter of the
//! catalogue that executes a harness; the parsers it feeds are pure
//! (`domain::catalogue`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::adapters::binary_path::{binary_available, harness_probe_path};
use crate::adapters::rpc_probe::probe_rpc_model_efforts;
use crate::application::ports::BinaryProbe;
use crate::application::read_catalogue::CatalogueReader;
use crate::domain::catalogue::Catalogue;

/// The [`BinaryProbe`] of the daemon: the harness probe `PATH` (ADR-0055), the
/// real binaries.
#[derive(Debug, Clone, Copy, Default)]
pub struct InstalledBinaries;

impl BinaryProbe for InstalledBinaries {
    fn installed(&self, binary: &str) -> bool {
        binary_available(binary)
    }

    fn version(&self, binary: &str) -> Option<String> {
        probe_version(binary)
    }

    fn catalogue(&self, binary: &str) -> Catalogue {
        probe_catalogue(binary)
    }
}

/// How often a harness binary's **version** is re-checked behind a `/settings`
/// fetch. Long enough that a burst of settings reads pays at most one `--version`
/// subprocess per harness; short enough that an auto-update is noticed within the
/// minute, with no restart.
const CATALOGUE_VERSION_TTL: Duration = Duration::from_secs(60);

/// Env var that overrides [`CATALOGUE_VERSION_TTL`], in milliseconds. An unparseable
/// value is ignored — a typo must not disable the cache.
pub const CATALOGUE_VERSION_TTL_MS_ENV: &str = "PDO_CATALOGUE_VERSION_TTL_MS";

/// The effective version re-check window. Read per call, NOT cached in a
/// `OnceLock`, so a test setting the var after boot still sees it.
pub fn catalogue_version_ttl() -> Duration {
    std::env::var(CATALOGUE_VERSION_TTL_MS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(CATALOGUE_VERSION_TTL)
}

/// The daemon's catalogue reader: the installed binaries, behind the version
/// cache whose window [`catalogue_version_ttl`] reads per call.
pub fn installed_catalogue_reader() -> CatalogueReader {
    CatalogueReader::new(Arc::new(InstalledBinaries), catalogue_version_ttl)
}

/// How long a catalogue / version probe is allowed to run before it is killed and
/// treated as "no answer" (#616). `--help` / `--version` exit immediately on every
/// harness in play; the cap is a defence against a binary that blocks (a broken
/// install, a prompt on stdin), so a probe can never wedge the boot task or the
/// `/settings` response.
pub(crate) const CATALOGUE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The version string of `binary`, read by running `<binary> --version` on the
/// harness probe `PATH` (#616, ADR-0053 §3). `None` when the binary can't be run,
/// times out, or prints nothing. This is the **freshness key** of the catalogue
/// cache: a changed version invalidates the cached catalogue and re-probes, so an
/// auto-updating binary is followed without a daemon restart.
///
/// UNLIKE [`binary_available`](crate::adapters::binary_path::binary_available), this **executes** the binary — deliberately, and
/// only ever off the resident hot path: at daemon boot, and on a throttled version
/// re-check behind the `/settings` fetch. `--version` is non-interactive and exits
/// at once; the timeout is the backstop.
pub fn probe_version(binary: &str) -> Option<String> {
    probe_version_on(binary, &harness_probe_path())
}

/// The offered catalogue of `binary`, read from the harness probe `PATH` (#616/#629,
/// ADR-0053 §1, ADR-0056). A binary that can't be run, times out, or enumerates
/// nothing anywhere yields [`Catalogue::default`] — the free-text
/// fallback. Executes the binary; see [`probe_version`] for why that is safe here and
/// not in [`binary_available`](crate::adapters::binary_path::binary_available).
pub fn probe_catalogue(binary: &str) -> crate::domain::catalogue::Catalogue {
    probe_catalogue_on(binary, &harness_probe_path())
}

/// [`probe_version`] with an explicit `PATH` — the testable core (a test points it
/// at a tempdir holding a fake binary, no ambient shell).
pub fn probe_version_on(binary: &str, path: &str) -> Option<String> {
    let out = run_probe(binary, &["--version"], path)?;
    out.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// [`probe_catalogue`] with an explicit `PATH` — the testable core, and where #629's,
/// #705's and #798's five sources (ADR-0056) are actually run.
///
/// `--help` is run **first, always**: it is universal, it is one of the three sources,
/// and — the reason it leads — it is where a CLI **declares its subcommands**. Only the
/// richer sources a binary advertises are then run. That gate is not politeness, it is
/// the correctness of the whole ladder. Measured: `claude` has no `completion` and no
/// `help` subcommand, and treats either as a *prompt* — `claude completion bash` opens
/// a session and idles until the probe's timeout kills it. Walking the ladder blind
/// would spend two five-second timeouts on every claude re-probe, one of them inside a
/// `/settings` response.
///
/// The **preference** order of ADR-0056 is independent of the run order: an answer from
/// the generated completion script outranks one from the settings prose, which outranks
/// `--help`'s own — per axis, so copilot's models can come from one source and its
/// effort stops from another. `--help` folds in last precisely because it is the
/// lowest-preference source, not because it ran first.
///
/// Cost, measured on the first-party harnesses: `claude` one subprocess (as before
/// #629), `copilot` two, `opencode` two, `pi` two (`--help` then `--list-models`)
/// plus — only for a binary that declares the RPC mode — one bounded interactive
/// session (#798, [`probe_rpc_model_efforts`]) that answers the per-model effort
/// axis. Never a hang, never an error: every exchange is deadline-bounded and a
/// failure is an empty axis, not an error. A binary
/// that answers no `--help` at all yields the free-text fallback without any subcommand
/// being guessed at.
pub fn probe_catalogue_on(binary: &str, path: &str) -> crate::domain::catalogue::Catalogue {
    use crate::domain::catalogue as cat;
    let Some(help) = run_probe(binary, &["--help"], path) else {
        return cat::Catalogue::default();
    };
    let mut catalogue = cat::Catalogue::default();

    // Preference 1: the generated completion script.
    if cat::advertises_subcommand(&help, "completion") {
        if let Some(script) = run_probe(binary, &["completion", "bash"], path) {
            catalogue.fill_missing_from(cat::parse_completion_script(&script));
        }
    }
    // Preference 2 (#705, ADR-0056 §1 as amended by #702): the generated model table,
    // if the binary declares `--list-models` and the model axis is still unanswered.
    // Gated on the declaration like the subcommands: `claude` has no such flag, and an
    // unknown flag must never be guessed at inside a `/settings` response.
    if !catalogue.is_complete() && cat::advertises_flag(&help, "--list-models") {
        if let Some(table) = run_probe(binary, &["--list-models"], path) {
            catalogue.fill_missing_from(cat::parse_list_models(&table));
        }
    }
    // Preference 3: the settings help topic, if an axis is still unanswered.
    if !catalogue.is_complete() && cat::advertises_subcommand(&help, "help") {
        if let Some(topic) = run_probe(binary, &["help", "config"], path) {
            catalogue.fill_missing_from(cat::parse_settings_prose(&topic));
        }
    }
    // Preference 4: what `--help` itself enumerates.
    catalogue.fill_missing_from(cat::parse_help(&help));
    // Preference 5 (#798): the per-model effort table, read by driving the binary's
    // RPC mode. Its own axis — no other source answers it, so `is_complete` does
    // not gate it: models and global efforts can both be answered while the
    // per-model table is still worth one bounded session. The gate is the same
    // load-bearing discipline as the subcommand/flag ones: only a binary whose
    // `--help` declares `--mode rpc` AND every flag the session passes is ever
    // driven — `claude` would read the argv as a prompt, and the session would
    // idle to the deadline inside a `/settings` response.
    if catalogue.model_efforts.is_empty() && cat::rpc_probe_allowed(&help) {
        catalogue.model_efforts = probe_rpc_model_efforts(binary, path);
    }
    catalogue
}

/// Run `<binary> <args>` with `PATH=path`, capturing stdout+stderr (a CLI may print
/// its help to either), bounded by [`CATALOGUE_PROBE_TIMEOUT`]. Returns `None` when
/// the binary can't be spawned, times out, or exits without output. Pure w.r.t. the
/// daemon's environment — the `PATH` is injected, so this is unit-testable against a
/// fake binary.
///
/// The timeout is a poll-and-kill loop rather than a blocking `output()` so a
/// wedged child (blocked on stdin, a broken install) is reaped instead of hanging
/// the caller's `spawn_blocking` worker forever.
///
/// Both pipes are **drained on their own threads while the child runs** (#629). The
/// obvious shape — wait for exit, then read — deadlocks on any source that outprints
/// the OS pipe buffer: the child blocks in `write`, never exits, and the probe reports
/// a timeout for a binary that answered perfectly. Measured: copilot's completion
/// script is 71 KB against a 64 KB Linux pipe buffer, so the source ADR-0056 prefers
/// is exactly the one that hit it.
fn run_probe(binary: &str, args: &[&str], path: &str) -> Option<String> {
    use std::io::Read;
    if binary.is_empty() {
        return None;
    }
    let mut child = std::process::Command::new(binary)
        .args(args)
        .env("PATH", path)
        // A probe must never inherit an interactive stdin; if a binary asks, it
        // gets EOF and exits rather than blocking.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    fn drain<R: Read + Send + 'static>(
        pipe: Option<R>,
    ) -> Option<std::thread::JoinHandle<Vec<u8>>> {
        pipe.map(|mut p| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = p.read_to_end(&mut buf);
                buf
            })
        })
    }
    let out_reader = drain(child.stdout.take());
    let err_reader = drain(child.stderr.take());

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > CATALOGUE_PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Leave the readers detached rather than joining them: a killed
                    // child whose grandchild still holds the pipe would never EOF, and
                    // a probe must not be able to block on that. Partial output is not
                    // a catalogue anyway — a timeout is "no answer".
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
    // The child exited, so both pipes are at EOF and the readers join immediately.
    let joined = |r: Option<std::thread::JoinHandle<Vec<u8>>>| {
        r.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    let (stdout, stderr) = (joined(out_reader), joined(err_reader));

    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if !stderr.is_empty() {
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&stderr));
    }
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #616: write an executable fake binary into `dir` that echoes `stdout` on
    /// `--help` and `version` on `--version`. Returns the dir's `PATH` string.
    #[cfg(unix)]
    fn fake_harness_binary(dir: &std::path::Path, name: &str, help: &str, version: &str) -> String {
        fake_harness_binary_with(dir, name, version, &[("--help", help)])
    }

    /// #629: write an executable fake binary that answers `--version` and any set of
    /// `(argv-head, stdout)` pairs — one per catalogue source under test. Returns the
    /// dir's `PATH` string. An argv the fixture does not declare falls through to a
    /// usage error on stderr, exactly as a real CLI answers a subcommand it lacks.
    ///
    /// Self-contained: `printf` is a `/bin/sh` builtin, so the script runs even though
    /// `run_probe` sets `PATH` to just this dir (no `cat`/`echo` binary). That
    /// restricted PATH mirrors nothing in production — there the probe PATH is a full
    /// union (ADR-0055) — it only keeps the fixture honest.
    #[cfg(unix)]
    fn fake_harness_binary_with(
        dir: &std::path::Path,
        name: &str,
        version: &str,
        sources: &[(&str, &str)],
    ) -> String {
        use std::os::unix::fs::PermissionsExt;
        // Match on the whole argv (`"$*"`), so a two-word source (`completion bash`,
        // `help config`) is as expressible as a flag.
        let arms: String = sources
            .iter()
            .map(|(argv, out)| format!("  '{argv}') printf '%s' '{out}';;\n"))
            .collect();
        let script = format!(
            "#!/bin/sh\ncase \"$*\" in\n  '--version') printf '%s\\n' '{version}';;\n{arms}  *) printf 'error: unknown command\\n' >&2; exit 1;;\nesac\n"
        );
        let bin = dir.join(name);
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir.to_string_lossy().into_owned()
    }

    /// #616, ADR-0053 §1/§3: running the resolved binary reads its version and parses
    /// its offered catalogue. Executed against a fake binary on an injected `PATH`,
    /// so the test is deterministic and touches no real harness.
    #[cfg(unix)]
    #[test]
    fn probe_reads_version_and_catalogue_from_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let help =
            "  --model <m>  [gpt-5|gpt-5-codex|o4-mini]\n  --effort <e>  One of: low, medium, high";
        let path = fake_harness_binary(dir.path(), "fake-harness", help, "fake-harness 1.402");

        assert_eq!(
            probe_version_on("fake-harness", &path).as_deref(),
            Some("fake-harness 1.402")
        );
        let cat = probe_catalogue_on("fake-harness", &path);
        assert_eq!(cat.models, vec!["gpt-5", "gpt-5-codex", "o4-mini"]);
        assert_eq!(cat.efforts, vec!["low", "medium", "high"]);
    }

    /// The `Commands:` block a fake binary prints so the probe is allowed to ask it
    /// for the richer sources (#629, ADR-0056). Without it, the gate — correctly —
    /// never runs `completion` / `help`.
    const DECLARES_BOTH_SUBCOMMANDS: &str =
        "Commands:\n  completion <shell>  Generate a shell completion script\n  help [topic]  Display help information\n";

    /// #629, ADR-0056: the completion script is preferred over the settings topic and
    /// over `--help`. The fixture makes the three sources disagree on purpose — only
    /// the completion script's answer may survive.
    #[cfg(unix)]
    #[test]
    fn the_generated_completion_script_outranks_the_prose_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_harness_binary_with(
            dir.path(),
            "ladder-harness",
            "ladder-harness 2.0",
            &[
                (
                    "completion bash",
                    "        --model)\n            COMPREPLY=( $(compgen -W \"auto from-completion\" -- \"$cur\") )\n            ;;\n        --effort|--reasoning-effort)\n            COMPREPLY=( $(compgen -W \"low max\" -- \"$cur\") )\n            ;;\n",
                ),
                ("help config", "  `model`: the model.\n    - \"from-settings\"\n"),
                (
                    "--help",
                    &format!(
                        "  --model <m> [from-help|other]\n  --effort <e> One of: a, b\n{DECLARES_BOTH_SUBCOMMANDS}"
                    ),
                ),
            ],
        );
        let cat = probe_catalogue_on("ladder-harness", &path);
        assert_eq!(cat.models, vec!["auto", "from-completion"]);
        assert_eq!(cat.efforts, vec!["low", "max"]);
    }

    /// #629: a source larger than the OS pipe buffer is read in full, not timed out.
    /// Measured on the real binary, `copilot completion bash` prints 71 KB against a
    /// 64 KB pipe: a probe that waits for exit before reading blocks the child in
    /// `write` and reports a timeout for the source ADR-0056 prefers.
    #[cfg(unix)]
    #[test]
    fn a_source_bigger_than_the_pipe_buffer_is_read_whole() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("chatty-harness");
        // ~200 KB of padding, then the enumeration — so the list only parses if the
        // whole stream was drained. `yes`-style loops need an external binary, and the
        // probe's PATH holds only this dir; a `while` over a doubling string is a
        // builtin-only way to print a lot.
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in\n  '--version') printf 'chatty 1.0\\n';;\n  '--help')\n    pad=0123456789abcdef\n    i=0\n    while [ $i -lt 14 ]; do pad=\"$pad$pad\"; i=$((i+1)); done\n    printf '%s\\n' \"$pad\"\n    printf '  --model <m> [big|list]\\n  --effort <e> One of: low, high\\n'\n    ;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        let raw =
            run_probe("chatty-harness", &["--help"], &path).expect("a big answer is an answer");
        assert!(raw.len() > 200_000, "the fixture must exceed a pipe buffer");
        let cat = probe_catalogue_on("chatty-harness", &path);
        assert_eq!(cat.models, vec!["big", "list"]);
        assert_eq!(cat.efforts, vec!["low", "high"]);
    }

    /// #629, ADR-0056: a binary that declares neither subcommand is never asked for
    /// them. The fixture makes both **hang** — the measured `claude` hazard, where
    /// `claude completion bash` is read as a prompt and idles to the probe timeout.
    /// The probe must come back with `--help`'s own answer, promptly.
    #[cfg(unix)]
    #[test]
    fn an_unadvertised_subcommand_is_never_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // Its own script rather than `fake_harness_binary_with`: the point of the
        // fixture is the catch-all, which must **hang** instead of erroring out. Its
        // `--help` declares no `Commands:` block, so nothing is advertised.
        let bin = dir.path().join("prompt-harness");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in\n  '--version') printf 'prompt-harness 3.0\\n';;\n  '--help') printf '%s' 'Usage: prompt-harness [prompt]\n  --model <m> [only|from-help]\n  --effort <e> One of: low, high\n';;\n  *) sleep 30;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        let started = Instant::now();
        let cat = probe_catalogue_on("prompt-harness", &path);
        assert!(
            started.elapsed() < CATALOGUE_PROBE_TIMEOUT,
            "an unadvertised subcommand must not be run at all, let alone timed out"
        );
        assert_eq!(cat.models, vec!["only", "from-help"]);
        assert_eq!(cat.efforts, vec!["low", "high"]);
    }

    /// #629: the copilot shape as measured on 1.0.80 minus its completion script (an
    /// older release, or a generator that stops emitting choices) — models from
    /// `help config`, efforts from `--help`, each axis owned by the first source that
    /// answers for it.
    #[cfg(unix)]
    #[test]
    fn each_axis_falls_through_to_the_next_source_independently() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_harness_binary_with(
            dir.path(),
            "split-harness",
            "split-harness 1.0.80",
            &[
                (
                    "help config",
                    "  `model`: AI model to use.\n    - \"claude-opus-5\"\n    - \"gpt-5.5\"\n\n  `contextTier`: tier.\n",
                ),
                (
                    "--help",
                    &format!(
                        "  --model <model>  Set the AI model to use (use 'auto')\n  --effort <level> Set the effort (choices: \"none\", \"max\")\n{DECLARES_BOTH_SUBCOMMANDS}"
                    ),
                ),
            ],
        );
        let cat = probe_catalogue_on("split-harness", &path);
        assert_eq!(cat.models, vec!["claude-opus-5", "gpt-5.5"]);
        assert_eq!(cat.efforts, vec!["none", "max"]);
    }

    /// #629: a binary that enumerates nothing on any of the three sources still yields
    /// an empty catalogue — the free-text fallback — and never an error. This is the
    /// measured `opencode` shape: a bare `provider/model` placeholder, no completion
    /// choices, no settings topic.
    #[cfg(unix)]
    #[test]
    fn a_binary_that_enumerates_nowhere_is_still_the_free_text_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_harness_binary_with(
            dir.path(),
            "bare-harness",
            "bare-harness 0.1",
            &[("--help", "  --model <provider/model>  Model to use\n")],
        );
        assert_eq!(
            probe_catalogue_on("bare-harness", &path),
            crate::domain::catalogue::Catalogue::default()
        );
    }

    /// #616: a binary that can't be resolved on the injected `PATH` yields no
    /// version and an empty catalogue — the free-text fallback, never a panic.
    #[test]
    fn probe_of_an_absent_binary_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        assert_eq!(probe_version_on("no-such-harness", &path), None);
        assert_eq!(
            probe_catalogue_on("no-such-harness", &path),
            crate::domain::catalogue::Catalogue::default()
        );
    }
}
