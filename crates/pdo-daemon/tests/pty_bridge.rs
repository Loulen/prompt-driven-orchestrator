//! Layer 3a — PTY bridge WebSocket integration test.
//!
//! Substitutes Claude with `bash -c 'cat'` inside a tmux session, opens
//! `WS /sessions/<id>/pty`, sends bytes, and asserts roundtrip echo.

use std::time::Duration;

use crate::common::TestDaemon;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

fn tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// The daemon talks to a tmux server scoped to its own socket (`tmux -L`), so
// out-of-band session management must go through the same socket — a session
// created on the default server would be invisible to the PTY bridge.
fn create_tmux_session_with_cat(socket: &str, name: &str) {
    let status = std::process::Command::new("tmux")
        .args(["-L", socket, "new-session", "-d", "-s", name, "cat"])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
}

fn kill_tmux_session(socket: &str, name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["-L", socket, "kill-session", "-t", name])
        .status();
}

/// Layer 3a: open WS /sessions/<id>/pty, send bytes to `cat`, read them back.
#[tokio::test]
async fn pty_ws_roundtrip_echo() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // This test exercises the PTY bridge, not the reaper. The session below is
    // created out-of-band (no run in the event log), so an armed orphan sweep
    // kills it as an unrecognised `pdo-*` name (no TTL on that arm) and the
    // attached client prints `[exited]` instead of the echo — opt out of all
    // automatic cleanup for THIS daemon, per-daemon, never via process-global env
    // (a sibling's `remove_var` used to land inside this `spawn`, cf.
    // `TestDaemon::spawn_nested`).
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    let session_name = "pdo-pty-test-echo";
    kill_tmux_session(&socket, session_name);
    create_tmux_session_with_cat(&socket, session_name);

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let input = b"hello world\n";
    ws.send(Message::Binary(input.to_vec().into()))
        .await
        .expect("send should succeed");

    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let remaining = deadline - tokio::time::Instant::now();
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Binary(data)))) => {
                collected.push_str(&String::from_utf8_lossy(&data));
                if collected.contains("hello world") {
                    break;
                }
            }
            Ok(Some(Ok(_))) => {} // ignore non-binary frames
            _ => break,
        }
    }

    assert!(
        collected.contains("hello world"),
        "expected 'hello world' in PTY output, got: {collected:?}"
    );

    let _ = ws.close(None).await;
    kill_tmux_session(&socket, session_name);
}

/// Build a raw WS upgrade request carrying an explicit `Origin`, so a test can
/// forge the header a browser would send. `host` is the daemon's real address
/// (routing), `origin` is the value under test (the guard).
fn ws_upgrade_request(
    url: &str,
    host: &str,
    origin: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(url)
        .header("Host", host)
        .header("Origin", origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .unwrap()
}

/// Assert a handshake failed with an exact HTTP 403 (`Origin not allowed`),
/// not merely "some error" — a DNS/refused/timeout error would satisfy
/// `is_err()` while proving nothing about the origin guard.
fn assert_handshake_403(
    result: Result<
        (
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::handshake::client::Response,
        ),
        tokio_tungstenite::tungstenite::Error,
    >,
) {
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status(), 403, "expected 403 Origin refusal");
        }
        Ok(_) => panic!("expected HTTP 403 handshake refusal, got a successful upgrade"),
        Err(other) => panic!("expected HTTP 403 handshake refusal, got {other:?}"),
    }
}

/// Layer 3a: WS /sessions/<id>/pty rejects requests with bad Origin header.
#[tokio::test]
async fn pty_ws_rejects_bad_origin() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): a configured origin is accepted on the PTY upgrade. The
/// session doesn't exist, so the bridge collapses right after — but the
/// handshake has already returned 101, which is all this asserts (no I/O).
#[tokio::test]
async fn pty_ws_accepts_configured_origin() {
    let origin = "http://pdo.example:9999";
    let daemon =
        TestDaemon::spawn_with_allowed_ws_origins(|_repo| Ok(()), vec![origin.to_string()])
            .await
            .unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), origin);

    let (_ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("configured origin should complete the WS handshake");
    assert_eq!(resp.status(), 101, "configured origin should upgrade");
}

/// Layer 3a (#564): with an allowlist set, an origin NOT on it is still 403 on
/// the PTY upgrade.
#[tokio::test]
async fn pty_ws_rejects_origin_absent_from_allowlist() {
    let daemon = TestDaemon::spawn_with_allowed_ws_origins(
        |_repo| Ok(()),
        vec!["https://pdo.example.com".to_string()],
    )
    .await
    .unwrap();

    let ws_url = format!("ws://{}/sessions/fake-session/pty", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): `/ws` (dashboard event stream) rejects a bad Origin. This is
/// the one endpoint whose behaviour CHANGES — it had no guard before — so it had
/// zero coverage until now.
#[tokio::test]
async fn ws_events_rejects_bad_origin() {
    let daemon = TestDaemon::spawn(|_repo| Ok(())).await.unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), "http://evil.com");

    let result = tokio_tungstenite::connect_async(request).await;
    assert_handshake_403(result);
}

/// Layer 3a (#564): a configured origin is accepted on `/ws`, AND the event
/// stream still works — a bare 101 wouldn't prove the stream survived, so read
/// the first frame and assert the `{"type":"ready"}` handshake.
#[tokio::test]
async fn ws_events_accepts_configured_origin() {
    let origin = "https://pdo.example.com";
    let daemon =
        TestDaemon::spawn_with_allowed_ws_origins(|_repo| Ok(()), vec![origin.to_string()])
            .await
            .unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), origin);

    let (mut ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("configured origin should complete the /ws handshake");
    assert_eq!(resp.status(), 101, "configured origin should upgrade");

    let msg = ws
        .next()
        .await
        .expect("stream should yield a first frame")
        .expect("first frame should not be an error");
    let text = msg.into_text().expect("first frame should be text");
    let parsed: serde_json::Value =
        serde_json::from_str(&text).expect("first frame should be JSON");
    assert_eq!(
        parsed["type"], "ready",
        "the event stream should still open with a ready frame"
    );
}

/// Layer 3a (#564): the localhost default keeps working when a third-party
/// allowlist is set (additive, D2) — the layer-3b / Playwright guard-rail. The
/// port is ephemeral, so the "default" origin is built from the bound addr,
/// never a literal, and the allowlist entry is deliberately unrelated to it.
#[tokio::test]
async fn ws_default_localhost_origin_still_allowed_with_allowlist_set() {
    let daemon = TestDaemon::spawn_with_allowed_ws_origins(
        |_repo| Ok(()),
        vec!["https://pdo.example.com".to_string()],
    )
    .await
    .unwrap();

    let ws_url = format!("ws://{}/ws", daemon.addr);
    let default_origin = format!("http://127.0.0.1:{}", daemon.addr.port());
    let request = ws_upgrade_request(&ws_url, &daemon.addr.to_string(), &default_origin);

    let (_ws, resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("loopback origin should still upgrade with an allowlist set");
    assert_eq!(resp.status(), 101, "loopback default must remain allowed");
}

/// Parse `/proc/<pid>/stat` into `(comm, state, ppid)`. The comm field is
/// wrapped in parens and may itself contain spaces or parens, so split on the
/// LAST `)` rather than tokenising the whole line.
#[cfg(target_os = "linux")]
fn read_proc_stat(pid: u32) -> Option<(String, char, u32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat[open + 1..close].to_string();
    let mut fields = stat[close + 1..].split_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid: u32 = fields.next()?.parse().ok()?;
    Some((comm, state, ppid))
}

/// PIDs of all top-level `/proc` entries (child processes appear here; threads
/// of our own process do not — they live under `/proc/<pid>/task/`).
#[cfg(target_os = "linux")]
fn proc_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .collect()
}

/// PIDs of live `tmux attach` clients that are children of `me` and whose command
/// line mentions `session_name`. The session name is unique per test, so this
/// never collides with a sibling `#[test]` running in the same binary. A client
/// that was spawned but never reaped shows up here (pre-#495 it stays ALIVE,
/// because the reader task keeps a dup of the master fd open, so no SIGHUP ever
/// reaches it). Returning the PIDs (not a count) lets the caller pin the exact
/// child it forged and track *that* PID across the reap, instead of a
/// process-wide zombie tally shared with sibling tests.
#[cfg(target_os = "linux")]
fn attach_child_pids(me: u32, session_name: &str) -> Vec<u32> {
    proc_pids()
        .into_iter()
        .filter(|&pid| match read_proc_stat(pid) {
            Some((_comm, _state, ppid)) if ppid == me => {
                std::fs::read(format!("/proc/{pid}/cmdline"))
                    .map(|c| String::from_utf8_lossy(&c).contains(session_name))
                    .unwrap_or(false)
            }
            _ => false,
        })
        .collect()
}

/// Of `pids` (captured while the clients were alive and name-matched), the ones
/// that are STILL our `tmux` children — paired with their process state. A fully
/// reaped child has left `/proc` (the parent `wait()`ed it) and so is absent
/// here; a live orphan reports its run/sleep state; a `<defunct>` child reports
/// `'Z'`. `read_proc_stat` reads comm/state even for zombies (whose cmdline is
/// empty), so this works after close when a name filter no longer would. The
/// `ppid == me && comm ~ "tmux"` guard rejects a recycled PID that the OS handed
/// to some unrelated process after the reap — this measurement is scoped to
/// THIS test's own children, never a sibling's.
#[cfg(target_os = "linux")]
fn surviving_children(me: u32, pids: &[u32]) -> Vec<(u32, char)> {
    pids.iter()
        .filter_map(|&pid| match read_proc_stat(pid) {
            Some((comm, state, ppid)) if ppid == me && comm.contains("tmux") => Some((pid, state)),
            _ => None,
        })
        .collect()
}

/// Layer 3a (#495): after the PTY WebSocket closes, the daemon must reap the
/// `tmux attach` child it spawned — leaving neither a live orphan client nor a
/// `<defunct>` zombie.
///
/// The daemon runs in-process (`serve_with_config`), so the client it forks is
/// a child of THIS test process and is observable via `/proc`. Pre-fix the
/// client stays ALIVE after close (task 1 keeps a dup of the master fd, so no
/// SIGHUP reaches it), and its PID never leaves `/proc` — the poll below times
/// out and the test fails. That is the negative control.
///
/// Both assertions are scoped to the exact child PID(s) this test forged,
/// captured while the client is still alive. An earlier version tallied *all*
/// `<defunct>` tmux children of the shared test-process PID against a baseline;
/// because every `#[test]` in this binary runs in that one process, a sibling
/// test's transient zombie could land inside this test's measurement window and
/// trip the assertion — a false failure under the full parallel workspace run,
/// aggravated once #564 added sibling PTY tests here. Pinning the specific PID
/// removes that cross-test race without serialising the test.
///
/// Linux-only: the assertion reads `/proc`. On other platforms the test is
/// compiled out (there is no CI target for them).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn pty_ws_reaps_tmux_child_on_close() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    // Out-of-band session (no run in the event log) → opt out of the orphan
    // sweep so it can't race the test and kill the session/client for us. A
    // per-daemon flag, not `set_var`: this test's own `remove_var` is what used to
    // arm the sibling echo test's reaper (cf. `TestDaemon::spawn_nested`).
    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    let socket = daemon.tmux_socket();

    let session_name = "pdo-pty-test-reap";
    kill_tmux_session(&socket, session_name);
    create_tmux_session_with_cat(&socket, session_name);

    let me = std::process::id();

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    // Wait for the attach client to actually come up before acting — this is
    // both a positive control (proves the /proc probe sees the child) and a
    // guard against measuring the reap before the child even exists. Capture the
    // exact PID(s) now, while the client is alive and its cmdline still carries
    // the (unique) session name; after close a zombie's cmdline is empty, so the
    // name filter no longer applies and only this pinned PID lets us tell OUR
    // child apart from a sibling test's.
    let up_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut child_pids = attach_child_pids(me, session_name);
    while child_pids.is_empty() && tokio::time::Instant::now() < up_deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
        child_pids = attach_child_pids(me, session_name);
    }
    assert!(
        !child_pids.is_empty(),
        "tmux attach client never appeared as a child of the test process"
    );

    // Exchange a byte so the bridge is fully wired, then close — the close is
    // what must trigger the reap.
    ws.send(Message::Binary(b"x".to_vec().into()))
        .await
        .expect("send should succeed");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _ = ws.close(None).await;

    // The reap runs asynchronously after the bridge's `select!` returns; poll
    // until every child PID we forged has left `/proc` (parent `wait()`ed it).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline && !surviving_children(me, &child_pids).is_empty()
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Clean up before asserting so a failure can't leak the session.
    kill_tmux_session(&socket, session_name);

    // Classify whatever is left of OUR children only — scoped to `child_pids`,
    // never a process-wide count. A survivor in run/sleep state is a leaked live
    // orphan; one in `'Z'` is a leaked `<defunct>` zombie. Both must be empty.
    let survivors = surviving_children(me, &child_pids);
    let live: Vec<_> = survivors
        .iter()
        .filter(|(_, state)| *state != 'Z')
        .collect();
    let zombies: Vec<_> = survivors
        .iter()
        .filter(|(_, state)| *state == 'Z')
        .collect();
    assert!(
        live.is_empty(),
        "PTY bridge leaked a live `tmux attach` client after WS close (#495): {live:?}"
    );
    assert!(
        zombies.is_empty(),
        "PTY bridge leaked a `<defunct>` tmux zombie after WS close (#495): {zombies:?}"
    );
}

// --- #946: leaving a terminal writes nothing into it ---
//
// portable-pty 0.8.1's `Drop for UnixMasterWriter` writes `\n` + VEOF (`^D`)
// into the master. While the `tmux attach` client is alive that input is
// forwarded to the pane: Claude Code's prompt gained a blank line on every
// node switch, and a hosted shell died on the first detach. The bridge must
// release the writer only once the client is killed and reaped.

/// Out-of-band pane that records every byte it receives, verbatim: the pane's
/// tty is put in raw mode (no line discipline, no echo) so a `\n` or a `^D`
/// lands in `record` instead of being interpreted. `cat` opens `record` only
/// after `stty` ran, so its existence means the recorder is ready.
fn create_recording_session(socket: &str, name: &str, record: &std::path::Path) {
    let script = format!("stty raw -echo; exec cat > '{}'", record.display());
    let status = std::process::Command::new("tmux")
        .args([
            "-L",
            socket,
            "new-session",
            "-d",
            "-s",
            name,
            "bash",
            "-c",
            &script,
        ])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
}

async fn wait_for_path(path: &std::path::Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !path.exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(path.exists(), "recording pane never came up");
}

fn session_alive(socket: &str, name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["-L", socket, "has-session", "-t", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// How the test leaves the terminal — one per exit path of the bridge.
#[derive(Clone, Copy, Debug)]
enum Detach {
    /// The front closes the WebSocket (node switch, Shell closed).
    CleanClose,
    /// The socket vanishes without a close frame (page reload, tab killed).
    Abandon,
    /// The tmux client goes away first (`detach-client`), socket still open.
    ClientEnds,
}

async fn attach_then_detach(
    daemon: &crate::common::TestDaemon,
    socket: &str,
    session_name: &str,
    how: Detach,
) {
    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    // Wait for the client's first repaint: the bridge is fully wired.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Binary(_)))) => break,
            Ok(Some(Ok(_))) => {}
            other => panic!("no PTY output before detaching: {other:?}"),
        }
    }

    match how {
        Detach::CleanClose => {
            let _ = ws.close(None).await;
            drop(ws);
        }
        Detach::Abandon => drop(ws),
        Detach::ClientEnds => {
            let status = std::process::Command::new("tmux")
                .args(["-L", socket, "detach-client", "-s", session_name])
                .status()
                .expect("failed to run tmux");
            assert!(status.success(), "tmux detach-client should succeed");
            // The bridge closes the socket once its client is gone.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                match tokio::time::timeout(remaining, ws.next()).await {
                    Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => break,
                    Ok(Some(Ok(_))) => {}
                    Err(_) => panic!("bridge never closed the socket after its client ended"),
                }
            }
            drop(ws);
        }
    }

    // Leave the bridge time to tear down: kill (~250ms grace) + reap, then the
    // writer's release.
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

async fn assert_detach_writes_nothing(how: Detach, session_name: &str) {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    // Out-of-band session → opt out of the orphan sweep (cf. the echo test).
    let daemon = crate::common::TestDaemon::spawn_nested(|_repo| Ok(()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let dir = tempfile::tempdir().unwrap();
    let record = dir.path().join("pane-input.bin");

    kill_tmux_session(&socket, session_name);
    create_recording_session(&socket, session_name, &record);
    wait_for_path(&record).await;

    attach_then_detach(&daemon, &socket, session_name, how).await;

    let received = std::fs::read(&record).unwrap();
    kill_tmux_session(&socket, session_name);
    assert!(
        received.is_empty(),
        "leaving the terminal ({how:?}) wrote {received:?} into the pane (#946)"
    );
}

/// #946: a clean close of the WebSocket writes no byte into the pane.
#[tokio::test]
async fn pty_ws_clean_close_writes_nothing_into_pane() {
    assert_detach_writes_nothing(Detach::CleanClose, "pdo-pty-test-quiet-close").await;
}

/// #946: an abandoned socket (no close frame) writes no byte into the pane.
#[tokio::test]
async fn pty_ws_abandoned_socket_writes_nothing_into_pane() {
    assert_detach_writes_nothing(Detach::Abandon, "pdo-pty-test-quiet-abandon").await;
}

/// #946: the tmux client ending first writes no byte into the pane either.
#[tokio::test]
async fn pty_ws_client_end_writes_nothing_into_pane() {
    assert_detach_writes_nothing(Detach::ClientEnds, "pdo-pty-test-quiet-client-end").await;
}

/// #946: a hosted shell (no rc, no IGNOREEOF) is still alive after an attach
/// then a detach through the bridge — pre-fix the `^D` made it exit, taking
/// the tmux session with it.
#[tokio::test]
async fn pty_ws_shell_survives_detach() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }
    let daemon = crate::common::TestDaemon::spawn_nested(|_repo| Ok(()))
        .await
        .unwrap();
    let socket = daemon.tmux_socket();
    let session_name = "pdo-pty-test-shell-survives";

    kill_tmux_session(&socket, session_name);
    let status = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "new-session",
            "-d",
            "-s",
            session_name,
            "env -u IGNOREEOF bash --norc --noprofile",
        ])
        .status()
        .expect("failed to run tmux");
    assert!(status.success(), "tmux new-session should succeed");
    // Let bash reach its prompt: a `^D` before readline is up would not count.
    tokio::time::sleep(Duration::from_millis(500)).await;

    attach_then_detach(&daemon, &socket, session_name, Detach::CleanClose).await;

    let alive = session_alive(&socket, session_name);
    kill_tmux_session(&socket, session_name);
    assert!(
        alive,
        "the hosted shell died when the terminal was left (#946)"
    );
}

/// #972: the daemon pings an open terminal socket on its own, so a reverse proxy
/// never sees it idle while the agent inside is silent — and sends a `heartbeat`
/// text frame with each ping, the beat the browser's watchdog can actually see.
#[tokio::test]
async fn pty_ws_receives_periodic_pings_and_heartbeats() {
    if !tmux_available() {
        eprintln!("tmux not on PATH — skipping");
        return;
    }

    let daemon = TestDaemon::spawn_nested(|_repo| Ok(())).await.unwrap();
    daemon.set_pty_ping_interval(Duration::from_millis(200));
    let socket = daemon.tmux_socket();

    let session_name = "pdo-pty-test-ping";
    kill_tmux_session(&socket, session_name);
    create_tmux_session_with_cat(&socket, session_name);

    let ws_url = format!("ws://{}/sessions/{}/pty", daemon.addr, session_name);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect should succeed");

    let mut pings = 0;
    let mut heartbeats = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while pings < 2 || heartbeats < 2 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Ping(_)))) => pings += 1,
            Ok(Some(Ok(Message::Text(text)))) if text.as_str() == r#"{"type":"heartbeat"}"# => {
                heartbeats += 1
            }
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }

    assert!(
        pings >= 2,
        "expected periodic pings on the PTY socket, got {pings}"
    );
    assert!(
        heartbeats >= 2,
        "expected periodic heartbeat text frames on the PTY socket, got {heartbeats}"
    );

    let _ = ws.close(None).await;
    kill_tmux_session(&socket, session_name);
}
