//! The text the Run and node commands print: one tab-separated line per item,
//! the daemon's JSON with `--json`. Pure, tested below.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

fn text(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn items(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// The nodes of a Run's detail, sorted by id: `id  status  iter`.
pub(super) fn node_lines(run: &Value) -> String {
    let mut nodes: Vec<(&String, &Value)> = run["nodes"]
        .as_object()
        .map(|m| m.iter().collect())
        .unwrap_or_default();
    nodes.sort_by_key(|(id, _)| id.as_str());
    let mut out = format!(
        "{}\t{}\t{}\t{}\n",
        text(run, "run_id"),
        text(run, "status"),
        text(run, "pipeline_name"),
        text(run, "name"),
    );
    for (id, node) in nodes {
        out.push_str(&format!(
            "  {id}\t{}\titer {}\n",
            text(node, "status"),
            text(node, "iter")
        ));
    }
    out
}

/// A Run's event log: `ts  kind  node  iter`.
pub(super) fn event_lines(events: &Value, names: &ActorNames) -> String {
    items(events)
        .iter()
        .map(|e| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                text(e, "ts"),
                text(e, "kind"),
                text(e, "node_id"),
                text(e, "iter"),
                actor_label(&e["payload"]["actor"], names)
            )
        })
        .collect()
}

/// The display names of Actors, keyed by the Actor's JSON.
pub(super) type ActorNames = std::collections::HashMap<String, String>;

/// The distinct Actors the events name (`payload.actor`), in order of first
/// appearance: what to ask the display names of.
pub(super) fn event_actors(events: &Value) -> Vec<Value> {
    let mut actors: Vec<Value> = Vec::new();
    for actor in items(events).iter().map(|e| &e["payload"]["actor"]) {
        if actor.is_object() && !actors.contains(actor) {
            actors.push(actor.clone());
        }
    }
    actors
}

/// `actors` with the names `displays` gives them (the answer of `POST
/// /actors/display`, one entry per Actor, `null` when none names it).
pub(super) fn actor_names(actors: &[Value], displays: &Value) -> ActorNames {
    actors
        .iter()
        .zip(items(&displays["displays"]))
        .filter_map(|(actor, display)| {
            let name = display["name"].as_str()?;
            Some((actor.to_string(), name.to_string()))
        })
        .collect()
}

/// Who made an event: its Actor's display name, else its id (an Agent by its
/// node, a Trigger by its id); empty for an event no Actor made.
fn actor_label(actor: &Value, names: &ActorNames) -> String {
    if !actor.is_object() {
        return String::new();
    }
    if let Some(name) = names.get(&actor.to_string()) {
        return name.clone();
    }
    match actor["kind"].as_str() {
        Some("identified") => text(actor, "id"),
        Some("agent") => format!("agent:{}", text(actor, "node_id")),
        Some("trigger") => format!("trigger:{}", text(actor, "trigger_id")),
        Some(kind) => kind.to_string(),
        None => String::new(),
    }
}

/// The cost of Runs: `run_id  usd` (`?` unknown, `≥` a lower bound).
pub(super) fn cost_lines(costs: &Value) -> String {
    items(&costs["runs"])
        .iter()
        .map(|r| {
            let cost = &r["cost"];
            let usd = match cost["usd"].as_f64() {
                None => "?".to_string(),
                Some(usd) if cost["partial"] == true => format!("≥{usd:.2}"),
                Some(usd) => format!("{usd:.2}"),
            };
            format!("{}\t{usd}\n", text(r, "run_id"))
        })
        .collect()
}

/// A Run's refs: `id  sha  label`.
pub(super) fn ref_lines(refs: &Value) -> String {
    items(&refs["refs"])
        .iter()
        .map(|r| {
            let sha = r["sha"].as_str().unwrap_or("-");
            format!("{}\t{sha}\t{}\n", text(r, "id"), text(r, "label"))
        })
        .collect()
}

/// The hits of a transcript search: `node  iter  #index  excerpt`, then a
/// line when more entries match than the hits carry.
pub(super) fn search_lines(found: &Value) -> String {
    let mut out: String = items(&found["hits"])
        .iter()
        .map(|h| {
            let hit = &h["hit"];
            let snippet = &hit["snippet"];
            let who = match hit["kind"].as_str() {
                Some("tool") => text(hit, "name"),
                _ => text(hit, "speaker"),
            };
            format!(
                "{}\t{}\t#{}\t{who}\t{}[{}]{}\n",
                text(h, "node_id"),
                text(h, "iter"),
                text(hit, "index"),
                text(snippet, "before"),
                text(snippet, "matched"),
                text(snippet, "after"),
            )
        })
        .collect();
    let shown = items(&found["hits"]).len();
    if let Some(matched) = found["matched"].as_u64() {
        if matched as usize > shown {
            out.push_str(&format!("… {matched} matches, {shown} shown\n"));
        }
    }
    out
}

/// A transcript page: `#index  speaker or tool  text`.
pub(super) fn transcript_lines(page: &Value) -> String {
    items(&page["entries"])
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let index = e["index"].as_u64().map_or(i.to_string(), |n| n.to_string());
            let who = e["name"]
                .as_str()
                .or_else(|| e["speaker"].as_str())
                .or_else(|| e["role"].as_str())
                .or_else(|| e["kind"].as_str())
                .unwrap_or_default();
            let body = e["text"]
                .as_str()
                .or_else(|| e["summary"].as_str())
                .unwrap_or_default()
                .replace('\n', " ");
            format!("#{index}\t{who}\t{body}\n")
        })
        .collect()
}

/// What retrying a node would invalidate.
pub(super) fn retry_preview_line(node_id: &str, preview: &Value) -> String {
    let downstream: Vec<String> = items(&preview["downstream"])
        .iter()
        .filter_map(|n| n.as_str().map(str::to_string))
        .collect();
    if downstream.is_empty() {
        return format!("retrying {node_id} invalidates nothing downstream\n");
    }
    format!(
        "retrying {node_id} invalidates {} ({} with artifacts)\n",
        downstream.join(", "),
        text(preview, "affected_count")
    )
}

/// `bytes` into `output`, else onto stdout as they are.
pub(super) fn write_bytes(bytes: &[u8], output: Option<&Path>) -> Result<()> {
    match output {
        Some(path) => {
            std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
        }
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(bytes).context("cannot write to stdout")?;
            out.flush().context("cannot write to stdout")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_run_shows_its_nodes_sorted() {
        let run = json!({
            "run_id": "r1", "status": "running", "pipeline_name": "p", "name": null,
            "nodes": {
                "b": { "status": "pending", "iter": 1 },
                "a": { "status": "completed", "iter": 2 }
            }
        });
        assert_eq!(
            node_lines(&run),
            "r1\trunning\tp\t\n  a\tcompleted\titer 2\n  b\tpending\titer 1\n"
        );
    }

    #[test]
    fn an_event_is_one_line() {
        let events = json!([
            { "ts": "t0", "kind": "run_started", "node_id": null, "iter": null },
            { "ts": "t1", "kind": "node_started", "node_id": "a", "iter": 1 }
        ]);
        assert_eq!(
            event_lines(&events, &ActorNames::new()),
            "t0\trun_started\t\t\t\nt1\tnode_started\ta\t1\t\n"
        );
    }

    #[test]
    fn an_event_shows_who_made_it_by_name_else_by_id() {
        let alice = json!({ "kind": "identified", "namespace": "test", "id": "alice" });
        let bob = json!({ "kind": "identified", "namespace": "test", "id": "bob" });
        let events = json!([
            { "ts": "t0", "kind": "node_taken_over", "node_id": "a", "iter": 1,
              "payload": { "source": "pty", "actor": bob } },
            { "ts": "t1", "kind": "command_issued", "node_id": "a", "iter": 1,
              "payload": { "command": "retry", "actor": alice } },
            { "ts": "t2", "kind": "node_stopped", "node_id": "a", "iter": 1,
              "payload": { "actor": { "kind": "agent", "run_id": "r", "node_id": "boss" } } },
            { "ts": "t3", "kind": "node_resumed", "node_id": "a", "iter": 1,
              "payload": { "actor": alice } },
            { "ts": "t4", "kind": "node_completed", "node_id": "a", "iter": 1,
              "payload": { "actor": { "kind": "local_operator" } } }
        ]);
        let actors = event_actors(&events);
        assert_eq!(actors.len(), 4, "each Actor asked once: {actors:?}");
        assert_eq!(actors[0], bob);
        let names = actor_names(
            &actors,
            &json!({ "displays": [null, { "name": "Alice", "avatar_url": null }, null, null] }),
        );
        let actors_shown: Vec<String> = event_lines(&events, &names)
            .lines()
            .map(|line| line.rsplit('\t').next().unwrap().to_string())
            .collect();
        assert_eq!(
            actors_shown,
            ["bob", "Alice", "agent:boss", "Alice", "local_operator"]
        );
    }

    #[test]
    fn a_cost_is_unknown_exact_or_a_lower_bound() {
        let costs = json!({ "runs": [
            { "run_id": "r1" },
            { "run_id": "r2", "cost": { "usd": 1.5, "partial": false } },
            { "run_id": "r3", "cost": { "usd": 0.25, "partial": true } }
        ]});
        assert_eq!(cost_lines(&costs), "r1\t?\nr2\t1.50\nr3\t≥0.25\n");
    }

    #[test]
    fn a_ref_without_sha_shows_a_dash() {
        let refs = json!({ "refs": [
            { "id": "fork", "sha": "abc", "label": "fork point" },
            { "id": "tip", "sha": null, "label": "tip" }
        ]});
        assert_eq!(ref_lines(&refs), "fork\tabc\tfork point\ntip\t-\ttip\n");
    }

    #[test]
    fn a_search_hit_marks_the_match_and_counts_the_rest() {
        let found = json!({
            "hits": [{ "node_id": "a", "iter": 1, "hit": {
                "kind": "message", "index": 3, "speaker": "assistant",
                "snippet": { "before": "the ", "matched": "word", "after": " here" }
            }}],
            "matched": 4
        });
        assert_eq!(
            search_lines(&found),
            "a\t1\t#3\tassistant\tthe [word] here\n… 4 matches, 1 shown\n"
        );
    }

    #[test]
    fn a_retry_preview_names_what_it_invalidates() {
        assert_eq!(
            retry_preview_line("a", &json!({ "downstream": [], "affected_count": 0 })),
            "retrying a invalidates nothing downstream\n"
        );
        assert_eq!(
            retry_preview_line(
                "a",
                &json!({ "downstream": ["b", "c"], "affected_count": 1 })
            ),
            "retrying a invalidates b, c (1 with artifacts)\n"
        );
    }

    #[test]
    fn a_transcript_entry_is_one_line() {
        let page = json!({ "entries": [
            { "index": 0, "speaker": "user", "text": "hello\nthere" },
            { "index": 1, "name": "Bash", "summary": "ls" }
        ]});
        assert_eq!(
            transcript_lines(&page),
            "#0\tuser\thello there\n#1\tBash\tls\n"
        );
    }
}
