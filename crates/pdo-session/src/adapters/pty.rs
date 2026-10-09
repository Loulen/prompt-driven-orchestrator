//! PTY bridge (the terminal's driver): spawns `tmux attach -t <session>` inside
//! a pseudo-terminal and bridges byte I/O between the PTY and a WebSocket.
//!
//! Upgrade: `WS /sessions/{id}/pty?poste=<id>`. The optional `poste` names the
//! browser (#867, ADR-0075); a socket without one counts as a poste of its own.
//!
//! Protocol (WS → daemon):
//! - Binary frames → stdin of the PTY (user keystrokes), dropped while the
//!   connection is a spectator
//! - Text frames with JSON `{"type":"resize","cols":N,"rows":N}` → PTY resize
//! - Text frames with JSON `{"type":"take_control"}` → a spectator takes the hand
//!   (#870); ignored from the pilot or a solo terminal
//!
//! Protocol (daemon → WS):
//! - Binary frames ← stdout of the PTY (terminal output)
//! - Text frames with JSON `{"type":"role","role":"solo"|"pilot"|"spectator",…}`
//!   on every role change (see [`RoleMsg`](crate::domain::shared_terminal::RoleMsg))
//! - Every [`terminal_beat_interval`](crate::Session::terminal_beat_interval)
//!   (ADR-0083 §5): a Ping frame, so no reverse proxy drops a terminal whose
//!   agent is silent, then the Text frame [`HEARTBEAT_FRAME`]: the browser
//!   never sees pings, and this beat is how it tells a silent terminal from a
//!   dead connection (half-open TCP, network cut without a close)
//!
//! The client's Close frame is answered before the connection ends (ADR-0083
//! §3); the opening and the closing have their lines in the journal
//! ([`ws_log`](crate::adapters::http::ws_log)).

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use pdo_kernel::Actor;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, warn};

use crate::adapters::http::ws_log::{self, Closing, NO_STATUS};

use crate::domain::pty_protocol::{decode_resize, is_take_control, HEARTBEAT_FRAME};
use crate::domain::shared_terminal::{TermSize, INITIAL_SIZE};
use crate::Session;

/// Keystrokes to the PTY through the shared writer (#946). A poisoned lock
/// (a panic mid-write) ends the input loop like a failed write would.
fn write_input(
    writer: &std::sync::Mutex<Box<dyn Write + Send>>,
    data: &[u8],
) -> std::io::Result<()> {
    writer
        .lock()
        .map_err(|_| std::io::Error::other("PTY writer poisoned"))?
        .write_all(data)
}

/// Bridge an upgraded socket to `session_id` until either side closes, with
/// its lines in the journal; `to_upgrade` is the time the upgrade took.
pub(crate) async fn handle_pty_ws(
    socket: WebSocket,
    session_id: String,
    poste: Option<String>,
    actor: Actor,
    session: Arc<Session>,
    to_upgrade: Duration,
) {
    let path = format!("/sessions/{session_id}/pty");
    ws_log::opened(&path, to_upgrade);
    let opened = Instant::now();
    let closing = bridge(socket, session_id, poste, actor, session).await;
    ws_log::closed(&path, closing, opened.elapsed());
}

/// End a socket the daemon cannot bridge with its own Close: a socket left
/// without one reads as a dropped daemon (`1006`), which the interface reopens.
async fn give_up(mut socket: WebSocket) -> Closing {
    let _ = socket.send(Message::Close(None)).await;
    Closing::ByDaemon
}

async fn bridge(
    socket: WebSocket,
    session_id: String,
    poste: Option<String>,
    actor: Actor,
    session: Arc<Session>,
) -> Closing {
    let terminals = session.terminals();

    let pty_system = native_pty_system();
    let initial_size = PtySize {
        rows: INITIAL_SIZE.rows,
        cols: INITIAL_SIZE.cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = match pty_system.openpty(initial_size) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to open PTY: {e}");
            return give_up(socket).await;
        }
    };

    // #867 / ADR-0075: join the terminal's presence before attaching, so a
    // spectator's client is born with `ignore-size` and never weighs on the
    // window, not even for the instant before a `refresh-client` would land.
    let (role_tx, mut role_rx) = mpsc::unbounded_channel::<String>();
    let mut membership = terminals.registry().join(&session_id, poste, role_tx);
    let conn_id = membership.id;
    let attach_ignoring_size = *membership.ignore_size.borrow_and_update();

    // Pinned to the daemon's private socket so we don't accidentally reach into
    // another pdo daemon's tmux state on the same host.
    let argv = terminals.attach_argv(&session_id, attach_ignoring_size);
    let mut cmd = CommandBuilder::new(&argv[0]);
    cmd.args(&argv[1..]);
    // The consumer at the other end of this PTY is xterm.js in the browser, so
    // declare that terminal type explicitly instead of inheriting the daemon's
    // ambient TERM. A daemon started headless (systemd, container, CI, nohup)
    // has TERM unset or `dumb`, which makes `tmux attach` abort with
    // "open terminal failed: terminal does not support clear" — the inline
    // manager/node terminal then shows that error instead of the session.
    cmd.env("TERM", "xterm-256color");

    // Acquire the master-side reader and writer BEFORE spawning the child.
    // Both are fallible; taking them first means that once the child exists the
    // only path out of this function is the select!/reap below — no early
    // return can drop an unreaped child (#495).
    let mut pty_reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to clone PTY reader: {e}");
            terminals.registry().leave(&session_id, conn_id);
            return give_up(socket).await;
        }
    };
    let pty_writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to take PTY writer: {e}");
            terminals.registry().leave(&session_id, conn_id);
            return give_up(socket).await;
        }
    };

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn tmux attach for {session_id}: {e}");
            terminals.registry().leave(&session_id, conn_id);
            return give_up(socket).await;
        }
    };

    // CORE-61: the first to drive the terminal took control of it, once its
    // `tmux attach` runs.
    if membership.took_the_hand {
        terminals.joined_as_driver(&actor, &session_id);
    }

    // Role changes after the attach flip the client's `ignore-size` live. The
    // task ends on its own when the connection leaves the registry.
    let flags_handle = child
        .process_id()
        .map(|pid| tokio::spawn(terminals.follow_ignore_size(pid, membership.ignore_size.clone())));
    let spectator = membership.spectator.clone();

    drop(pair.slave);

    let master = pair.master;
    let (mut ws_sink, mut ws_stream) = socket.split();

    // #946: leaving a terminal writes nothing into it. portable-pty's
    // `Drop for UnixMasterWriter` writes `\n` + VEOF (`^D`) into the master;
    // released while the `tmux attach` client is alive, the client forwards
    // them to the pane (a blank line in Claude Code's prompt on every node
    // switch, a hosted shell killed). So the writer is shared, never owned, by
    // the input task: whatever way that task ends (close, abandoned socket,
    // write error, panic, or still running when another task wins the
    // select!), it only drops a handle. The last one goes after the reap below,
    // when nothing forwards the bytes any more.
    let pty_writer = Arc::new(std::sync::Mutex::new(pty_writer));
    let input_writer = Arc::clone(&pty_writer);

    // Channel: PTY stdout → async sender → WebSocket
    let (pty_tx, mut pty_rx) = mpsc::channel::<Vec<u8>>(64);

    // Task 1: blocking read from PTY, send chunks through channel
    let read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pty_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if pty_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // The input task tells the output task that the client sent its Close:
    // the answer goes out through the sink, which the output task owns.
    let (client_closed_tx, mut client_closed_rx) = oneshot::channel::<()>();

    // Task 2: forward PTY output and role frames to the WebSocket, and beat it
    // (ADR-0083 §5): an agent that thinks for minutes leaves the socket silent,
    // and a reverse proxy closes a silent WebSocket once its read timeout runs
    // out. The browser answers the Ping on its own but cannot see it: the
    // heartbeat Text frame is what its silence delay reads.
    let beat_every = session.terminal_beat_interval();
    let ws_send_handle = tokio::spawn(async move {
        let mut beat =
            tokio::time::interval_at(tokio::time::Instant::now() + beat_every, beat_every);
        loop {
            let msg = tokio::select! {
                data = pty_rx.recv() => match data {
                    Some(data) => Message::Binary(data.into()),
                    None => break,
                },
                Some(frame) = role_rx.recv() => Message::Text(frame.into()),
                _ = beat.tick() => {
                    if ws_sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        return Closing::Dropped;
                    }
                    Message::Text(HEARTBEAT_FRAME.into())
                }
                _ = &mut client_closed_rx => break,
            };
            if ws_sink.send(msg).await.is_err() {
                return Closing::Dropped;
            }
        }
        // Answers the client's Close when it sent one (queued by the
        // protocol), sends the daemon's own otherwise.
        let _ = ws_sink.close().await;
        Closing::ByDaemon
    });

    // Task 3: read from WebSocket, write to PTY stdin (+ handle resize)
    let input_session = session_id.clone();
    let input_context = session.clone();
    let ws_recv_handle = tokio::spawn(async move {
        let terminals = input_context.terminals();
        let mut closing = Closing::Dropped;
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                // ADR-0075 §3: a spectator is read-only. Its keystrokes (and the
                // mouse reports that travel the same way) never reach the pane,
                // and so never lift a declared wait either.
                Message::Binary(_) if spectator.load(std::sync::atomic::Ordering::SeqCst) => {}
                Message::Binary(data) if write_input(&input_writer, &data).is_err() => {
                    break;
                }
                Message::Binary(data) => {
                    // #588 / ADR-0069 §1: a human keystroke crossing the bridge is
                    // the observable that lifts a declared wait — an Enter after at
                    // least one typed byte since the declaration. Written to the
                    // PTY first (above), then read: the bridge never delays input.
                    terminals.note_input(&actor, &input_session, &data).await;
                }
                Message::Text(text) => {
                    if let Some(resize) = decode_resize(&text) {
                        let new_size = PtySize {
                            rows: resize.rows,
                            cols: resize.cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        };
                        if let Err(e) = master.resize(new_size) {
                            warn!("PTY resize failed: {e}");
                        }
                        terminals.registry().resize(
                            &input_session,
                            conn_id,
                            TermSize {
                                cols: resize.cols,
                                rows: resize.rows,
                            },
                        );
                    } else if is_take_control(&text) {
                        // #870 / ADR-0075: only a spectator's request does
                        // anything; the registry flips the roles, the `ignore-size`
                        // of both postes' clients and the read-only filter.
                        terminals
                            .take_control(&actor, &input_session, conn_id)
                            .await;
                    } else {
                        // Unknown / malformed control frames must NEVER be
                        // written to the PTY as user input — that would inject
                        // stray characters into whatever has the focus inside
                        // tmux (Claude Code's prompt, an editor, etc.). User
                        // keystrokes always travel as Binary frames; Text
                        // frames are reserved for our JSON control protocol.
                        warn!(
                            "Ignoring unrecognized text frame on PTY socket \
                             ({} bytes)",
                            text.len()
                        );
                    }
                }
                Message::Close(frame) => {
                    closing = Closing::ByClient(frame.map_or(NO_STATUS, |f| f.code));
                    let _ = client_closed_tx.send(());
                    break;
                }
                _ => {}
            }
        }
        closing
    });

    // Wait for any task to finish, then clean up
    // The client's Close first: it also ends the output task at once.
    let closing = tokio::select! {
        biased;
        closing = ws_recv_handle => closing.unwrap_or(Closing::Dropped),
        closing = ws_send_handle => closing.unwrap_or(Closing::Dropped),
        _ = read_handle => Closing::ByDaemon,
    };

    // Leave before the reap: the hand passes on (and the remaining clients'
    // `ignore-size` flips) while this client is still being torn down.
    terminals.registry().leave(&session_id, conn_id);
    if let Some(handle) = flags_handle {
        handle.abort();
    }

    // Reap the `tmux attach` child (#495). portable_pty's Child, like std's,
    // does NOT wait() on drop, so before this every closed pane leaked a
    // `[tmux: client] <defunct>` for the daemon's whole lifetime. A bare wait()
    // here is NOT enough: try_clone_reader()/take_writer() each dup() the master
    // fd, so on a plain socket close the reader task still holds one open, the
    // client has NOT received SIGHUP, and wait() would block forever. kill()
    // signals the client PID directly (SIGHUP, then SIGKILL after a ~250ms
    // grace) regardless of fd state; wait() then reaps it. Killing the client
    // also closes the slave, unblocking the reader task. Run on a blocking
    // thread so the grace period can't stall the async runtime.
    let _ = tokio::task::spawn_blocking(move || {
        let _ = child.kill();
        let _ = child.wait();
    })
    .await;

    // #946: only now, with the client reaped, may the writer's `\n^D` fire:
    // it lands on a PTY nobody reads. A still-running input task holds the
    // other handle and releases it just as harmlessly when its socket ends.
    drop(pty_writer);

    closing
}
