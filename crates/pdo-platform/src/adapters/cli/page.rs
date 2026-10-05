//! `pdo page` — mount, list and unmount pages through the running daemon.

use anyhow::{Context, Result};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

use super::PageAction;
use crate::domain::page_mount::PageMount;
use crate::domain::service_unit::DEFAULT_DAEMON_URL;

/// The node session this CLI runs in, when it does: both `PDO_RUN_ID` and
/// `PDO_NODE_ID`, non-blank.
fn session_env_claim() -> Option<(String, String)> {
    let clean = |v: Result<String, std::env::VarError>| {
        v.ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let run_id = clean(std::env::var("PDO_RUN_ID"))?;
    let node_id = clean(std::env::var("PDO_NODE_ID"))?;
    Some((run_id, node_id))
}

pub fn run_page(action: PageAction) -> Result<()> {
    let url = std::env::var("PDO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string());
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default();
    let session = session_env_claim();
    let with_identity = |request: reqwest::blocking::RequestBuilder| {
        let mut request = request.header("X-PDO-Actor", "cli");
        if let Some((run_id, node_id)) = &session {
            request = request
                .header(SESSION_RUN_HEADER, run_id)
                .header(SESSION_NODE_HEADER, node_id);
        }
        request
    };

    match action {
        PageAction::Mount { name, directory } => {
            let directory = directory
                .canonicalize()
                .with_context(|| format!("cannot resolve {}", directory.display()))?;
            let response = with_identity(client.post(format!("{url}/pages")))
                .json(&serde_json::json!({ "name": name, "directory": directory }))
                .send()
                .context("failed to reach daemon")?;
            let mount: PageMount = daemon_response(response, "mount page")?;
            println!(
                "Mounted {} at /pages/{}/ from {}.",
                mount.name, mount.name, mount.directory
            );
        }
        PageAction::List => {
            let response = with_identity(client.get(format!("{url}/pages")))
                .send()
                .context("failed to reach daemon")?;
            let mounts: Vec<PageMount> = daemon_response(response, "list pages")?;
            for mount in mounts {
                let run = mount.run_id.as_deref().unwrap_or("-");
                println!(
                    "{}\t{}\t{}\t{}",
                    mount.name, mount.directory, run, mount.created_at
                );
            }
        }
        PageAction::Unmount { name } => {
            let response = with_identity(client.delete(format!("{url}/pages/{name}")))
                .send()
                .context("failed to reach daemon")?;
            let _: PageMount = daemon_response(response, "unmount page")?;
            println!("Unmounted {name}.");
        }
    }
    Ok(())
}

/// The body of a success, or the daemon's own `error` sentence.
fn daemon_response<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
    operation: &str,
) -> Result<T> {
    let status = response.status();
    let raw = response.text().unwrap_or_default();
    if !status.is_success() {
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        let detail = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or(raw.as_str());
        anyhow::bail!("daemon refused to {operation} ({status}): {detail}");
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("unreadable success body from daemon: {raw}"))
}
