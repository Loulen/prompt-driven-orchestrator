//! `pdo reap` (#474, ADR-0012a): the deterministic half of the disk janitor,
//! over `GET /runs/reapable` and the `cleanup_run` command.

use anyhow::{Context, Result};

use super::cli_daemon_url;
use crate::domain::reap_policy;
use crate::domain::state::RunStatus;

/// Human-readable byte size for reap reports (binary units, one decimal).
pub(crate) fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let f = n as f64;
    if f >= GB {
        format!("{:.1} GB", f / GB)
    } else if f >= MB {
        format!("{:.1} MB", f / MB)
    } else if f >= KB {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{n} B")
    }
}

/// snake_case status label for reap reports (mirrors the wire representation).
fn reap_status_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::AwaitingUser => "awaiting_user",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Skipped => "skipped",
        RunStatus::Halted => "halted",
        RunStatus::Paused => "paused",
        RunStatus::Archived => "archived",
    }
}

/// One-shot `pdo reap`: the deterministic half of the disk janitor. Lists
/// `GET /runs/reapable?size=true`, runs the pure [`reap_policy`] over it, and —
/// unless `--count`/`--dry-run` — archives each selected Run via
/// `POST /runs/{id}/commands {"kind":"cleanup_run"}`.
///
/// **Exit semantics.** Failure to *list* is fatal: a broken URL that silently
/// no-ops forever is worse than a visible node failure. Per-run reclaim errors and
/// a wall-clock cutoff must NOT be fatal — a failed janitor Run leaves its own
/// worktree behind, which a `completed`-only policy never reclaims, so the residue
/// grows monotonically. The rest waits for the next fire.
pub fn run_reap(
    count: bool,
    dry_run: bool,
    ttl_hours: Option<i64>,
    terminal_ttl_hours: Option<i64>,
    budget_secs: u64,
) -> Result<()> {
    let url = cli_daemon_url();
    let client = reqwest::blocking::Client::new();
    let listing = format!("{url}/runs/reapable?size=true");

    let list_runs = |client: &reqwest::blocking::Client| -> Result<Vec<reap_policy::ReapableRun>> {
        client
            .get(&listing)
            .send()
            .context("failed to reach daemon (GET /runs/reapable)")?
            .error_for_status()
            .context("daemon rejected GET /runs/reapable")?
            .json()
            .context("failed to parse /runs/reapable response")
    };

    let runs = list_runs(&client)?;

    let mut policy = reap_policy::ReapPolicy::default();
    if let Some(h) = ttl_hours {
        policy.completed_ttl_secs = h.saturating_mul(3600);
    }
    if let Some(h) = terminal_ttl_hours {
        policy.terminal_ttl_secs = h.saturating_mul(3600);
    }

    let plan = policy.plan(&runs);

    if count {
        println!("{}", plan.reclaim.len());
        return Ok(());
    }

    if plan.reclaim.is_empty() {
        println!(
            "disk-janitor: nothing to reclaim ({} terminal run(s) listed, none past TTL)",
            plan.retained
        );
        return Ok(());
    }

    println!(
        "disk-janitor: {} run(s) to reclaim (~{}), {} retained",
        plan.reclaim.len(),
        human_bytes(plan.reclaim_bytes),
        plan.retained
    );
    for d in &plan.reclaim {
        println!(
            "  {} [{}] ~{} — {}",
            d.run_id,
            reap_status_label(&d.status),
            human_bytes(d.approx_disk_bytes.unwrap_or(0)),
            d.reason
        );
    }

    if dry_run {
        println!("disk-janitor: --dry-run, no reclaim issued");
        return Ok(());
    }

    // Reclaim within the wall-clock budget.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(budget_secs.max(1));
    let mut reclaimed_ids: Vec<String> = Vec::new();
    let mut deferred = 0usize;
    let mut errors = 0usize;

    for d in &plan.reclaim {
        if std::time::Instant::now() >= deadline {
            deferred += 1;
            continue;
        }
        let endpoint = format!("{url}/runs/{}/commands", d.run_id);
        match client
            .post(&endpoint)
            .json(&serde_json::json!({ "kind": "cleanup_run" }))
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                let code = status.as_u16();
                let body = resp.text().unwrap_or_default();
                // Don't trust a bare `200`: a mistyped path 200s the SPA shell and
                // `cleanup_run` swallows fs/git errors, so assert on the body.
                // `409` = archived between listing and now, a benign no-op.
                if code == 409 || (code == 200 && body.contains("\"archived\"")) {
                    reclaimed_ids.push(d.run_id.clone());
                } else {
                    errors += 1;
                    eprintln!("  cleanup_run {} → {} {}", d.run_id, status, body.trim());
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("  cleanup_run {} → transport error: {e}", d.run_id);
            }
        }
    }

    // Honest proof: re-list and confirm the reclaimed ids are gone. A `200` /
    // `409` above only says the command was accepted, not that the bytes left.
    let confirmed = match list_runs(&client) {
        Ok(after) => {
            let still: std::collections::HashSet<String> =
                after.into_iter().map(|r| r.run_id).collect();
            reclaimed_ids
                .iter()
                .filter(|id| !still.contains(*id))
                .count()
        }
        // Can't re-list — report the accepted count without the confirmation.
        Err(_) => reclaimed_ids.len(),
    };

    println!(
        "disk-janitor: reclaimed {} run(s) ({confirmed} confirmed gone), {deferred} deferred (budget {budget_secs}s), {errors} error(s)",
        reclaimed_ids.len()
    );

    Ok(())
}
