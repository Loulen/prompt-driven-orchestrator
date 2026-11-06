//! `pdo review` (#751, ADR-0067 §4): the thin CLI client of the review's
//! routes. `list` and `reply` run from a node or manager session: the Run id
//! comes from `PDO_RUN_ID` (`--run` overrides it); identity travels in the
//! session headers, so `identify` names the node's Agent and the daemon
//! deduces the author (`manager` for the manager session, else the node id).
//! `diff`, `comment`, `resolve` and `reopen` are the Review page's gestures,
//! for an operator as for a session: read the diff, send a comment on a line
//! (the Pipeline Manager receives it), decide on one.

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use pdo_run::adapters::cli::{cli_daemon_url, cli_run_id, session_env_claim};
use serde_json::{json, Value};

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
    /// The Run's diff: its patch, or with `--stat` one line per file (status,
    /// additions, deletions, path). `--from`/`--to` compare two Run refs
    /// (fork → tip by default).
    Diff {
        #[arg(long)]
        stat: bool,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// The Run; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
        /// The structured diff (files, hunks, lines) as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Write a comment on a line of the Run's diff and send it, as the Review
    /// page does: the Pipeline Manager receives it (started if it is not
    /// running). Prints the comment's id.
    Comment {
        /// The file, as the diff names it.
        path: String,
        /// The line, in the file of `--side`.
        line: i64,
        /// The comment (markdown).
        #[arg(long, visible_alias = "body")]
        text: String,
        /// The side of the diff the line is on: `new` (default) or `old`.
        #[arg(long, value_enum, default_value_t = Side::New)]
        side: Side,
        /// The Run refs the line was read between (fork → tip by default).
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// The Run; defaults to the session's `PDO_RUN_ID`.
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Resolve a comment, as the Review page's Resolve.
    Resolve {
        id: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Reopen a resolved comment.
    Reopen {
        id: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Side {
    New,
    Old,
}

/// One-shot `pdo review list` / `pdo review reply` (#751): the thin CLI client of
/// the review endpoints. Blocking `reqwest`, no tokio runtime (see `main.rs`);
/// plain `Result` → `0`/`1` exit mapping. Identity travels in the session headers
/// — the daemon deduces the author (`manager` for the manager session, else the
/// node id); `--run` only changes which Run is addressed, never who speaks.
pub fn run_review(action: ReviewAction) -> Result<()> {
    let url = cli_daemon_url();
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
        page => return run_review_page(page),
    }
    Ok(())
}

/// The Run a command addresses: `--run`, else the session's `PDO_RUN_ID`.
fn run_of(flag: Option<String>) -> Result<String> {
    match flag {
        Some(r) if !r.trim().is_empty() => Ok(r.trim().to_string()),
        _ => cli_run_id()
            .context("no Run: pass `--run <run-id>` or run this from a PDO node/manager session"),
    }
}

/// The Review page's gestures, through the shared client of the daemon.
fn run_review_page(action: ReviewAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        ReviewAction::Diff {
            stat,
            from,
            to,
            run,
            json,
        } => {
            let run = segment(&run_of(run)?);
            if !stat && !json && from.is_none() && to.is_none() {
                let patch = daemon.get(&format!("/runs/{run}/diff"))?.ok()?;
                print!("{}", patch.text());
                return Ok(());
            }
            let mut query = Vec::new();
            for (key, value) in [("from", from), ("to", to)] {
                if let Some(value) = value {
                    query.push(format!("{key}={}", segment(&value)));
                }
            }
            let diff = daemon
                .get(&format!("/runs/{run}/diff/structured?{}", query.join("&")))?
                .ok()?
                .json()?;
            if json {
                print_json(&diff);
            } else if stat {
                print!("{}", stat_lines(&diff));
            } else {
                print!("{}", patch_text(&diff));
            }
        }
        ReviewAction::Comment {
            path,
            line,
            text,
            side,
            from,
            to,
            run,
            json,
        } => {
            anyhow::ensure!(!text.trim().is_empty(), "--text must not be empty");
            let run_id = run_of(run)?;
            let side = match side {
                Side::New => "new",
                Side::Old => "old",
            };
            let sent = daemon
                .post(
                    &format!("/runs/{}/review/comments/send", segment(&run_id)),
                    Some(&json!({ "comments": [{
                        "path": path, "side": side, "line": line,
                        "from": from, "to": to, "text": text,
                    }] })),
                )?
                .ok()?
                .json()?;
            if json {
                print_json(&sent);
            } else {
                let ids = sent_ids(&sent);
                println!("{} sent · {path}:{line} · run {run_id}", ids.join(", "));
            }
        }
        ReviewAction::Resolve { id, run, json } => {
            let run_id = run_of(run)?;
            let answer = daemon
                .post(
                    &format!(
                        "/runs/{}/review/comments/{}/resolve",
                        segment(&run_id),
                        segment(id.trim())
                    ),
                    None,
                )?
                .ok()?
                .json()?;
            decided(&id, &answer, json, "resolved");
        }
        ReviewAction::Reopen { id, run, json } => {
            let run_id = run_of(run)?;
            let answer = daemon
                .post(
                    &format!(
                        "/runs/{}/review/comments/{}/reopen",
                        segment(&run_id),
                        segment(id.trim())
                    ),
                    None,
                )?
                .ok()?
                .json()?;
            decided(&id, &answer, json, "reopened");
        }
        ReviewAction::List { .. } | ReviewAction::Reply { .. } => {
            unreachable!("answered by run_review")
        }
    }
    Ok(())
}

fn decided(id: &str, answer: &Value, json: bool, verb: &str) {
    if json {
        print_json(answer);
    } else {
        println!("{id} {verb}");
    }
}

/// The ids of the comments a send recorded.
fn sent_ids(sent: &Value) -> Vec<String> {
    sent["sent"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str().map(str::to_string))
        .collect()
}

fn file_path(file: &Value) -> &str {
    file["new_path"]
        .as_str()
        .or_else(|| file["old_path"].as_str())
        .unwrap_or("?")
}

/// One line per file of a structured diff: status, additions, deletions, path.
fn stat_lines(diff: &Value) -> String {
    diff["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|file| {
            format!(
                "{}\t+{}\t-{}\t{}\n",
                file["status"].as_str().unwrap_or("?"),
                file["additions"],
                file["deletions"],
                file_path(file)
            )
        })
        .collect()
}

/// A structured diff read as a patch: each file, each hunk's `@@` line, its
/// lines with their `+`, `-` or ` `.
fn patch_text(diff: &Value) -> String {
    let mut out = String::new();
    for file in diff["files"].as_array().into_iter().flatten() {
        out.push_str(&format!("--- {}\n", file_path(file)));
        for hunk in file["hunks"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@ {}\n",
                hunk["old_start"],
                hunk["old_lines"],
                hunk["new_start"],
                hunk["new_lines"],
                hunk["header"].as_str().unwrap_or("")
            ));
            for line in hunk["lines"].as_array().into_iter().flatten() {
                let mark = match line["kind"].as_str() {
                    Some("add") => '+',
                    Some("del") => '-',
                    _ => ' ',
                };
                out.push(mark);
                out.push_str(line["content"].as_str().unwrap_or(""));
                out.push('\n');
            }
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn diff() -> Value {
        json!({ "files": [{
            "old_path": "src/a.rs", "new_path": "src/a.rs", "status": "modified",
            "binary": false, "additions": 1, "deletions": 1,
            "hunks": [{ "old_start": 3, "old_lines": 2, "new_start": 3, "new_lines": 2, "header": "fn a()",
                "lines": [
                    { "kind": "context", "content": "let x = 1;" },
                    { "kind": "del", "content": "x" },
                    { "kind": "add", "content": "y" },
                ] }],
        }, {
            "old_path": null, "new_path": "b.md", "status": "added",
            "binary": false, "additions": 4, "deletions": 0, "hunks": [],
        }]})
    }

    #[test]
    fn the_stat_is_one_line_per_file() {
        assert_eq!(
            stat_lines(&diff()),
            "modified\t+1\t-1\tsrc/a.rs\nadded\t+4\t-0\tb.md\n"
        );
    }

    #[test]
    fn a_structured_diff_reads_back_as_a_patch() {
        assert_eq!(
            patch_text(&diff()),
            "--- src/a.rs\n@@ -3,2 +3,2 @@ fn a()\n let x = 1;\n-x\n+y\n--- b.md\n"
        );
    }

    #[test]
    fn a_send_answers_the_ids_it_recorded() {
        let sent = json!({ "sent": [{ "id": "rc-003" }, { "id": "rc-004" }], "batch_id": "b" });
        assert_eq!(sent_ids(&sent), ["rc-003", "rc-004"]);
    }
}
