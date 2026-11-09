//! Why a session died, as far as the host can tell: the memory pressure and
//! the sessions that died with it.

use crate::domain::event;
use crate::domain::state::{self, NodeStatus};

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
    /// The **hard error** the node's harness journal carries (#615, ADR-0052), if
    /// any. Set for a harness that **exits 0 on a hard failure** (`copilot`): the
    /// exit code is not a verdict, so PDO reads the failure off the journal and
    /// names it here, in the `NodeFailed` payload, instead of reporting only the
    /// symptom ("session died"). `None` for `claude` (whose death is its own
    /// signal) and for any harness whose journal shows no trailing error.
    pub harness_error: Option<String>,
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
            "harness_error": self.harness_error,
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
pub fn attach_diagnostics(events: &mut [event::Event], diag: &SessionDeathDiagnostics) {
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
/// the session context's `Tmux::session_exists`.
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

/// The nodes whose session the sweep probes: each `Running` one, and each
/// `AwaitingUser` one whose agent waits in its session for its user (a declared
/// wait, a refused unreleased completion) — `true` for those, probed for the
/// death of their session only: their turn ended on a question, it never
/// completes them. A node parked on its children has no session to probe.
pub fn session_holding_nodes(run_state: &state::RunState) -> Vec<(String, i64, bool)> {
    run_state
        .nodes
        .iter()
        .filter_map(|(id, ns)| match ns.status {
            NodeStatus::Running => Some((id.clone(), ns.iter, false)),
            NodeStatus::AwaitingUser
                if ns.awaiting.as_ref().is_some_and(|a| {
                    crate::domain::declared_wait::opens_declared_wait(&a.cause)
                }) =>
            {
                Some((id.clone(), ns.iter, true))
            }
            _ => None,
        })
        .collect()
}

pub fn running_nodes(run_state: &state::RunState) -> Vec<(String, i64)> {
    run_state
        .nodes
        .iter()
        .filter(|(_, ns)| ns.status == NodeStatus::Running)
        .map(|(id, ns)| (id.clone(), ns.iter))
        .collect()
}
