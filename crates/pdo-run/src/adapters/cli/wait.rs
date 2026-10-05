//! `pdo wait-user` and `pdo run wait`: declaring a wait on the user, and
//! waiting on the node's child runs.

use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

use super::*;
use crate::application::children_wait::CHILDREN_WAIT_SERVER_MAX_SECS;

/// Exit code of `pdo run wait` when `--timeout` elapsed with no child settled
/// (ADR-0069 §3). Not a failure: the agent loops and calls again.
pub const EXIT_WAIT_TIMEOUT: u8 = 2;

/// `pdo wait-user [--message]` — declare the wait (#588 / ADR-0069 §1). Exit `0`
/// declared or legal no-op · `3` refused by name (script node, no live session:
/// still your turn, nothing terminal happened) · `1` daemon unreachable.
pub fn run_wait_user(message: Option<String>) -> std::process::ExitCode {
    let (rid, nid) = match (cli_run_id(), cli_node_id()) {
        (Ok(r), Ok(n)) => (r, n),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("pdo wait-user FAILED — {e:#}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let url = cli_daemon_url();
    let iter = cli_node_iter();
    let message = message
        .as_deref()
        .map(pdo_session::domain::declared_wait::truncate_message)
        .filter(|m| !m.is_empty());
    let resp = match reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default()
        .post(format!("{url}/runs/{rid}/nodes/{nid}/wait-user"))
        .header(SESSION_RUN_HEADER, &rid)
        .header(SESSION_NODE_HEADER, &nid)
        .json(&serde_json::json!({ "iter": iter, "message": message }))
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pdo wait-user FAILED to reach the daemon — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let status = resp.status();
    let raw = resp.text().unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    if status.is_success() {
        let noop = body.get("noop").and_then(|v| v.as_bool()) == Some(true);
        let refreshed = body.get("refreshed").and_then(|v| v.as_bool()) == Some(true);
        let what = if noop {
            "already awaiting the user — nothing to do (legal no-op)"
        } else if refreshed {
            "still awaiting the user — the banner now shows your new message"
        } else {
            "now awaiting the user"
        };
        eprintln!(
            "Node {nid} is {what}.{}\nThe wait lifts when the user answers in the PDO terminal \
             (Enter) or releases completion. Keep the conversation going; never block on it.",
            message
                .as_deref()
                .map(|m| format!(" Banner: “{m}”"))
                .unwrap_or_default()
        );
        return std::process::ExitCode::SUCCESS;
    }
    if status.is_server_error() {
        eprintln!("pdo wait-user FAILED to get a verdict — daemon returned {status}: {raw}");
        return std::process::ExitCode::FAILURE;
    }
    let slug = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let detail = body
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or(raw.as_str());
    eprintln!(
        "pdo wait-user REFUSED — {slug}: {detail}.\nNothing changed; the node is still yours. \
         Do NOT run `pdo fail`."
    );
    std::process::ExitCode::from(EXIT_REFUSED_RECOVERABLE)
}

/// `pdo run wait [--all] [--timeout <s>]` — the blocking pull on the children
/// long-poll (#588 / ADR-0069 §3). Exit `0` with one JSON line per settled child
/// (or a `noop` line when no child was active) · `2` timeout, nothing on stdout ·
/// `1` unreachable daemon or not in a node session.
pub fn run_run_wait(all: bool, timeout: Option<u64>) -> std::process::ExitCode {
    let Some((run_id, node_id)) = session_env_claim() else {
        eprintln!(
            "pdo run wait REFUSED — not_in_node_session: this command only makes sense from a \
             PDO node session (PDO_RUN_ID / PDO_NODE_ID are not both set)."
        );
        return std::process::ExitCode::FAILURE;
    };
    let url = cli_daemon_url();
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .timeout(std::time::Duration::from_secs(
            CHILDREN_WAIT_SERVER_MAX_SECS + 15,
        ))
        .build()
        .expect("reqwest client");
    let deadline = timeout.map(|t| std::time::Instant::now() + std::time::Duration::from_secs(t));
    // `since` comes from the daemon's clock (echoed back on every timeout), so a
    // child that settles between two polls is still caught and no skew applies.
    let mut since: Option<String> = None;
    loop {
        let mut server_wait = CHILDREN_WAIT_SERVER_MAX_SECS;
        if let Some(d) = deadline {
            let remaining = d.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() && since.is_some() {
                return std::process::ExitCode::from(EXIT_WAIT_TIMEOUT);
            }
            server_wait = server_wait.min(remaining.as_secs().max(1));
        }
        let mut req = client
            .get(format!("{url}/runs/{run_id}/nodes/{node_id}/children/wait"))
            .query(&[
                ("all", all.to_string()),
                ("timeout_secs", server_wait.to_string()),
            ])
            .header(SESSION_RUN_HEADER, &run_id)
            .header(SESSION_NODE_HEADER, &node_id);
        if let Some(s) = &since {
            req = req.query(&[("since", s)]);
        }
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("pdo run wait FAILED to reach the daemon — {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        let status = resp.status();
        let raw = resp.text().unwrap_or_default();
        let body: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            let slug = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or(raw.as_str());
            eprintln!("pdo run wait REFUSED — daemon returned {status}: {slug}");
            return std::process::ExitCode::FAILURE;
        }
        if body.get("noop").and_then(|v| v.as_bool()) == Some(true) {
            println!(
                "{}",
                serde_json::json!({
                    "noop": true,
                    "reason": body.get("reason").cloned().unwrap_or(serde_json::Value::Null),
                })
            );
            return std::process::ExitCode::SUCCESS;
        }
        if let Some(children) = body.get("children").and_then(|v| v.as_array()) {
            if !children.is_empty() {
                for child in children {
                    println!("{child}");
                }
                return std::process::ExitCode::SUCCESS;
            }
        }
        // Server-side timeout: carry its clock forward and poll again, unless the
        // caller's own deadline has passed.
        since = body
            .get("since")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or(since);
        if let Some(d) = deadline {
            if std::time::Instant::now() >= d {
                return std::process::ExitCode::from(EXIT_WAIT_TIMEOUT);
            }
        }
    }
}
