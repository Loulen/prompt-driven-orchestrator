//! `pdo review list` / `pdo review reply` (#751, ADR-0067 §4): the thin CLI
//! client of the review's routes, run from a node or manager session. The Run
//! id comes from `PDO_RUN_ID` (`--run` overrides it); identity travels in the
//! session headers, so `identify` names the node's Agent and the daemon
//! deduces the author (`manager` for the manager session, else the node id).

use anyhow::{Context, Result};
use clap::Subcommand;
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_run::adapters::cli::{cli_daemon_url, cli_run_id, session_env_claim};

use crate::domain::comments::StateFilter;

#[derive(Subcommand, Debug)]
pub enum ReviewAction {
    /// List the Run's review comments as JSON, each with its thread of replies.
    /// Mapped onto the Run's current fork → tip: `outdated: true` means the line
    /// a comment points at has changed since it was written (its `excerpt` keeps
    /// the original hunk); `mapped_line` is where the line sits now.
    List {
        /// `open` (default), `resolved` or `all`.
        #[arg(long, default_value = "open")]
        state: String,
        /// The Run to read; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
    },
    /// Reply to one comment (`rc-001`, …). `--resolved` proposes a resolution —
    /// or resolves directly when the instance setting `review_agent_can_resolve`
    /// is on. The human can always reopen.
    Reply {
        /// The comment id, as shown by `pdo review list` and in the message you received.
        id: String,
        /// Your answer (markdown). `--body` is accepted as an alias.
        #[arg(long, visible_alias = "body")]
        text: String,
        /// You consider the comment addressed.
        #[arg(long)]
        resolved: bool,
        /// The Run the comment belongs to; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
    },
}

/// One-shot `pdo review list` / `pdo review reply` (#751): the thin CLI client of
/// the review endpoints. Blocking `reqwest`, no tokio runtime (see `main.rs`);
/// plain `Result` → `0`/`1` exit mapping. Identity travels in the session headers
/// — the daemon deduces the author (`manager` for the manager session, else the
/// node id); `--run` only changes which Run is addressed, never who speaks.
pub fn run_review(action: ReviewAction) -> Result<()> {
    let url = cli_daemon_url();
    let client = reqwest::blocking::Client::new();
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
    let run_of = |flag: Option<String>| -> Result<String> {
        match flag {
            Some(r) if !r.trim().is_empty() => Ok(r.trim().to_string()),
            _ => cli_run_id().context(
                "no Run: pass `--run <run-id>` or run this from a PDO node/manager session",
            ),
        }
    };

    match action {
        ReviewAction::List { state, run } => {
            let run_id = run_of(run)?;
            if StateFilter::parse(&state).is_none() {
                anyhow::bail!("--state must be `open`, `resolved` or `all`, got {state:?}");
            }
            // #752: mapped onto the Run's current fork → tip, so each comment
            // says `outdated: true` when the line it points at has changed since
            // (and `mapped_line` when it merely shifted).
            let response = with_identity(client.get(format!(
                "{url}/runs/{run_id}/review/comments?state={}&from=fork&to=tip",
                state.trim()
            )))
            .send()
            .context("failed to reach daemon")?;
            let body: serde_json::Value = daemon_cli_response(response, "list review comments")?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        ReviewAction::Reply {
            id,
            text,
            resolved,
            run,
        } => {
            let run_id = run_of(run)?;
            if text.trim().is_empty() {
                anyhow::bail!("--text must not be empty");
            }
            let response = with_identity(client.post(format!(
                "{url}/runs/{run_id}/review/comments/{}/reply",
                id.trim()
            )))
            .json(&serde_json::json!({ "text": text, "resolved": resolved }))
            .send()
            .context("failed to reach daemon")?;
            let body: serde_json::Value = daemon_cli_response(response, "reply to review comment")?;
            let author = body["author"].as_str().unwrap_or("agent");
            let outcome = body["outcome"].as_str().unwrap_or("replied");
            let what = match outcome {
                "resolved" => "resolved (the instance lets agents resolve; the human can reopen)",
                "proposed" => "resolution proposed — the human decides with Resolve / Reopen",
                _ => "reply recorded",
            };
            println!("{id}: {what} · author {author} · run {run_id}");
        }
    }
    Ok(())
}

fn daemon_cli_response<T: serde::de::DeserializeOwned>(
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
