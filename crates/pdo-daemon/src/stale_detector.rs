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

/// Whether a node's transcript is idle past [`STALE_THRESHOLD`] as of `now`.
///
/// The **single source** of the threshold comparison (#373): both [`decide`]
/// (the staleness authority) and [`assess_node`] (which consults it only to
/// gate the costly outputs validation) go through here, so the gate is never
/// re-implemented. A future `mtime` (clock skew) counts as not-idle.
fn idle_past_threshold(mtime: SystemTime, now: SystemTime) -> bool {
    now.duration_since(mtime).unwrap_or(Duration::ZERO) >= STALE_THRESHOLD
}

/// Pure decision logic: given the probe results, determine the detection.
pub fn decide(probe: &NodeProbe) -> Detection {
    if !probe.session_alive {
        return Detection::SessionDied;
    }

    let Some(mtime) = probe.jsonl_mtime else {
        return Detection::Ok;
    };

    if !idle_past_threshold(mtime, probe.now) {
        return Detection::Ok;
    }

    match probe.artifacts_valid {
        Some(true) => Detection::AutoComplete,
        _ => Detection::Stale,
    }
}

/// Encode a working directory path exactly as Claude Code names its
/// `~/.claude/projects/` directory: every non-`[A-Za-z0-9]` char maps to `-`
/// (case preserved, runs NOT collapsed). So a leading `/` becomes a leading `-`
/// and `.pdo`/`.claude` become `--pdo`/`--claude`.
///
/// Example: `/home/user/project` → `-home-user-project`; a PDO node dir like
/// `/home/u/.pdo/runs/X/worktree` → `-home-u--pdo-runs-X-worktree`.
///
/// This is the single source of truth for the CC project-dir encoding;
/// [`crate::run_cost::cc_project_dirname`] delegates here.
///
/// History (#373): this previously stripped the leading `/` and left `.`
/// intact, so [`find_session_jsonl`] resolved `None` for *every* PDO node and
/// the mtime-based `Stale`/`AutoComplete` branches of [`decide`] were dead in
/// production. Fixed and verified against real on-disk dirs.
pub fn encode_working_dir(dir: &Path) -> String {
    dir.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
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

/// Injected I/O for [`assess_node`]. The impure sweep layer ([`crate::lib`])
/// implements this against tmux + the filesystem for one running node; unit
/// tests supply a fake so the whole probe → gate → decide → events →
/// diagnostics → dedup pipeline runs without a daemon.
///
/// Every method is a side-effect-free *read*: the reap/spawn side effects stay
/// in the sweep, keyed off [`Assessment::detection`].
pub trait NodeProbes {
    /// Is the node's tmux session still alive?
    fn session_alive(&self) -> bool;

    /// mtime of the newest Claude Code transcript for this node's working dir,
    /// or `None` when no transcript dir/file resolves.
    fn jsonl_mtime(&self) -> Option<SystemTime>;

    /// Do the node's declared outputs validate against the pipeline? Consulted
    /// by [`assess_node`] **only** once the idle threshold is crossed, so the
    /// (relatively costly) validation never runs on a fresh node.
    fn validate_outputs(&self) -> bool;

    /// Best-effort capture of the node's tmux pane, for the usage-limit menu
    /// probe (#290). Only called on the `Ok` path (an alive, non-stale node).
    fn capture_pane(&self) -> Option<String>;

    /// Best-effort session-death forensics (#234). Gathered lazily — only when
    /// the session is found dead — so no tmux/proc I/O runs on a healthy node.
    fn session_death_diagnostics(&self) -> SessionDeathDiagnostics;
}

/// Whether the mtime-based auto-complete path is allowed to *act* (reap the
/// idle session and advance the pipeline) or only to be *observed* (#373).
///
/// Auto-complete is irreversible and can fire falsely on a node mid a
/// legitimate >[`STALE_THRESHOLD`] tool call whose outputs already validate, so
/// re-arming the terminal action is trust-gated (ADR-0012). Unit A ships
/// [`Observe`](Self::Observe): [`assess_node`] emits the non-terminal
/// [`event_log::EventKind::NodeAutoCompleteObserved`] marker (node stays
/// Running) instead of the terminal `NodeAutoCompleted`. Flipping the sweep to
/// [`Act`](Self::Act) is the Unit B change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCompletePolicy {
    /// Emit an observe-only marker; never reap/advance. (#373 Unit A.)
    Observe,
    /// Emit the terminal `NodeAutoCompleted`; the sweep reaps + advances.
    Act,
}

/// Outcome of [`assess_node`]: the raw detection plus the events to append and
/// the observability the sweep needs. The impure caller appends
/// [`Self::events`] and runs any reap/spawn side effects keyed off
/// [`Self::detection`].
///
/// Not `PartialEq`/`Eq`: [`event_log::Event`]'s payload is a
/// `serde_json::Value` (no `Eq`), and callers/tests inspect the fields
/// individually rather than compare a whole `Assessment`.
#[derive(Debug, Clone)]
pub struct Assessment {
    /// Raw detection from [`decide`] (before the [`AutoCompletePolicy`] is
    /// applied to the auto-complete event shape). The sweep drives its
    /// reap/spawn side effects off this.
    pub detection: Detection,
    /// Events to append. A `SessionDied` failure already carries its
    /// diagnostics; an `Observe`-policy auto-complete carries a non-terminal
    /// `NodeAutoCompleteObserved`; a usage-limit menu carries a (deduped)
    /// `NodeBlockedOnLimit`. Empty for a nominal `Ok` node.
    pub events: Vec<event_log::Event>,
    /// True when the node is alive & non-stale but its pane shows Claude Code's
    /// usage-limit menu — feeds the per-sweep `blocked_on_limit` gauge (#290).
    /// Set on every sweep the menu is visible, independent of event dedup.
    pub blocked_on_limit: bool,
    /// The session-death forensics gathered on the `SessionDied` path (`None`
    /// otherwise), surfaced so the sweep can log the structured fields (#234)
    /// without re-running the probe or re-parsing the event payload.
    pub session_death_diagnostics: Option<SessionDeathDiagnostics>,
}

/// True when an event of `kind` for `(node_id, iter)` already exists in
/// `prior_events` — the rising-edge de-dup key for the informational markers
/// (`NodeBlockedOnLimit`, `NodeAutoCompleteObserved`). Pure over the event log
/// snapshot the sweep already loaded, so a held condition emits one event, not
/// one per ~30 s sweep tick, and the dedup survives a daemon restart.
fn episode_has_event(
    prior_events: &[event_log::Event],
    kind: &EventKind,
    node_id: &str,
    iter: i64,
) -> bool {
    prior_events
        .iter()
        .any(|e| &e.kind == kind && e.node_id.as_deref() == Some(node_id) && e.iter == Some(iter))
}

fn informational_event(
    kind: EventKind,
    run_id: &str,
    node_id: &str,
    iter: i64,
    payload: serde_json::Value,
) -> event_log::Event {
    event_log::Event {
        id: None,
        run_id: run_id.to_string(),
        ts: event_log::now_iso(),
        kind,
        node_id: Some(node_id.to_string()),
        iter: Some(iter),
        payload: Some(payload),
    }
}

/// The stale-detection policy for a single running node, with all I/O injected
/// via `probes` (#373). This is the one place the whole pipeline lives:
///
/// ```text
/// probes → STALE_THRESHOLD gate → decide → detection_events
///        → attach_diagnostics (SessionDied) → usage-limit-dedup (Ok)
/// ```
///
/// so [`crate::lib`]'s sweep is reduced to a loop that builds a [`NodeProbes`]
/// adapter, calls this, appends [`Assessment::events`], and runs the reap/spawn
/// side effects keyed off [`Assessment::detection`].
///
/// The `STALE_THRESHOLD` gate has a **single source**: [`decide`].
/// `assess_node` consults [`NodeProbes::validate_outputs`] only once the idle
/// age has (potentially) crossed the threshold, so a fresh node never pays for
/// outputs validation — but the gating *decision* itself is not duplicated
/// here, it is [`decide`]'s.
///
/// `prior_events` is the run's event-log snapshot, used purely for the
/// rising-edge de-dup of the informational markers (see [`episode_has_event`]).
pub fn assess_node(
    probes: &impl NodeProbes,
    prior_events: &[event_log::Event],
    run_id: &str,
    node_id: &str,
    iter: i64,
    now: SystemTime,
    auto_complete_policy: AutoCompletePolicy,
) -> Assessment {
    let session_alive = probes.session_alive();
    let jsonl_mtime = probes.jsonl_mtime();

    // Validate outputs only once the transcript is idle past the threshold,
    // via the SAME [`idle_past_threshold`] gate `decide` uses — this avoids the
    // validation I/O on a fresh node without re-implementing the gate. `decide`
    // remains the sole authority on the Ok/Stale/AutoComplete partition.
    let threshold_crossed = jsonl_mtime.is_some_and(|mt| idle_past_threshold(mt, now));
    let artifacts_valid = threshold_crossed.then(|| probes.validate_outputs());

    let probe = NodeProbe {
        session_alive,
        jsonl_mtime,
        now,
        artifacts_valid,
    };
    let detection = decide(&probe);

    match detection {
        Detection::Ok => {
            // Alive & non-stale, but maybe wedged on Claude Code's usage-limit
            // menu (#290): observability only — the node keeps running. The
            // gauge counts every sweep the menu is visible; the event is emitted
            // once per (node, iter) episode.
            let blocked_on_limit = probes
                .capture_pane()
                .is_some_and(|pane| detect_usage_limit(&pane));
            let events = if blocked_on_limit
                && !episode_has_event(prior_events, &EventKind::NodeBlockedOnLimit, node_id, iter)
            {
                vec![informational_event(
                    EventKind::NodeBlockedOnLimit,
                    run_id,
                    node_id,
                    iter,
                    serde_json::json!({ "signal": "usage_limit_menu" }),
                )]
            } else {
                vec![]
            };
            Assessment {
                detection,
                events,
                blocked_on_limit,
                session_death_diagnostics: None,
            }
        }
        Detection::SessionDied => {
            let mut events = detection_events(&detection, run_id, node_id, iter);
            let diag = probes.session_death_diagnostics();
            attach_diagnostics(&mut events, &diag);
            Assessment {
                detection,
                events,
                blocked_on_limit: false,
                session_death_diagnostics: Some(diag),
            }
        }
        Detection::AutoComplete => {
            // #373 Unit A: re-arming the terminal auto-complete (reap + advance)
            // is trust-gated. Under `Observe`, emit a deduped non-terminal marker
            // so the node stays Running and the "would auto-complete" moment is
            // durably visible; under `Act`, emit the real terminal event.
            let events = match auto_complete_policy {
                AutoCompletePolicy::Act => detection_events(&detection, run_id, node_id, iter),
                AutoCompletePolicy::Observe => {
                    if episode_has_event(
                        prior_events,
                        &EventKind::NodeAutoCompleteObserved,
                        node_id,
                        iter,
                    ) {
                        vec![]
                    } else {
                        vec![informational_event(
                            EventKind::NodeAutoCompleteObserved,
                            run_id,
                            node_id,
                            iter,
                            serde_json::json!({ "reason": "idle_valid_outputs_observe_only" }),
                        )]
                    }
                }
            };
            Assessment {
                detection,
                events,
                blocked_on_limit: false,
                session_death_diagnostics: None,
            }
        }
        Detection::Stale => Assessment {
            events: detection_events(&detection, run_id, node_id, iter),
            detection,
            blocked_on_limit: false,
            session_death_diagnostics: None,
        },
    }
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

    // --- encode_working_dir (#373: matches Claude Code's real scheme) ---

    #[test]
    fn encode_basic_path_keeps_leading_dash() {
        // Every non-alphanumeric maps to `-`, so a leading `/` becomes `-`.
        assert_eq!(
            encode_working_dir(Path::new("/home/user/project")),
            "-home-user-project"
        );
    }

    #[test]
    fn encode_root() {
        assert_eq!(encode_working_dir(Path::new("/")), "-");
    }

    #[test]
    fn encode_deeply_nested() {
        assert_eq!(encode_working_dir(Path::new("/a/b/c/d/e")), "-a-b-c-d-e");
    }

    #[test]
    fn encode_maps_dot_to_dash() {
        // #373 root cause: a real PDO node dir carries `.pdo` (→ `--pdo`) and a
        // leading `-`. Before the fix this produced `home-...-.pdo-...` and
        // resolved nothing under ~/.claude/projects.
        assert_eq!(
            encode_working_dir(Path::new("/home/u/.pdo/runs/X/worktree")),
            "-home-u--pdo-runs-X-worktree"
        );
    }

    #[test]
    fn encode_matches_cc_project_dirname() {
        // The two encoders are unified on one implementation (#373): they must
        // never drift again.
        for dir in [
            "/home/llenoir/Documents/perso/Maestro/.pdo/runs/2026-abc/nodes/n1/iter-1",
            "/home/u/.claude",
            "/tmp/x.y.z",
        ] {
            assert_eq!(
                encode_working_dir(Path::new(dir)),
                crate::run_cost::cc_project_dirname(Path::new(dir)),
                "encode_working_dir and cc_project_dirname must agree for {dir}"
            );
        }
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

    #[test]
    fn find_jsonl_resolves_a_real_pdo_node_dir() {
        // #373 regression: a representative PDO node working dir (absolute,
        // carries `.pdo`) must resolve to the transcript CC actually writes —
        // i.e. under the leading-dash, `--pdo` name. Pre-fix this looked up
        // `home-...-.pdo-...` and found nothing, so the mtime probe was dead.
        let home_guard = TempHome::new();
        let home = home_guard.path();

        let node_dir =
            Path::new("/home/llenoir/Documents/perso/Maestro/.pdo/runs/20260623-100032-9b8331b/nodes/gzpYZA2m/iter-1");

        // The transcript dir CC writes: leading `-`, `.pdo` → `--pdo`.
        let cc_name = home
            .join(".claude")
            .join("projects")
            .join("-home-llenoir-Documents-perso-Maestro--pdo-runs-20260623-100032-9b8331b-nodes-gzpYZA2m-iter-1");
        std::fs::create_dir_all(&cc_name).unwrap();
        std::fs::write(cc_name.join("session.jsonl"), "{}").unwrap();

        // The encoder now produces exactly that name …
        assert_eq!(
            home.join(".claude")
                .join("projects")
                .join(encode_working_dir(node_dir)),
            cc_name
        );
        // … so the probe resolves the transcript.
        let found = find_session_jsonl(node_dir);
        assert!(
            found.is_some(),
            "find_session_jsonl must resolve a real PDO node transcript after the #373 fix"
        );
        assert_eq!(found.unwrap().file_name().unwrap(), "session.jsonl");
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

    // --- assess_node (#373: whole sweep-policy pipeline with fake I/O) ---

    /// A fully controllable [`NodeProbes`] fake. `validate_calls` records how
    /// many times `validate_outputs` ran so a test can prove the threshold gate
    /// short-circuits the (costly) validation on a fresh node.
    struct FakeProbes {
        session_alive: bool,
        jsonl_mtime: Option<SystemTime>,
        outputs_valid: bool,
        pane: Option<String>,
        diagnostics: SessionDeathDiagnostics,
        validate_calls: std::cell::Cell<usize>,
    }

    impl FakeProbes {
        /// Alive, transcript idle `age` old, outputs `valid`, no pane.
        fn idle(age: Duration, valid: bool) -> Self {
            Self {
                session_alive: true,
                jsonl_mtime: Some(SystemTime::now() - age),
                outputs_valid: valid,
                pane: None,
                diagnostics: SessionDeathDiagnostics::default(),
                validate_calls: std::cell::Cell::new(0),
            }
        }
    }

    impl NodeProbes for FakeProbes {
        fn session_alive(&self) -> bool {
            self.session_alive
        }
        fn jsonl_mtime(&self) -> Option<SystemTime> {
            self.jsonl_mtime
        }
        fn validate_outputs(&self) -> bool {
            self.validate_calls.set(self.validate_calls.get() + 1);
            self.outputs_valid
        }
        fn capture_pane(&self) -> Option<String> {
            self.pane.clone()
        }
        fn session_death_diagnostics(&self) -> SessionDeathDiagnostics {
            self.diagnostics.clone()
        }
    }

    fn assess(probes: &FakeProbes, policy: AutoCompletePolicy) -> Assessment {
        assess_node(probes, &[], "run1", "worker", 1, SystemTime::now(), policy)
    }

    #[test]
    fn assess_dead_session_fails_with_diagnostics() {
        let probes = FakeProbes {
            session_alive: false,
            jsonl_mtime: Some(SystemTime::now() - Duration::from_secs(300)),
            outputs_valid: true, // must be ignored: dead session wins
            pane: None,
            diagnostics: SessionDeathDiagnostics {
                tmux_server_alive: Some(false),
                correlated_deaths: 2,
                ..Default::default()
            },
            validate_calls: std::cell::Cell::new(0),
        };
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::SessionDied);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeFailed);
        // #234: diagnostics folded into the failure payload AND surfaced for the
        // sweep's structured log.
        assert_eq!(
            a.events[0].payload.as_ref().unwrap()["diagnostics"]["correlated_deaths"],
            serde_json::json!(2)
        );
        assert_eq!(a.session_death_diagnostics.unwrap().correlated_deaths, 2);
        assert!(!a.blocked_on_limit);
    }

    #[test]
    fn assess_fresh_node_is_ok_and_skips_validation() {
        // Alive, transcript touched 60 s ago (< threshold): Ok, no events, and —
        // the single-source gate — outputs validation is never invoked.
        let probes = FakeProbes::idle(Duration::from_secs(60), true);
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::Ok);
        assert!(a.events.is_empty());
        assert_eq!(
            probes.validate_calls.get(),
            0,
            "validate_outputs must NOT run before the threshold is crossed"
        );
    }

    #[test]
    fn assess_no_transcript_is_ok_and_skips_validation() {
        let probes = FakeProbes {
            session_alive: true,
            jsonl_mtime: None,
            outputs_valid: true,
            pane: None,
            diagnostics: SessionDeathDiagnostics::default(),
            validate_calls: std::cell::Cell::new(0),
        };
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::Ok);
        assert!(a.events.is_empty());
        assert_eq!(probes.validate_calls.get(), 0);
    }

    #[test]
    fn assess_idle_invalid_outputs_is_stale() {
        let probes = FakeProbes::idle(Duration::from_secs(200), false);
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::Stale);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeStale);
        assert_eq!(
            probes.validate_calls.get(),
            1,
            "validate_outputs must run once past the threshold"
        );
    }

    #[test]
    fn assess_idle_valid_outputs_observe_only_is_non_terminal() {
        // The core #373 Unit A guard: idle + valid outputs would auto-complete,
        // but under Observe it must NOT emit the terminal NodeAutoCompleted —
        // only the non-terminal observed marker.
        let probes = FakeProbes::idle(Duration::from_secs(200), true);
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::AutoComplete);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeAutoCompleteObserved);
        assert_ne!(a.events[0].kind, EventKind::NodeAutoCompleted);
    }

    #[test]
    fn assess_idle_valid_outputs_act_emits_terminal_auto_completed() {
        // Unit B shape (behind the policy): the same detection, but Act emits the
        // real terminal event the sweep would reap/advance on.
        let probes = FakeProbes::idle(Duration::from_secs(200), true);
        let a = assess(&probes, AutoCompletePolicy::Act);
        assert_eq!(a.detection, Detection::AutoComplete);
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeAutoCompleted);
    }

    #[test]
    fn assess_observe_marker_is_deduped_per_episode() {
        // A held idle+valid node must emit the observed marker once, not once
        // per sweep: a prior marker for this (node, iter) suppresses re-emission.
        let probes = FakeProbes::idle(Duration::from_secs(200), true);
        let prior = vec![event_log::Event {
            id: None,
            run_id: "run1".to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeAutoCompleteObserved,
            node_id: Some("worker".to_string()),
            iter: Some(1),
            payload: None,
        }];
        let a = assess_node(
            &probes,
            &prior,
            "run1",
            "worker",
            1,
            SystemTime::now(),
            AutoCompletePolicy::Observe,
        );
        assert_eq!(a.detection, Detection::AutoComplete);
        assert!(
            a.events.is_empty(),
            "a second observe marker for the same episode must be suppressed"
        );
    }

    #[test]
    fn assess_usage_limit_menu_flags_blocked_and_emits_once() {
        let probes = FakeProbes {
            session_alive: true,
            jsonl_mtime: Some(SystemTime::now() - Duration::from_secs(30)), // fresh → Ok
            outputs_valid: false,
            pane: Some("❯ 1. Stop and wait for limit to reset".to_string()),
            diagnostics: SessionDeathDiagnostics::default(),
            validate_calls: std::cell::Cell::new(0),
        };
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::Ok);
        assert!(
            a.blocked_on_limit,
            "usage-limit menu must set the gauge flag"
        );
        assert_eq!(a.events.len(), 1);
        assert_eq!(a.events[0].kind, EventKind::NodeBlockedOnLimit);
    }

    #[test]
    fn assess_usage_limit_gauge_set_but_event_deduped() {
        // On a subsequent sweep the menu is still up: the gauge still counts it,
        // but the event is not re-emitted (rising-edge dedup).
        let probes = FakeProbes {
            session_alive: true,
            jsonl_mtime: Some(SystemTime::now() - Duration::from_secs(30)),
            outputs_valid: false,
            pane: Some("Stop and wait for limit to reset".to_string()),
            diagnostics: SessionDeathDiagnostics::default(),
            validate_calls: std::cell::Cell::new(0),
        };
        let prior = vec![event_log::Event {
            id: None,
            run_id: "run1".to_string(),
            ts: event_log::now_iso(),
            kind: EventKind::NodeBlockedOnLimit,
            node_id: Some("worker".to_string()),
            iter: Some(1),
            payload: None,
        }];
        let a = assess_node(
            &probes,
            &prior,
            "run1",
            "worker",
            1,
            SystemTime::now(),
            AutoCompletePolicy::Observe,
        );
        assert!(
            a.blocked_on_limit,
            "gauge counts every sweep the menu is up"
        );
        assert!(
            a.events.is_empty(),
            "the blocked event is emitted only once"
        );
    }

    #[test]
    fn assess_ok_node_without_menu_is_silent() {
        let probes = FakeProbes {
            session_alive: true,
            jsonl_mtime: Some(SystemTime::now() - Duration::from_secs(30)),
            outputs_valid: false,
            pane: Some("● Running: cargo test".to_string()),
            diagnostics: SessionDeathDiagnostics::default(),
            validate_calls: std::cell::Cell::new(0),
        };
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::Ok);
        assert!(!a.blocked_on_limit);
        assert!(a.events.is_empty());
    }

    #[test]
    fn assess_dead_session_takes_precedence_over_idle_mtime() {
        // A dead session with an idle transcript must resolve SessionDied, never
        // Stale/AutoComplete — decide checks liveness first, and assess_node must
        // preserve that ordering.
        let probes = FakeProbes {
            session_alive: false,
            jsonl_mtime: Some(SystemTime::now() - Duration::from_secs(500)),
            outputs_valid: false,
            pane: None,
            diagnostics: SessionDeathDiagnostics::default(),
            validate_calls: std::cell::Cell::new(0),
        };
        let a = assess(&probes, AutoCompletePolicy::Observe);
        assert_eq!(a.detection, Detection::SessionDied);
    }
}
