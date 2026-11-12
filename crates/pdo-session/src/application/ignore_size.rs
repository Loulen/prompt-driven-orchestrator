//! The `ignore-size` of a shared terminal's tmux clients (#867, ADR-0075),
//! applied live: a spectator's client carries it, flipped through the
//! [`Tmux`] port with `refresh-client -f`, never a re-attach. The presence
//! registry (`shared_terminal`) decides it; this follows its decisions.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::warn;

use crate::application::ports::Tmux;

/// Keep the tmux client of `client_pid` in line with the `ignore-size` wanted
/// for its connection, until the connection leaves (the sender is dropped).
///
/// The client may not be attached yet when a change lands (the flag was decided
/// before `tmux attach` spawned), so each application retries for a short while.
/// `watch` only ever yields the latest value: a quick flip-flop applies the final
/// state, never an out-of-order one.
pub async fn follow_ignore_size(
    tmux: Arc<dyn Tmux>,
    client_pid: u32,
    mut wanted: watch::Receiver<bool>,
) {
    while wanted.changed().await.is_ok() {
        let flag = *wanted.borrow_and_update();
        let port = tmux.clone();
        let applied =
            tokio::task::spawn_blocking(move || set_ignore_size(&*port, client_pid, flag))
                .await
                .unwrap_or(false);
        if !applied {
            warn!(
                "Could not set ignore-size={flag} on the tmux client of pid {client_pid} \
                 (socket {})",
                tmux.socket()
            );
        }
    }
}

/// `ignore-size` on the client whose process is `client_pid`, retried while the
/// client is not listed yet.
fn set_ignore_size(tmux: &dyn Tmux, client_pid: u32, flag: bool) -> bool {
    const ATTEMPTS: u32 = 30;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
        if tmux.set_client_ignore_size(client_pid, flag) {
            return true;
        }
    }
    false
}
