use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::event_log::{self, EventKind, NodeStatus};
use crate::outputs_validator;
use crate::pipeline;

pub const STALE_THRESHOLD: Duration = Duration::from_secs(120);

/// How often the background sweep wakes up. Independent of [`STALE_THRESHOLD`]
/// (the idle age that *counts* as stale): the sweep must tick often enough to
/// notice a threshold crossing promptly. Surfaced by `GET /stale/health` (#251)
/// and mirrors `trigger_scheduler::TICK_INTERVAL_SECS`.
pub const STALE_TICK_INTERVAL_SECS: u64 = 30;

/// On-screen anchors for Claude Code's usage-limit interactive menu (#290).
///
/// The menu wording is NOT officially documented and DRIFTS across CC versions
/// (corroborated by anthropics/claude-code#28484 + a direct capture 2026-06-30).
/// These are the substrings observed most stable; match is case-insensitive after
/// ANSI-stripping + whitespace-collapsing. Detection is best-effort /
/// observability-only (#290 Slice 1): a miss is the status quo (no regression), a
/// false positive is one harmless informational event. UPDATE THIS LIST when CC
/// wording changes.
const USAGE_LIMIT_ANCHORS: &[&str] = &[
    "stop and wait for limit to reset",
    "stop and wait for the limit to reset",
];

/// Strip ANSI/CSI escape sequences from a tmux pane capture (which is taken with
/// `-e`, so it contains escapes). Char-safe: preserves multi-byte UTF-8 (e.g. the
/// menu's `❯`). Best-effort — handles CSI (`ESC [ … final @-~`) and drops a lone
/// escape's next char; good enough for pane text (mostly SGR colour codes).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if matches!(nc, '\u{40}'..='\u{7e}') {
                        break; // CSI final byte
                    }
                }
            } else {
                chars.next(); // lone/other escape — drop the following char
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True if the captured pane shows Claude Code's usage-limit interactive menu.
/// `pane` is raw tmux capture (may contain ANSI). Observability-only (#290): the
/// caller flags the node but never changes its fate.
pub fn detect_usage_limit(pane: &str) -> bool {
    // Normalise: strip ANSI, lowercase (anchors are ASCII), collapse whitespace so
    // line-wrap / padding can't split an anchor.
    let stripped = strip_ansi(pane).to_ascii_lowercase();
    let norm = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    USAGE_LIMIT_ANCHORS.iter().any(|a| norm.contains(a))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    SessionDied,
    AutoComplete,
    Stale,
    Ok,
}

/// Inputs gathered by the caller (side-effectful layer) and passed into
/// the pure decision function.
pub struct NodeProbe {
    pub session_alive: bool,
    pub jsonl_mtime: Option<SystemTime>,
    pub now: SystemTime,
    pub artifacts_valid: Option<bool>,
}

/// Pure decision logic: given the probe results, determine the detection.
pub fn decide(probe: &NodeProbe) -> Detection {
    if !probe.session_alive {
        return Detection::SessionDied;
    }

    let Some(mtime) = probe.jsonl_mtime else {
        return Detection::Ok;
    };

    let age = probe.now.duration_since(mtime).unwrap_or(Duration::ZERO);
    if age < STALE_THRESHOLD {
        return Detection::Ok;
    }

    match probe.artifacts_valid {
        Some(true) => Detection::AutoComplete,
        _ => Detection::Stale,
    }
}

/// Encode a working directory path the same way Claude Code does for its
/// projects directory.
///
/// Example: `/home/user/project` → `home-user-project`
///
/// KNOWN BUG (do not fix here in a cost slice): this strips the leading `/` and
/// does not map `.`, so a PDO node dir like `/home/u/.pdo/runs/X/worktree`
/// encodes to `home-u--pdo-runs-X-worktree` **without** the leading `-` CC
/// actually uses — [`find_session_jsonl`] then returns `None` for every PDO
/// node, leaving the mtime-based stale/auto-complete probe effectively dead.
/// Fixing it re-activates that probe (a real behavioral change, #251-adjacent),
/// so it needs its own tests/validation. Cost estimation (#272) needs the
/// correct dir name and therefore uses its own [`crate::run_cost::cc_project_dirname`]
/// to stay isolated from this fix.
pub fn encode_working_dir(dir: &Path) -> String {
    let s = dir.to_string_lossy();
    let stripped = s.strip_prefix('/').unwrap_or(&s);
    stripped.replace('/', "-")
}

/// Find the most recently modified `.jsonl` file in the Claude Code projects
/// directory for the given working directory.
pub fn find_session_jsonl(working_dir: &Path) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let encoded = encode_working_dir(working_dir);
    let projects_dir = PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(&encoded);

    newest_jsonl_in(&projects_dir)
}

fn newest_jsonl_in(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    let mut newest: Option<(PathBuf, SystemTime)> = None;

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        match &newest {
            Some((_, best_time)) if mtime <= *best_time => {}
            _ => newest = Some((path, mtime)),
        }
    }

    newest.map(|(p, _)| p)
}

/// Validate outputs for a node using the pipeline definition.
pub fn validate_outputs(
    pipeline_path: &Path,
    node_id: &str,
    iter: i64,
    artifacts_dir: &Path,
) -> bool {
    let yaml = match std::fs::read_to_string(pipeline_path) {
        Ok(y) => y,
        Err(_) => return false,
    };
    let pipeline_def = match pipeline::parse_pipeline(&yaml) {
        Ok(p) => p.pipeline,
        Err(_) => return false,
    };

    outputs_validator::validate(&pipeline_def, node_id, iter, artifacts_dir).is_ok()
}

/// Build events for a detection result. Returns empty vec for Detection::Ok.
pub fn detection_events(
    detection: &Detection,
    run_id: &str,
    node_id: &str,
    iter: i64,
) -> Vec<event_log::Event> {
    // The session-died cause names the dead tmux session (#213 AC1) so the
    // failure is self-explanatory in the UI/log; the other causes are
    // node-relative and need no session name.
    let (kind, reason) = match detection {
        Detection::Ok => return vec![],
        Detection::SessionDied => {
            let session = crate::tmux_session_manager::node_session_name(run_id, node_id, iter);
            (
                EventKind::NodeFailed,
                format!("session_died: tmux session {session} no longer exists"),
            )
        }
        Detection::AutoComplete => (
            EventKind::NodeAutoCompleted,
            "auto_completed_idle_valid".to_string(),
        ),
        Detection::Stale => (EventKind::NodeStale, "idle_outputs_incomplete".to_string()),
    };

    vec![event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(serde_json::json!({ "reason": reason })),
    }]
}

/// Best-effort diagnostic context captured the moment a node's session is found
/// dead (#234). Without this the daemon records only the *symptom*
/// (`session_died: tmux session … no longer exists`) and every occurrence
/// becomes a from-scratch forensic investigation; with it the operator can tell
/// "one session died" from "the whole tmux server collapsed" on first sight.
///
/// Every field is best-effort: a `None` (or `0` for [`Self::correlated_deaths`])
/// means the probe could not run or found nothing, never a confirmed negative
/// — the impure sweep layer ([`crate::lib`]) does the tmux/proc I/O, this struct
/// only carries the result and shapes it into the payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionDeathDiagnostics {
    /// `tmux -L <socket> ls` result: `Some(false)` means the whole server is
    /// gone (every session under the socket died at once — the #234 root cause),
    /// `Some(true)` means only this one session vanished, `None` = probe failed.
    pub tmux_server_alive: Option<bool>,
    /// `MemAvailable` from `/proc/meminfo` at detection time, in KiB.
    pub mem_available_kb: Option<u64>,
    /// `SwapFree` from `/proc/meminfo` at detection time, in KiB.
    pub swap_free_kb: Option<u64>,
    /// How many *other* running nodes in the same run were also found
    /// session-dead in this sweep. A non-zero count points at a server-wide
    /// collapse (multiple runs dying ~ms apart) rather than an isolated death.
    pub correlated_deaths: usize,
}

impl SessionDeathDiagnostics {
    /// Shape the diagnostics into the JSON object attached to the `NodeFailed`
    /// payload alongside `reason`. Pure — no I/O. `None` fields serialise to
    /// `null` so a missing probe is distinguishable from a real value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tmux_server_alive": self.tmux_server_alive,
            "mem_available_kb": self.mem_available_kb,
            "swap_free_kb": self.swap_free_kb,
            "correlated_deaths": self.correlated_deaths,
        })
    }
}

/// Fold session-death diagnostics into the `NodeFailed` event(s) built by
/// [`detection_events`], adding a `diagnostics` object alongside `reason`.
///
/// Pure: the impure sweep gathers the diagnostics (tmux/proc reads) then calls
/// this. A no-op for any event whose payload is missing or not a JSON object,
/// so the non-`SessionDied` detections (which carry no diagnostics) are never
/// touched even if this is mistakenly called on them.
pub fn attach_diagnostics(events: &mut [event_log::Event], diag: &SessionDeathDiagnostics) {
    for event in events.iter_mut() {
        if let Some(obj) = event.payload.as_mut().and_then(|p| p.as_object_mut()) {
            obj.insert("diagnostics".to_string(), diag.to_json());
        }
    }
}

/// Parse the contents of `/proc/meminfo`, returning `(MemAvailable, SwapFree)`
/// in KiB. Either is `None` when its line is absent or unparseable. Pure, so
/// the impure sweep layer only performs the file read.
pub fn parse_meminfo(contents: &str) -> (Option<u64>, Option<u64>) {
    // Lines look like `MemAvailable:    1234 kB`; take the first numeric token
    // after the `<key>:` prefix. The trailing `:` keeps `MemAvailable` from
    // matching `MemFree`/`MemTotal` and `SwapFree` from matching `SwapTotal`.
    let field = |key: &str| -> Option<u64> {
        contents.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?;
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    (field("MemAvailable:"), field("SwapFree:"))
}

/// Count how many of `running` (other than `self_node`) are session-dead,
/// according to `is_dead`. Pure given the predicate so the counting logic is
/// testable without tmux; the impure sweep passes a closure backed by
/// `tmux_session_manager::session_exists`.
pub fn count_correlated_deaths(
    running: &[(String, i64)],
    self_node: (&str, i64),
    is_dead: impl Fn(&str, i64) -> bool,
) -> usize {
    running
        .iter()
        .filter(|(id, it)| (id.as_str(), *it) != self_node)
        .filter(|(id, it)| is_dead(id, *it))
        .count()
}

/// Collect all running nodes from a RunState.
pub fn running_nodes(run_state: &event_log::RunState) -> Vec<(String, i64)> {
    run_state
        .nodes
        .iter()
        .filter(|(_, ns)| ns.status == NodeStatus::Running)
        .map(|(id, ns)| (id.clone(), ns.iter))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    // --- detect_usage_limit / strip_ansi (#290) ---

    #[test]
    fn detects_usage_limit_menu_with_ansi() {
        // A realistic capture: the selected line carries SGR colour codes and the
        // `❯` cursor glyph, wrapped by the surrounding menu text.
        let pane = "What do you want to do?\n\x1b[2m❯\x1b[0m 1. Stop and \
                    wait for limit to reset\n  2. Switch to usage credits\n";
        assert!(detect_usage_limit(pane));
    }

    #[test]
    fn detects_the_the_variant() {
        assert!(detect_usage_limit(
            "…please wait for the limit to reset. Stop and wait for the limit to reset\n"
        ));
    }

    #[test]
    fn case_insensitive() {
        assert!(detect_usage_limit("STOP AND WAIT FOR LIMIT TO RESET"));
    }

    #[test]
    fn wrapped_anchor_still_matches() {
        // The anchor split by a newline + padding (as a narrow pane would wrap it)
        // must still match — proves the whitespace-collapse normalisation.
        assert!(detect_usage_limit(
            "stop and wait\n   for   limit\nto reset"
        ));
    }

    #[test]
    fn normal_running_pane_is_not_flagged() {
        let pane = "\x1b[2m✻\x1b[0m Thinking… (esc to interrupt)\n\
                    ● Running: cargo test -p pdo-daemon\n";
        assert!(!detect_usage_limit(pane));
    }

    #[test]
    fn empty_pane_is_not_flagged() {
        assert!(!detect_usage_limit(""));
    }

    #[test]
    fn strip_ansi_preserves_unicode() {
        assert_eq!(strip_ansi("\x1b[1m❯\x1b[0m x"), "❯ x");
    }

    // --- encode_working_dir ---

    #[test]
    fn encode_basic_path() {
        assert_eq!(
            encode_working_dir(Path::new("/home/user/project")),
            "home-user-project"
        );
    }

    #[test]
    fn encode_root() {
        assert_eq!(encode_working_dir(Path::new("/")), "");
    }

    #[test]
    fn encode_deeply_nested() {
        assert_eq!(encode_working_dir(Path::new("/a/b/c/d/e")), "a-b-c-d-e");
    }

    // --- decide (pure logic) ---

    #[test]
    fn dead_session_returns_session_died() {
        let probe = NodeProbe {
            session_alive: false,
            jsonl_mtime: None,
            now: SystemTime::now(),
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::SessionDied);
    }

    #[test]
    fn dead_session_regardless_of_mtime() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: false,
            jsonl_mtime: Some(now - Duration::from_secs(300)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::SessionDied);
    }

    #[test]
    fn no_jsonl_file_returns_ok() {
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: None,
            now: SystemTime::now(),
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn fresh_mtime_returns_ok() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(60)),
            now,
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn threshold_boundary_119s_returns_ok() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(119)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::Ok);
    }

    #[test]
    fn threshold_boundary_120s_with_valid_artifacts_returns_auto_complete() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(120)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn threshold_boundary_120s_with_invalid_artifacts_returns_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(120)),
            now,
            artifacts_valid: Some(false),
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    #[test]
    fn threshold_boundary_121s_with_valid_artifacts_returns_auto_complete() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(121)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn threshold_boundary_121s_with_missing_artifacts_returns_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(121)),
            now,
            artifacts_valid: None,
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    #[test]
    fn idle_with_valid_artifacts_auto_completes() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(200)),
            now,
            artifacts_valid: Some(true),
        };
        assert_eq!(decide(&probe), Detection::AutoComplete);
    }

    #[test]
    fn idle_with_invalid_artifacts_is_stale() {
        let now = SystemTime::now();
        let probe = NodeProbe {
            session_alive: true,
            jsonl_mtime: Some(now - Duration::from_secs(200)),
            now,
            artifacts_valid: Some(false),
        };
        assert_eq!(decide(&probe), Detection::Stale);
    }

    // --- detection_events ---

    #[test]
    fn events_ok_is_empty() {
        assert!(detection_events(&Detection::Ok, "r", "n", 1).is_empty());
    }

    #[test]
    fn events_session_died() {
        let events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeFailed);
        assert_eq!(events[0].node_id.as_deref(), Some("node1"));
        let payload = events[0].payload.as_ref().unwrap();
        // #213 AC1: the failure cause must name the dead tmux session so an
        // operator inspecting the run can tell exactly which session vanished.
        let reason = payload["reason"].as_str().unwrap();
        assert!(
            reason.contains("pdo-run1-node1-iter-1"),
            "session-died cause {reason:?} must name the dead session"
        );
    }

    #[test]
    fn events_auto_complete() {
        let events = detection_events(&Detection::AutoComplete, "run1", "node1", 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeAutoCompleted);
        assert_eq!(events[0].iter, Some(2));
    }

    #[test]
    fn events_stale() {
        let events = detection_events(&Detection::Stale, "run1", "node1", 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::NodeStale);
    }

    // --- #234 session-death diagnostics ---

    #[test]
    fn diagnostics_to_json_carries_all_fields() {
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(123),
            swap_free_kb: Some(456),
            correlated_deaths: 2,
        };
        let json = diag.to_json();
        assert_eq!(json["tmux_server_alive"], serde_json::json!(false));
        assert_eq!(json["mem_available_kb"], serde_json::json!(123));
        assert_eq!(json["swap_free_kb"], serde_json::json!(456));
        assert_eq!(json["correlated_deaths"], serde_json::json!(2));
    }

    #[test]
    fn diagnostics_to_json_none_probes_serialize_to_null() {
        // A failed probe must be distinguishable from a real value, so `None`
        // fields serialise to JSON `null` rather than being dropped.
        let json = SessionDeathDiagnostics::default().to_json();
        assert!(json["tmux_server_alive"].is_null());
        assert!(json["mem_available_kb"].is_null());
        assert!(json["swap_free_kb"].is_null());
        assert_eq!(json["correlated_deaths"], serde_json::json!(0));
    }

    #[test]
    fn attach_diagnostics_enriches_session_died_payload_alongside_reason() {
        let mut events = detection_events(&Detection::SessionDied, "run1", "node1", 1);
        let diag = SessionDeathDiagnostics {
            tmux_server_alive: Some(false),
            mem_available_kb: Some(2048),
            swap_free_kb: Some(0),
            correlated_deaths: 1,
        };
        attach_diagnostics(&mut events, &diag);

        let payload = events[0].payload.as_ref().unwrap();
        // The original symptom is preserved …
        assert!(payload["reason"].as_str().unwrap().contains("session_died"));
        // … and the diagnostics sit alongside it.
        assert_eq!(
            payload["diagnostics"]["tmux_server_alive"],
            serde_json::json!(false)
        );
        assert_eq!(
            payload["diagnostics"]["correlated_deaths"],
            serde_json::json!(1)
        );
        assert_eq!(
            payload["diagnostics"]["mem_available_kb"],
            serde_json::json!(2048)
        );
    }

    #[test]
    fn attach_diagnostics_is_noop_on_empty_events() {
        // Detection::Ok yields no events — attaching must not panic.
        let mut events = detection_events(&Detection::Ok, "run1", "node1", 1);
        attach_diagnostics(&mut events, &SessionDeathDiagnostics::default());
        assert!(events.is_empty());
    }

    #[test]
    fn parse_meminfo_extracts_available_and_swap() {
        let contents = "\
MemTotal:       16384000 kB
MemFree:          512000 kB
MemAvailable:    8192000 kB
SwapTotal:       2048000 kB
SwapFree:         204800 kB
";
        let (mem, swap) = parse_meminfo(contents);
        assert_eq!(mem, Some(8192000));
        assert_eq!(swap, Some(204800));
    }

    #[test]
    fn parse_meminfo_missing_fields_return_none() {
        // No MemAvailable / SwapFree lines (e.g. an ancient kernel) → None,
        // not a wrong value picked up from a similarly-named line.
        let contents = "MemTotal:  16384000 kB\nMemFree:  512000 kB\nSwapTotal:  2048000 kB\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn parse_meminfo_ignores_malformed_values() {
        let contents = "MemAvailable:  notanumber kB\nSwapFree:\n";
        assert_eq!(parse_meminfo(contents), (None, None));
    }

    #[test]
    fn count_correlated_deaths_excludes_self_and_counts_dead_peers() {
        let running = vec![
            ("a".to_string(), 1),
            ("b".to_string(), 1),
            ("c".to_string(), 2),
        ];
        // Self ("a", 1) is excluded even though the predicate would call it
        // dead; "b" is dead, "c" is alive → exactly one correlated death.
        let dead = |id: &str, _it: i64| id != "c";
        assert_eq!(count_correlated_deaths(&running, ("a", 1), dead), 1);
    }

    #[test]
    fn count_correlated_deaths_zero_when_peers_alive() {
        let running = vec![("a".to_string(), 1), ("b".to_string(), 1)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| false), 0);
    }

    #[test]
    fn count_correlated_deaths_distinguishes_iter() {
        // Same node id, different iter, must be treated as a distinct peer and
        // counted — not collapsed onto self.
        let running = vec![("a".to_string(), 1), ("a".to_string(), 2)];
        assert_eq!(count_correlated_deaths(&running, ("a", 1), |_, _| true), 1);
    }

    // --- running_nodes ---

    #[test]
    fn running_nodes_filters_correctly() {
        use crate::event_log::{project, Event, EventKind};

        fn make_event(kind: EventKind, node_id: Option<&str>, iter: Option<i64>) -> Event {
            let payload = if kind == EventKind::RunStarted {
                Some(serde_json::json!({ "pipeline_name": "test" }))
            } else {
                None
            };
            Event {
                id: None,
                run_id: "test-run".to_string(),
                ts: event_log::now_iso(),
                kind,
                node_id: node_id.map(String::from),
                iter,
                payload,
            }
        }

        let events = vec![
            make_event(EventKind::RunStarted, None, None),
            make_event(EventKind::NodeStarted, Some("worker"), Some(1)),
            make_event(EventKind::NodeStarted, Some("planner"), Some(1)),
            make_event(EventKind::NodeCompleted, Some("planner"), Some(1)),
        ];

        let state = project(&events).unwrap();
        let running = running_nodes(&state);
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].0, "worker");
        assert_eq!(running[0].1, 1);
    }

    // --- find_session_jsonl (filesystem) ---

    /// RAII guard that swaps HOME for a temp dir while holding the crate-wide
    /// HOME lock. `find_session_jsonl` reads `$HOME`, and other test modules
    /// (library_store, lib.rs FakeHome) also mutate HOME — without the lock,
    /// parallel test threads clobber each other's HOME mid-test.
    struct TempHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        tmp: tempfile::TempDir,
        prev: Option<std::ffi::OsString>,
    }

    impl TempHome {
        fn new() -> Self {
            let lock = crate::library_store::HOME_TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let tmp = tempfile::tempdir().unwrap();
            let prev = std::env::var_os("HOME");
            std::env::set_var("HOME", tmp.path());
            Self {
                _lock: lock,
                tmp,
                prev,
            }
        }

        fn path(&self) -> &Path {
            self.tmp.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn find_jsonl_returns_newest_file() {
        let home_guard = TempHome::new();
        let home = home_guard.path();

        let encoded = encode_working_dir(Path::new("/home/user/project"));
        let projects_dir = home.join(".claude").join("projects").join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        let old_file = projects_dir.join("old-session.jsonl");
        std::fs::write(&old_file, "old").unwrap();
        filetime::set_file_mtime(
            &old_file,
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(300)),
        )
        .unwrap();

        let new_file = projects_dir.join("new-session.jsonl");
        std::fs::write(&new_file, "new").unwrap();

        let result = find_session_jsonl(Path::new("/home/user/project"));

        assert!(result.is_some());
        assert_eq!(result.unwrap().file_name().unwrap(), "new-session.jsonl");
    }

    #[test]
    fn find_jsonl_no_dir_returns_none() {
        let _home_guard = TempHome::new();
        assert!(find_session_jsonl(Path::new("/nonexistent/dir")).is_none());
    }

    #[test]
    fn find_jsonl_ignores_non_jsonl_files() {
        let home_guard = TempHome::new();
        let home = home_guard.path();

        let encoded = encode_working_dir(Path::new("/tmp/testdir"));
        let projects_dir = home.join(".claude").join("projects").join(&encoded);
        std::fs::create_dir_all(&projects_dir).unwrap();

        std::fs::write(projects_dir.join("notes.txt"), "not jsonl").unwrap();
        std::fs::write(projects_dir.join("data.json"), "not jsonl either").unwrap();

        assert!(find_session_jsonl(Path::new("/tmp/testdir")).is_none());
    }

    // --- validate_outputs (integration with outputs_validator) ---

    #[test]
    fn validate_outputs_with_no_declared_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs: []\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }

    #[test]
    fn validate_outputs_with_missing_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let pipeline_path = tmp.path().join("pipeline.yaml");
        std::fs::write(
            &pipeline_path,
            "name: test\nnodes:\n  - id: start\n    name: Start\n    type: start\n    inputs: []\n    outputs:\n      - name: user_prompt\n  - id: worker\n    name: Worker\n    type: doc-only\n    inputs:\n      - name: task\n    outputs:\n      - name: report\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n    outputs: []\nedges:\n  - source: { node: start, port: user_prompt }\n    target: { node: worker, port: task }\n",
        )
        .unwrap();

        let artifacts_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        assert!(!validate_outputs(
            &pipeline_path,
            "worker",
            1,
            &artifacts_dir
        ));
    }
}
