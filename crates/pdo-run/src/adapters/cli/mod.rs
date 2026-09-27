//! The agent CLI (`pdo complete`, `fail`, `skip`, `wait-user`, `run wait`): a
//! node session's way to the daemon. Every call carries the session headers,
//! so `identify` names the caller as the node's Agent (`Actor::Agent`).

mod complete;
mod wait;

use anyhow::{Context, Result};

use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

pub use complete::run_complete;
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
    let client = reqwest::blocking::Client::new();
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
    let client = reqwest::blocking::Client::new();
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
