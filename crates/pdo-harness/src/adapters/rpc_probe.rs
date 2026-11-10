//! The RPC model-effort probe (#798): drives a harness's `--mode rpc` session
//! to read the effort levels each model reports.

use std::time::{Duration, Instant};

/// Overall wall-clock budget for one RPC model-effort probe (#798): the spawn, the
/// model list, and one `set_model` + `get_available_thinking_levels` round trip per
/// model. The version cache makes this a once-per-binary-version cost; the bound
/// makes it never more than that. Generous against a slow machine (the measured
/// exchange over 366 models completes in well under a second), small enough that a
/// wedged binary cannot outlive a settings fetch for long.
const RPC_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Hard cap on how many models one RPC probe interrogates (#798). The measured
/// catalogue (pi 0.85.1 on this host) is 366 rows; the cap keeps a pathological
/// provider list from stretching the session — models past the cap simply stay
/// unknown, and an absent key is the global fallback, so the cost of the cap is a
/// coarser offer, never a wrong one.
const RPC_MODEL_CAP: usize = 512;

/// The per-model effort table of `binary`, read by driving its RPC mode (#798):
/// one session lists the binary's models (`get_available_models`), then a
/// `set_model` + `get_available_thinking_levels` round trip per model — the levels
/// the binary itself reports, never a model-name rule nor a `reasoning` boolean
/// (measured on pi 0.85.1: `openrouter/stealth/union-alpha` reports `[off]` where
/// `openrouter/openai/gpt-5.4` reports five stops). Keys are the `provider/id`
/// composite, the same rendering `--list-models` offers, so they meet the
/// catalogue's model ids without translation.
///
/// The caller gates on [`crate::domain::catalogue::rpc_probe_allowed`]; this
/// function assumes the gate passed. Any failure — spawn, protocol, deadline —
/// yields the table read so far, usually empty: absent keys are *unknown*, and
/// unknown is the global fallback, the path that cannot break (ADR-0053).
/// Executes the binary interactively (stdin/stdout); same safety stance as
/// [`probe_version`] — only ever off the resident hot path, and bounded.
pub fn probe_rpc_model_efforts(
    binary: &str,
    path: &str,
) -> std::collections::BTreeMap<String, Vec<String>> {
    probe_rpc_model_efforts_with(binary, path, RPC_PROBE_TIMEOUT, RPC_MODEL_CAP)
}

/// [`probe_rpc_model_efforts`] with explicit bounds — the testable core, so a test
/// can shrink the deadline and the cap without process-global env (the discipline
/// `catalogue_version_ttl` had to grow an env var to get).
pub fn probe_rpc_model_efforts_with(
    binary: &str,
    path: &str,
    timeout: Duration,
    max_models: usize,
) -> std::collections::BTreeMap<String, Vec<String>> {
    use crate::domain::catalogue as cat;
    use std::sync::mpsc;

    let mut out = std::collections::BTreeMap::new();
    if binary.is_empty() || max_models == 0 {
        return out;
    }
    // The inert-session argv: every token the gate declared, then the mode. stdin
    // is PIPED (the protocol is driven over it — unlike `run_probe`, whose probes
    // get EOF) and stderr is discarded (a chatty binary cannot fill a pipe we
    // never drain and wedge itself).
    let mut child = match std::process::Command::new(binary)
        .args(cat::RPC_HYGIENE_FLAGS)
        .args(["--mode", "rpc"])
        .env("PATH", path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return out,
    };
    let deadline = Instant::now() + timeout;

    // One reader thread owns stdout and feeds lines to a channel, so the exchange
    // loop can time out on a child that stops answering instead of blocking in
    // `read` forever (the live-session sibling of `run_probe`'s drain threads).
    let (tx, rx) = mpsc::sync_channel::<String>(64);
    let stdout = child.stdout.take();
    // Detached on purpose (see the cleanup comment at the bottom).
    std::thread::spawn(move || {
        let Some(pipe) = stdout else { return };
        let mut buf = String::new();
        // Bound total output as well as wall time, including unterminated lines.
        let bounded = std::io::Read::take(pipe, 16 * 1024 * 1024);
        let mut reader = std::io::BufReader::new(bounded);
        loop {
            buf.clear();
            match std::io::BufRead::read_line(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = buf.trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut stdin = child.stdin.take();
    // The model list. All requests are correlated by COMMAND, not by id: the
    // protocol is strictly sequential (each request is sent only after the
    // previous answer was read), so the next response of `command` is
    // unambiguous — and a binary that drops correlation ids still works. Events
    // and unrelated responses are skipped, bounded by the deadline.
    let models_line = rpc_exchange(
        &mut stdin,
        &rx,
        "get_available_models",
        serde_json::json!({ "id": "models", "type": "get_available_models" }),
        deadline,
    );
    let models = models_line
        .as_deref()
        .map(cat::parse_rpc_models)
        .unwrap_or_default();
    for (i, (provider, id)) in models.into_iter().take(max_models).enumerate() {
        if Instant::now() >= deadline {
            break;
        }
        // The catalogue key: `provider/id`, the `--list-models` rendering.
        let key = format!("{provider}/{id}");
        let set = serde_json::json!({
            "id": format!("m{i}"),
            "type": "set_model",
            "provider": provider,
            "modelId": id,
        });
        let Some(line) = rpc_exchange(&mut stdin, &rx, "set_model", set, deadline) else {
            break;
        };
        let accepted = serde_json::from_str::<serde_json::Value>(&line)
            .map(|v| v["success"].as_bool() == Some(true))
            .unwrap_or(false);
        if !accepted {
            // The binary refused the model: the key stays ABSENT = unknown, the
            // global fallback — never guessed, never known-empty.
            continue;
        }
        let levels = serde_json::json!({
            "id": format!("t{i}"),
            "type": "get_available_thinking_levels",
        });
        let Some(line) = rpc_exchange(
            &mut stdin,
            &rx,
            "get_available_thinking_levels",
            levels,
            deadline,
        ) else {
            break;
        };
        if let Some(levels) = cat::parse_rpc_thinking_levels(&line) {
            out.insert(key, levels);
        }
    }

    // Close stdin, then kill and reap: the child's stdout EOFs and the reader
    // thread ends on its own — DETACHED, like `run_probe`'s drain threads: a killed
    // child whose grandchild still holds the pipe (its own spawned `sleep`, say)
    // would never EOF, and a probe must not be able to block the caller on that.
    // A partially-read table is still a table — per-model keys are independent, so
    // what was learned stands.
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    out
}

/// One request/response exchange of the RPC probe: write `request` (a JSONL line),
/// then read lines until one is a response of `command` — skipping events,
/// unrelated responses, and non-JSON noise — or the deadline arrives. `None` on
/// timeout, a closed pipe, or a dead child; the caller degrades per exchange.
fn rpc_exchange(
    stdin: &mut Option<std::process::ChildStdin>,
    rx: &std::sync::mpsc::Receiver<String>,
    command: &str,
    request: serde_json::Value,
    deadline: Instant,
) -> Option<String> {
    use std::io::Write;
    let stdin = stdin.as_mut()?;
    let line = serde_json::to_string(&request).ok()?;
    writeln!(stdin, "{line}").ok()?;
    stdin.flush().ok()?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let line = rx.recv_timeout(deadline - now).ok()?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            // A banner, a warning, a partial line: skip and keep waiting.
            continue;
        };
        if v["type"].as_str() == Some("response") && v["command"].as_str() == Some(command) {
            return Some(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::binary_probe::{probe_catalogue_on, CATALOGUE_PROBE_TIMEOUT};

    // -----------------------------------------------------------------------
    // #798: the RPC model-effort probe. Fake binaries speak the tiny slice of
    // the pi RPC protocol the probe drives; every failure shape is exercised
    // against an injected deadline so no test can hang or race the clock.
    // -----------------------------------------------------------------------

    /// A fake `pi`-shaped harness: `--help` passes the RPC gate, `--mode rpc`
    /// speaks JSONL. Three models on purpose — one full level set (`gpt-5.4`),
    /// one that reports only `off` (`union-alpha`), one spare (`third/model`) for
    /// the cap test. `refuse` is a substring; a `set_model` naming it is answered
    /// with `success:false` and the session keeps serving.
    #[cfg(unix)]
    fn fake_rpc_harness(dir: &std::path::Path, name: &str, refuse: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let refuse_arm = if refuse.is_empty() {
            String::new()
        } else {
            // Built by pushes, not format!: the arm carries literal braces, and
            // doubling every one for the macro is how a fixture rots.
            let mut arm = String::from("            *");
            arm.push_str(refuse);
            arm.push_str("*)\n");
            arm.push_str(
                "                printf '%s\\n' '{\"id\":\"s\",\"type\":\"response\",\"command\":\"set_model\",\"success\":false,\"error\":\"refused\"}'\n",
            );
            arm.push_str("                ;;\n");
            arm
        };
        let script = r#"#!/bin/sh
case "$*" in
  '--version') printf '%s\n' 'rpc-harness 1.0';;
  '--help')
    printf '%s\n' \
      'Options:' \
      '  --mode <mode>                  Output mode: text (default), json, or rpc' \
      '  --no-session                   ephemeral session' \
      '  --no-extensions, -ne           no extension discovery' \
      '  --no-skills, -ns               no skills discovery' \
      '  --no-prompt-templates, -np     no prompt templates' \
      '  --no-themes                    no themes' \
      '  --no-context-files, -nc        no context files' \
      '  --offline                      no network operations' \
      '  --no-approve, -na              no project-local trust'
    ;;
  *'--mode rpc'*)
    MODEL=other
    while IFS= read -r line; do
      case "$line" in
        *get_available_models*)
          printf '%s\n' '{"id":"models","type":"response","command":"get_available_models","success":true,"data":{"models":[{"id":"openai/gpt-5.4","name":"GPT 5.4","provider":"openrouter","reasoning":true},{"id":"stealth/union-alpha","name":"Union","provider":"openrouter","reasoning":false},{"id":"third/model","name":"Third","provider":"openrouter","reasoning":true}]}}'
          ;;
        *set_model*)
          case "$line" in
__REFUSE_ARM__            *union-alpha*)
              MODEL=union-alpha
              printf '%s\n' '{"id":"s","type":"response","command":"set_model","success":true,"data":{}}'
              ;;
            *)
              MODEL=other
              printf '%s\n' '{"id":"s","type":"response","command":"set_model","success":true,"data":{}}'
              ;;
          esac
          ;;
        *get_available_thinking_levels*)
          if [ "$MODEL" = union-alpha ]; then
            printf '%s\n' '{"id":"t","type":"response","command":"get_available_thinking_levels","success":true,"data":{"levels":["off"]}}'
          else
            printf '%s\n' '{"id":"t","type":"response","command":"get_available_thinking_levels","success":true,"data":{"levels":["off","low","medium","high","xhigh"]}}'
          fi
          ;;
      esac
    done
    ;;
  *)
    printf 'error: unknown command\n' >&2
    exit 1
    ;;
esac
"#
        .replace("__REFUSE_ARM__", &refuse_arm);
        let bin = dir.join(name);
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir.to_string_lossy().into_owned()
    }

    /// #798, the issue's repro end to end: a binary that declares the RPC mode
    /// yields a per-model effort table keyed `provider/id` — the exact levels the
    /// fake reports, nothing inferred. `union-alpha`'s `[off]` is a KNOWN offer of
    /// one level; it must not collapse into the unknown bucket.
    #[cfg(unix)]
    #[test]
    fn the_rpc_session_yields_each_models_verbatim_levels() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_rpc_harness(dir.path(), "rpc-harness", "");
        let cat = probe_catalogue_on("rpc-harness", &path);
        let gpt54 = cat
            .model_efforts
            .get("openrouter/openai/gpt-5.4")
            .expect("gpt-5.4 in the table");
        assert_eq!(
            gpt54.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["off", "low", "medium", "high", "xhigh"],
        );
        let union = cat
            .model_efforts
            .get("openrouter/stealth/union-alpha")
            .expect("union-alpha in the table");
        assert_eq!(
            union.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["off"],
            "[off] is a known per-model offer, not an unknown"
        );
        assert_eq!(cat.model_efforts.len(), 3);
        // The keys meet the `--list-models` rendering without translation; and the
        // session answers ONLY the per-model axis — the global axes keep their
        // sources' answers (here: none, the declared absence).
        assert!(cat.models.is_empty());
        assert!(cat.efforts.is_empty());
    }

    /// #798: a model the binary REFUSES at `set_model` stays absent = unknown (the
    /// global fallback), while the models it accepted keep their verbatim levels.
    /// Never guessed, never known-empty.
    #[cfg(unix)]
    #[test]
    fn a_refused_model_stays_unknown_the_others_keep_their_levels() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_rpc_harness(dir.path(), "refusing-harness", "union-alpha");
        let cat = probe_catalogue_on("refusing-harness", &path);
        assert!(
            !cat.model_efforts
                .contains_key("openrouter/stealth/union-alpha"),
            "the refused model is unknown: {:?}",
            cat.model_efforts
        );
        assert_eq!(
            cat.model_efforts
                .get("openrouter/openai/gpt-5.4")
                .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["off", "low", "medium", "high", "xhigh"]),
        );
        assert_eq!(
            cat.model_efforts
                .get("openrouter/third/model")
                .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["off", "low", "medium", "high", "xhigh"]),
            "the probe keeps going after a refusal"
        );
    }

    /// #798: the cap. Only the first `max_models` models are interrogated; the
    /// rest stay unknown — a coarser offer, never a wrong one.
    #[cfg(unix)]
    #[test]
    fn the_rpc_probe_is_capped_to_max_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = fake_rpc_harness(dir.path(), "capped-harness", "");
        let map = probe_rpc_model_efforts_with("capped-harness", &path, Duration::from_secs(5), 2);
        assert_eq!(map.len(), 2, "exactly the first two offers");
        assert!(map.contains_key("openrouter/openai/gpt-5.4"));
        assert!(map.contains_key("openrouter/stealth/union-alpha"));
        assert!(!map.contains_key("openrouter/third/model"));
    }

    /// #798: a binary that accepts the session and never answers is bounded by the
    /// injected deadline and yields an empty table — the global fallback, not a
    /// hung `/settings`. The fixture blocks on stdin for ever (builtin-only, see
    /// its comment); the probe must not wait for it.
    #[cfg(unix)]
    #[test]
    fn an_rpc_session_that_never_answers_is_bounded_and_empty() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("stuck-harness");
        // The hang is a builtin-only stdin read (an external `sleep` would not
        // resolve on the probe's restricted PATH, and the fixture would exit at
        // once instead of exercising the deadline).
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in\n  *'--mode rpc'*) while IFS= read -r x; do :; done;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        let started = Instant::now();
        let map =
            probe_rpc_model_efforts_with("stuck-harness", &path, Duration::from_millis(400), 10);
        assert!(map.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "bounded by the injected deadline, not the fixture's endless read"
        );
    }

    /// #798: malformed output — banners, non-JSON noise, an early exit — is an
    /// empty table, never an error and never a half-parsed model.
    #[cfg(unix)]
    #[test]
    fn garbage_from_an_rpc_session_is_an_empty_table() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("garbage-harness");
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in\n  *'--mode rpc'*) printf 'hello\\nnot json\\n{}\\n'; exit 0;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let map =
            probe_rpc_model_efforts_with("garbage-harness", &path, Duration::from_secs(2), 10);
        assert!(map.is_empty());
    }

    /// #798, the gate end to end: a binary whose `--help` does NOT declare the RPC
    /// mode is never driven, even though its rpc mode would hang — the catalogue
    /// comes back promptly from `--help` alone, the per-model axis empty (unknown,
    /// the pre-#798 behaviour). The measured `claude` hazard, mirrored.
    #[cfg(unix)]
    #[test]
    fn the_rpc_probe_only_runs_for_a_binary_that_declares_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("unannounced-harness");
        // The catch-all hangs on a builtin-only stdin read — the measured `claude`
        // hazard (an unannounced argv is read as a prompt). An external `sleep`
        // would not resolve on the probe's restricted PATH, so the fixture must not
        // rely on one to hang.
        std::fs::write(
            &bin,
            "#!/bin/sh\ncase \"$*\" in\n  '--help') printf '%s\\n' '  --model <m>  [from-help|other]' '  --effort <e> One of: low, high';;\n  *) while IFS= read -r x; do :; done;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().to_string_lossy().into_owned();

        let started = Instant::now();
        let cat = probe_catalogue_on("unannounced-harness", &path);
        assert!(
            started.elapsed() < CATALOGUE_PROBE_TIMEOUT,
            "the gate refused the session: nothing hung"
        );
        assert_eq!(cat.models, vec!["from-help", "other"]);
        assert_eq!(cat.efforts, vec!["low", "high"]);
        assert!(cat.model_efforts.is_empty());
    }
}
