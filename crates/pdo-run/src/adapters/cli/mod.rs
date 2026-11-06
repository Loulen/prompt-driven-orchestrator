//! The run context's CLI. The agent's (`pdo complete`, `fail`, `skip`,
//! `wait-user`, `run wait`) is a node session's way to the daemon: every call
//! carries the session headers, so `identify` names the caller as the node's
//! Agent (`Actor::Agent`). `pdo run create` creates a Run (a child of the
//! node's Run when called from a session, ADR-0064), `pdo run status` reads one
//! Run in a line, `pdo reap` reclaims the disk of old terminal Runs. The
//! operator's gestures on a Run (`pdo run cancel|retry|reopen|archive|…`,
//! [`RunGesture`]) and on a node (`pdo node`, [`NodeAction`]) are thin clients
//! of the shared one (`pdo_platform::adapters::cli::daemon`).

mod complete;
mod node;
mod node_read;
mod reap;
mod run_action;
mod run_create;
mod run_gesture;
mod run_list;
mod run_pipeline_cli;
mod run_read;
mod run_render;
mod run_status;
mod upload;
mod wait;

use anyhow::{Context, Result};

use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

pub use complete::run_complete;
pub use node::{run_node, NodeAction};
pub use reap::run_reap;
pub use run_action::RunAction;
pub use run_create::run_run_create;
pub use run_gesture::{run_run_gesture, RunGesture};
pub use run_list::run_run_list;
pub use run_read::{run_run_read, RunRead};
pub use run_status::{run_run_status, RunStatusLine, EXIT_RUN_NOT_FOUND};
pub use wait::{run_run_wait, run_wait_user, EXIT_WAIT_TIMEOUT};

pub fn cli_daemon_url() -> String {
    std::env::var("PDO_DAEMON_URL")
        .unwrap_or_else(|_| pdo_kernel::env::DEFAULT_DAEMON_URL.to_string())
}

pub fn cli_run_id() -> Result<String> {
    std::env::var("PDO_RUN_ID")
        .context("PDO_RUN_ID not set — this command must be run inside a PDO NodeRun session")
}

pub fn cli_node_id() -> Result<String> {
    std::env::var("PDO_NODE_ID")
        .context("PDO_NODE_ID not set — this command must be run inside a PDO NodeRun session")
}

pub fn cli_node_iter() -> i64 {
    std::env::var("PDO_NODE_ITER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// Exit code of `pdo complete` when the completion was **refused and the node is
/// still yours** (ADR-0035 §4): fix the artefacts and call it again. Nothing
/// terminal is recorded, so `pdo fail` would be wrong.
pub const EXIT_REFUSED_RECOVERABLE: u8 = 3;

/// Exit code of `pdo complete` when the completion was **refused and the runtime
/// has already ruled** (ADR-0035 §4). The daemon appended the terminal event
/// itself; `pdo fail` would double it — and `RunFailed` is not guarded, so the
/// second one lands with a false reason. Read by a `||` in a `script` node's bash
/// tail (`pdo_session::domain::launch::build_script_tail`).
pub const EXIT_REFUSED_TERMINAL: u8 = 4;

pub fn run_fail(reason: String) -> Result<()> {
    let url = cli_daemon_url();
    let rid = cli_run_id()?;
    let nid = cli_node_id()?;
    let iter = cli_node_iter();

    let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/fail");
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default();
    let resp = client
        .post(&endpoint)
        .header(SESSION_RUN_HEADER, &rid)
        .header(SESSION_NODE_HEADER, &nid)
        .json(&serde_json::json!({ "reason": reason, "iter": iter }))
        .send()
        .context("failed to reach daemon")?;

    if resp.status().is_success() {
        eprintln!("Node {nid} marked failed.");
    } else {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {body}");
    }
    Ok(())
}

/// Graceful no-op: mark the current node done-but-skipped and end the run as
/// `skipped` rather than `failed`. Short-circuits downstream (the run reaches no
/// `end` node) so a fired-into-an-empty-pool run stays out of the failure signal.
pub fn run_skip(reason: String) -> Result<()> {
    let url = cli_daemon_url();
    let rid = cli_run_id()?;
    let nid = cli_node_id()?;
    let iter = cli_node_iter();

    let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/skip");
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default();
    let resp = client
        .post(&endpoint)
        .header(SESSION_RUN_HEADER, &rid)
        .header(SESSION_NODE_HEADER, &nid)
        .json(&serde_json::json!({ "reason": reason, "iter": iter }))
        .send()
        .context("failed to reach daemon")?;

    if resp.status().is_success() {
        eprintln!("Node {nid} skipped (graceful no-op); run marked skipped.");
    } else {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {body}");
    }
    Ok(())
}

/// The Session hébergée this CLI process runs in (ADR-0092): a
/// `PDO_SESSION_ID` that names one, outside any node session.
pub fn hosted_session_claim() -> Option<String> {
    if session_env_claim().is_some() {
        return None;
    }
    let id = std::env::var("PDO_SESSION_ID").ok()?.trim().to_string();
    id.starts_with(pdo_session::domain::names::HOSTED_SESSION_PREFIX)
        .then_some(id)
}

/// `pdo complete`, `fail` or `skip` (`command`) from a Session hébergée: a
/// refusal that says why, with the exit code of a refusal that leaves the
/// turn to the caller (`3`). `None` outside a Session hébergée.
pub fn refused_in_hosted_session(command: &str) -> Option<std::process::ExitCode> {
    let id = hosted_session_claim()?;
    eprintln!(
        "pdo {command} REFUSED — hosted_session: {id} is a hosted session, outside any Run. \
         It is never completed, failed or skipped: it ends when someone terminates it.\n\
         Nothing changed; the session goes on."
    );
    Some(std::process::ExitCode::from(EXIT_REFUSED_RECOVERABLE))
}

/// The session identity this CLI process carries via the `PDO_RUN_ID` /
/// `PDO_NODE_ID` env vars every node session is wrapped with
/// (`pdo_session::domain::launch::wrap_with_env`). Only a non-empty PAIR is a claim —
/// a lone var means out of session (mirror of the daemon's
/// `session_claim_from_headers`): the created Run is then a root.
pub fn session_env_claim() -> Option<(String, String)> {
    let clean = |v: Result<String, std::env::VarError>| {
        v.ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let run_id = clean(std::env::var("PDO_RUN_ID"))?;
    let node_id = clean(std::env::var("PDO_NODE_ID"))?;
    Some((run_id, node_id))
}
