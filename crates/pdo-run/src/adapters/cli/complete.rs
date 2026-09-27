//! `pdo complete`: the completion of a node iteration, and how its refusal
//! reads in the agent's terminal.

use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};

use super::*;

/// `pdo complete` — signal that this node iteration is done.
///
/// Returns an [`ExitCode`](std::process::ExitCode) rather than a `Result` because
/// this subcommand has a **public exit-code contract** (ADR-0035 §4): `0` granted
/// or legal duplicate · `3` refused, still your turn · `4` refused, the runtime
/// already ruled · `1` breakdown. Those codes live in pipeline authors' bash, so
/// they are as much an API as the wire shape.
///
/// Don't reach for `std::process::exit`: it skips the `reqwest` client's
/// destructors and breaks the symmetry with the other subcommands.
pub fn run_complete(auto: bool) -> std::process::ExitCode {
    let rid = match cli_run_id() {
        Ok(v) => v,
        Err(e) => return complete_breakdown(&format!("{e:#}")),
    };
    let nid = match cli_node_id() {
        Ok(v) => v,
        Err(e) => return complete_breakdown(&format!("{e:#}")),
    };
    let url = cli_daemon_url();
    let iter = cli_node_iter();

    let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/done");
    let client = reqwest::blocking::Client::new();
    // The session headers identify the caller as this node's Agent (#915).
    let resp = match client
        .post(&endpoint)
        .header(SESSION_RUN_HEADER, &rid)
        .header(SESSION_NODE_HEADER, &nid)
        .json(&serde_json::json!({ "iter": iter, "auto": auto }))
        .send()
    {
        Ok(r) => r,
        Err(e) => return complete_breakdown(&format!("failed to reach daemon: {e}")),
    };

    let status = resp.status();
    let raw = resp.text().unwrap_or_default();
    // The success body of `POST …/done` is the bare text `ok`, so a parse failure
    // is expected there and must not be treated as a breakdown.
    let body: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);

    if status.is_success() {
        if body.get("noop").and_then(|v| v.as_bool()) == Some(true) {
            let reason = body
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("the node iteration was already terminal");
            eprintln!(
                "Node {nid} was already complete — nothing to do ({reason}).\n\
                 This is a legal duplicate, not a failure. Do NOT run `pdo fail`."
            );
        } else {
            eprintln!("Node {nid} marked complete.");
        }
        return std::process::ExitCode::SUCCESS;
    }

    // A 5xx is a breakdown, not a verdict: the daemon could not decide, so nothing
    // was ruled and `pdo fail` is the right escalation.
    if status.is_server_error() {
        return complete_breakdown(&format!("daemon returned {status}: {raw}"));
    }
    let Some(slug) = body.get("error").and_then(|v| v.as_str()) else {
        return complete_breakdown(&format!(
            "daemon returned {status} with an unreadable body: {raw}"
        ));
    };
    let recoverable = body.get("recoverable").and_then(|v| v.as_bool());

    let mut out = String::new();
    match recoverable {
        Some(true) => out.push_str(&format!(
            "pdo complete REFUSED — {}. The node is still running; it is still your turn.\n",
            refusal_headline(slug, &body)
        )),
        Some(false) => out.push_str(&format!(
            "pdo complete REFUSED and the node is now FAILED — {}.\n\
             The daemon has already recorded the terminal event. `resume_run` is the only lever.\n",
            refusal_headline(slug, &body)
        )),
        // No `recoverable` key: an older daemon. Say so instead of guessing which
        // side of the fence we are on.
        None => out.push_str(&format!(
            "pdo complete REFUSED — {} (daemon returned {status} without a `recoverable` \
             flag, so whether the node is still yours is unknown).\n",
            refusal_headline(slug, &body)
        )),
    }
    out.push_str(&refusal_detail_lines(slug, &body));
    out.push_str(match (slug, recoverable) {
        ("completion_not_released", Some(true)) => {
            "The node and its run are now awaiting the user (the daemon declared the \
             wait for you). Ask the user to click **Mark ready for completion** (or \
             **Mark complete** to force completion). After release, run `pdo complete` \
             again. For any later question, declare it with \
             `pdo wait-user --message \"<question>\"`.\n\
             Do NOT run `pdo fail`."
        }
        (_, Some(true)) => {
            "Fix what is listed above, then run `pdo complete` again.\nDo NOT run `pdo fail`."
        }
        (_, Some(false)) => {
            "Do NOT run `pdo fail` — the failure is already recorded. Stop here and report it."
        }
        (_, None) => "Re-read the run state before acting. Do NOT run `pdo fail` blindly.",
    });
    eprintln!("{out}");

    match recoverable {
        Some(true) => std::process::ExitCode::from(EXIT_REFUSED_RECOVERABLE),
        // Unknown recoverability is treated as terminal: the expensive mistake is
        // appending a second failure, not skipping a retry.
        Some(false) | None => std::process::ExitCode::from(EXIT_REFUSED_TERMINAL),
    }
}

/// The `1` arm of `pdo complete`: transport, missing env, unreadable body, `5xx`.
/// The **only** arm where `pdo fail` is the right advice.
pub fn complete_breakdown(detail: &str) -> std::process::ExitCode {
    eprintln!(
        "pdo complete FAILED to get a verdict — {detail}\n\
         Nothing was decided about this node. Retry, or signal the problem with \
         `pdo fail --reason \"…\"`."
    );
    std::process::ExitCode::FAILURE
}

/// One human sentence per refusal slug. An unknown slug is rendered **verbatim**
/// (ADR-0001): a client that does not recognise a cause must not hide it.
pub fn refusal_headline(slug: &str, body: &serde_json::Value) -> String {
    match slug {
        "missing_outputs" => "your declared outputs are incomplete".into(),
        "frontmatter_retry_pending" => {
            "your output frontmatter does not match the declared schema (this was the ONE retry \
             you get)"
                .into()
        }
        "frontmatter_retry_exhausted" => {
            "your output frontmatter still did not match after the retry".into()
        }
        "script_validation_failed" => "this script node's declared outputs did not validate".into(),
        "delivery_failed" => {
            "PDO could not deliver your work onto the run's branch — it is still on disk, \
             untouched"
                .into()
        }
        "merge_conflict" => "merging your sub-worktree into the pipeline branch conflicted".into(),
        "merge_resolution_failed" | "merge_resolver_failed" | "merge_resolver_spawned" => {
            "the merge resolver did not settle the conflict".into()
        }
        "completion_rejected" => body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("the run does not accept this completion")
            .to_string(),
        "completion_not_released" => {
            "the user has not released this interactive node for completion".into()
        }
        "run_forgotten" => "this run has been forgotten (archived); it accepts nothing".into(),
        // Unknown slug: hand it over as-is, plus whatever prose came with it.
        other => match body.get("message").and_then(|v| v.as_str()) {
            Some(m) => format!("{other}: {m}"),
            None => other.to_string(),
        },
    }
}

/// The actionable middle of the message: what exactly is wrong, one item per line.
/// Reads both shapes of the validation detail — flat (`violations`/`missing`) and
/// the `script` fail-fast's nested `detail` (ADR-0035 §5) — so the agent gets the
/// list either way.
pub fn refusal_detail_lines(slug: &str, body: &serde_json::Value) -> String {
    let mut out = String::new();

    let nested = body.get("detail");
    let missing = body
        .get("missing")
        .or_else(|| nested.and_then(|d| d.get("missing")))
        .and_then(|v| v.as_array());
    if let Some(missing) = missing {
        if !missing.is_empty() {
            out.push_str("Missing declared outputs:\n");
            for m in missing {
                if let Some(m) = m.as_str() {
                    out.push_str(&format!("  - {m}\n"));
                }
            }
            out.push_str(
                "Write each missing artefact to the path listed in your prompt preamble.\n",
            );
        }
    }

    let violations = body
        .get("violations")
        .or_else(|| nested.and_then(|d| d.get("violations")))
        .and_then(|v| v.as_array());
    if let Some(violations) = violations {
        if !violations.is_empty() {
            out.push_str("Frontmatter violations:\n");
            for v in violations {
                let port = v.get("port").and_then(|x| x.as_str()).unwrap_or("?");
                let field = v.get("field").and_then(|x| x.as_str()).unwrap_or("?");
                let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("?");
                out.push_str(&format!("  - {port}.{field}: {reason}\n"));
            }
        }
    }

    if let Some(reason) = body.get("reason").and_then(|v| v.as_str()) {
        out.push_str(&format!("Reason: {reason}\n"));
    }
    // The guard's prose is already the headline; anything else's `message` is extra
    // context worth printing once.
    if slug != "completion_rejected" {
        if let Some(msg) = body.get("message").and_then(|v| v.as_str()) {
            if !msg.is_empty() && !slug.starts_with("run_") {
                out.push_str(&format!("Detail: {msg}\n"));
            }
        }
    }
    out
}
