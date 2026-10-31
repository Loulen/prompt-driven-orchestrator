//! `pdo run status <id>`: the state of a Run in one line, for an agent that
//! waits without polling and a maintainer who reads it at a glance. A client of
//! the Run's detail (`GET /runs/{id}`) and of its children
//! (`GET /runs/{id}/children`): no route of its own.

use std::process::ExitCode;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::cli_daemon_url;
use super::run_list::daemon_sentence;
use crate::domain::run_summary::working_nodes;
use crate::domain::state::{NodeStatus, RunState, RunStatus};

/// Exit code of `pdo run status` when the daemon knows no Run by that id.
pub const EXIT_RUN_NOT_FOUND: u8 = 4;

/// What `pdo run status` says of a Run. Its JSON form is the `--json` output:
/// one compact object with the fields of the line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunStatusLine {
    pub run_id: String,
    pub status: RunStatus,
    /// Working nodes (agent, script) that are completed or skipped.
    pub nodes_done: usize,
    /// Working nodes of the pipeline, started or not.
    pub nodes_total: usize,
    /// Working nodes running, or ready to run and waiting for a session slot.
    pub nodes_running: usize,
    pub nodes_awaiting_user: usize,
    pub nodes_failed: usize,
    /// Child Runs not yet terminal.
    pub children_active: usize,
    /// The Run's cost in USD; `None` when it is not known (no cost source, or
    /// a harness whose cost PDO cannot read).
    pub cost_usd: Option<f64>,
    /// `cost_usd` is a lower bound: a model had no price.
    pub cost_partial: bool,
}

impl RunStatusLine {
    /// The status of `run`, which has `children_active` child Runs not yet
    /// terminal.
    pub fn of(run: &RunState, children_active: usize) -> Self {
        let nodes = working_nodes(run);
        let count =
            |pred: fn(&NodeStatus) -> bool| nodes.iter().filter(|n| pred(&n.status)).count();
        let cost = run
            .cost
            .as_ref()
            .filter(|cost| cost.uncosted_harnesses.is_empty());
        Self {
            run_id: run.run_id.clone(),
            status: run.status.clone(),
            nodes_done: count(NodeStatus::is_settled_complete),
            nodes_total: nodes.len(),
            nodes_running: count(|s| matches!(s, NodeStatus::Running | NodeStatus::Waiting)),
            nodes_awaiting_user: count(|s| matches!(s, NodeStatus::AwaitingUser)),
            nodes_failed: count(|s| matches!(s, NodeStatus::Failed)),
            children_active,
            cost_usd: cost.map(|cost| cost.usd),
            cost_partial: cost.is_some_and(|cost| cost.partial),
        }
    }

    /// `<id> <status> · nodes <done>/<total> done[, <n> running][, <n>
    /// awaiting_user][, <n> failed] · children <n> active · $<cost>`.
    pub fn line(&self) -> String {
        let mut nodes = format!("nodes {}/{} done", self.nodes_done, self.nodes_total);
        for (n, what) in [
            (self.nodes_running, "running"),
            (self.nodes_awaiting_user, "awaiting_user"),
            (self.nodes_failed, "failed"),
        ] {
            if n > 0 {
                nodes.push_str(&format!(", {n} {what}"));
            }
        }
        let cost = match self.cost_usd {
            None => "$—".to_string(),
            Some(usd) if self.cost_partial => format!("${usd:.2} (lower bound)"),
            Some(usd) => format!("${usd:.2}"),
        };
        format!(
            "{} {} · {nodes} · children {} active · {cost}",
            self.run_id,
            status_word(&self.status),
            self.children_active,
        )
    }
}

/// The wire word of a Run status (`awaiting_user`, …).
fn status_word(status: &RunStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The part of `GET /runs/{id}/children` this command reads.
#[derive(Deserialize)]
struct Children {
    nodes: Vec<ChildGroup>,
}

#[derive(Deserialize)]
struct ChildGroup {
    children: Vec<Child>,
}

#[derive(Deserialize)]
struct Child {
    status: RunStatus,
}

impl Children {
    fn active(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(|group| &group.children)
            .filter(|child| child.status.is_live())
            .count()
    }
}

/// What a read of the daemon came back with.
enum Read {
    Found(String),
    NotFound,
}

/// `pdo run status <id> [--json]`. Exit `0` the Run is found · `4` no Run by
/// that id · `1` the daemon is unreachable or refused the read.
pub fn run_run_status(run_id: &str, json: bool) -> ExitCode {
    match status_of(run_id) {
        Ok(Some(status)) => {
            if json {
                println!("{}", serde_json::to_string(&status).expect("serializes"));
            } else {
                println!("{}", status.line());
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("pdo run status: no run {run_id} on this daemon.");
            ExitCode::from(EXIT_RUN_NOT_FOUND)
        }
        Err(e) => {
            eprintln!("pdo run status FAILED — {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// The status of the Run `run_id`; `None` when the daemon knows no such Run.
fn status_of(run_id: &str) -> anyhow::Result<Option<RunStatusLine>> {
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default();
    let base = format!("{}/runs/{run_id}", cli_daemon_url());
    let Read::Found(detail) = read(&client, &base)? else {
        return Ok(None);
    };
    let run: RunState = serde_json::from_str(&detail)
        .map_err(|e| anyhow::anyhow!("unreadable run detail from the daemon: {e}"))?;
    // A Run forgotten between the two reads has no children left to count.
    let children_active = match read(&client, &format!("{base}/children"))? {
        Read::Found(body) => serde_json::from_str::<Children>(&body)
            .map_err(|e| anyhow::anyhow!("unreadable children list from the daemon: {e}"))?
            .active(),
        Read::NotFound => 0,
    };
    Ok(Some(RunStatusLine::of(&run, children_active)))
}

fn read(client: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Read> {
    let resp = client
        .get(url)
        .header("X-PDO-Actor", "cli")
        .send()
        .map_err(|e| anyhow::anyhow!("failed to reach the daemon: {e}"))?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status == StatusCode::NOT_FOUND {
        return Ok(Read::NotFound);
    }
    if !status.is_success() {
        let sentence = daemon_sentence(&body).unwrap_or(body);
        anyhow::bail!("daemon returned {status}: {sentence}");
    }
    Ok(Read::Found(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::state::CostStat;

    /// A Run of the pipeline whose working nodes are `defs`, each in the
    /// status given (`None`: not started yet).
    fn run(status: &str, defs: &[(&str, Option<&str>)], cost: Option<CostStat>) -> RunState {
        let node_defs: Vec<_> = defs
            .iter()
            .map(|(id, _)| def(id, "agent"))
            .chain([def("loop", "loop")])
            .collect();
        let nodes: serde_json::Map<_, _> = defs
            .iter()
            .filter_map(|(id, status)| {
                status.map(|s| {
                    (
                        id.to_string(),
                        serde_json::json!({
                            "node_id": id, "status": s, "iter": 1, "started_at": null,
                            "completed_at": null, "failure_reason": null
                        }),
                    )
                })
            })
            .collect();
        let mut run: RunState = serde_json::from_value(serde_json::json!({
            "run_id": "r1", "status": status, "pipeline_name": "p", "input": null,
            "started_at": null, "completed_at": null, "nodes": nodes, "node_defs": node_defs,
        }))
        .expect("a RunState");
        run.cost = cost;
        run
    }

    fn def(id: &str, node_type: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "node_type": node_type, "view_x": null, "view_y": null,
            "inputs": [], "outputs": []
        })
    }

    fn cost(usd: f64, partial: bool) -> CostStat {
        serde_json::from_value(serde_json::json!({ "usd": usd, "partial": partial }))
            .expect("a CostStat")
    }

    #[test]
    fn a_finished_run_reads_all_done() {
        let run = run(
            "completed",
            &[("a", Some("completed")), ("b", Some("skipped"))],
            Some(cost(1.5, false)),
        );
        assert_eq!(
            RunStatusLine::of(&run, 0).line(),
            "r1 completed · nodes 2/2 done · children 0 active · $1.50"
        );
    }

    #[test]
    fn a_running_run_names_its_running_and_awaiting_nodes() {
        let run = run(
            "awaiting_user",
            &[
                ("a", Some("completed")),
                ("b", Some("running")),
                ("c", Some("awaiting_user")),
                ("d", None),
            ],
            Some(cost(0.123, false)),
        );
        assert_eq!(
            RunStatusLine::of(&run, 0).line(),
            "r1 awaiting_user · nodes 1/4 done, 1 running, 1 awaiting_user · children 0 active · $0.12"
        );
    }

    #[test]
    fn a_run_with_children_and_a_failed_node_counts_both() {
        let run = run(
            "running",
            &[("a", Some("failed")), ("b", Some("waiting"))],
            Some(cost(12.0, false)),
        );
        assert_eq!(
            RunStatusLine::of(&run, 3).line(),
            "r1 running · nodes 0/2 done, 1 running, 1 failed · children 3 active · $12.00"
        );
    }

    #[test]
    fn a_partial_cost_is_a_lower_bound_and_an_absent_one_a_dash() {
        let partial = run("running", &[("a", Some("running"))], Some(cost(2.0, true)));
        assert!(RunStatusLine::of(&partial, 0)
            .line()
            .ends_with("· $2.00 (lower bound)"));

        let absent = run("running", &[("a", Some("running"))], None);
        assert!(RunStatusLine::of(&absent, 0).line().ends_with("· $—"));

        // A harness whose cost PDO cannot read makes the total unknown, not $0.
        let mut uncosted = cost(0.0, false);
        uncosted.uncosted_harnesses = vec!["other".into()];
        let uncosted = run("running", &[("a", Some("running"))], Some(uncosted));
        let status = RunStatusLine::of(&uncosted, 0);
        assert!(status.line().ends_with("· $—"));
        assert_eq!(status.cost_usd, None);
    }

    #[test]
    fn the_json_form_is_one_compact_object_with_the_line_s_fields() {
        let run = run(
            "running",
            &[("a", Some("completed")), ("b", Some("running"))],
            Some(cost(1.0, true)),
        );
        let json = serde_json::to_string(&RunStatusLine::of(&run, 2)).unwrap();
        assert!(!json.contains('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!({
                "run_id": "r1", "status": "running",
                "nodes_done": 1, "nodes_total": 2, "nodes_running": 1,
                "nodes_awaiting_user": 0, "nodes_failed": 0,
                "children_active": 2, "cost_usd": 1.0, "cost_partial": true
            })
        );
    }

    #[test]
    fn only_live_children_are_active() {
        let children: Children = serde_json::from_value(serde_json::json!({
            "run_id": "r1",
            "nodes": [
                { "node_id": "a", "children": [
                    { "status": "running" }, { "status": "completed" }, { "status": "awaiting_user" }
                ]},
                { "node_id": null, "children": [ { "status": "paused" }, { "status": "failed" } ] }
            ]
        }))
        .unwrap();
        assert_eq!(children.active(), 3);
    }
}
