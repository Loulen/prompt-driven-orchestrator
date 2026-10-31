//! The host reads taken the moment a node's session is found dead: the tmux
//! server, the memory, the sessions that died with it. Shaped into the event
//! payload by [`stale_detector::SessionDeathDiagnostics`].

use crate::application::stale::detector as stale_detector;

/// Gather best-effort diagnostic context the moment a node session is found dead.
/// All I/O lives here; the pure shaping into the event payload is
/// [`stale_detector::SessionDeathDiagnostics`]. The most discriminating fact is
/// `tmux_server_alive`: a dead server means the whole socket collapsed and every
/// session under it died at once, rather than this one node dying in isolation.
pub(super) fn gather_session_death_diagnostics(
    tmux: &dyn pdo_session::application::ports::Tmux,
    run_id: &str,
    node_id: &str,
    iter: i64,
    running: &[(String, i64)],
) -> stale_detector::SessionDeathDiagnostics {
    let tmux_server_alive = tmux.server_alive();

    let (mem_available_kb, swap_free_kb) = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .map(|c| stale_detector::parse_meminfo(&c))
        .unwrap_or((None, None));

    let correlated_deaths =
        stale_detector::count_correlated_deaths(running, (node_id, iter), |id, it| {
            let s = pdo_session::domain::names::node_session_name(run_id, id, it);
            !tmux.session_exists(&s)
        });

    stale_detector::SessionDeathDiagnostics {
        tmux_server_alive,
        mem_available_kb,
        swap_free_kb,
        correlated_deaths,
        // Filled by `assess_node` after this impure gather; this function does the
        // tmux/proc reads only.
        harness_error: None,
    }
}
