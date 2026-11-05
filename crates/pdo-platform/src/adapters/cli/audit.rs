//! `pdo audit`: the journal of the mutations made outside a Run, newest first,
//! over `GET /audit`.

use anyhow::Result;
use clap::Args;
use serde_json::Value;

use super::daemon::{print_json, segment, Daemon};

#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Only the entries at or after this instant (RFC 3339).
    #[arg(long)]
    pub from: Option<String>,
    /// Only the entries before this instant (RFC 3339).
    #[arg(long)]
    pub to: Option<String>,
    /// Only the entries about this kind of target (`trigger`, `project`, …).
    #[arg(long)]
    pub target_kind: Option<String>,
    /// Only the entries about this target id.
    #[arg(long)]
    pub target_id: Option<String>,
    /// At most this many entries.
    #[arg(long)]
    pub limit: Option<i64>,
    /// Print the daemon's JSON answer.
    #[arg(long)]
    pub json: bool,
}

pub fn run_audit(args: AuditArgs) -> Result<()> {
    let query = query(&args);
    let entries = Daemon::from_env()
        .get(&format!("/audit?{query}"))?
        .ok()?
        .json()?;
    if args.json {
        print_json(&entries);
    } else {
        print!("{}", lines(&entries));
    }
    Ok(())
}

fn query(args: &AuditArgs) -> String {
    let pairs: Vec<String> = [
        ("from", args.from.clone()),
        ("to", args.to.clone()),
        ("target_kind", args.target_kind.clone()),
        ("target_id", args.target_id.clone()),
        ("limit", args.limit.map(|l| l.to_string())),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|v| format!("{key}={}", segment(&v))))
    .collect();
    pairs.join("&")
}

/// One tab-separated line per entry: `ts  action  target_kind  target_id  actor`.
fn lines(entries: &Value) -> String {
    let field = |entry: &Value, key: &str| entry[key].as_str().unwrap_or("").to_string();
    entries
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                field(entry, "ts"),
                field(entry, "action"),
                field(entry, "target_kind"),
                field(entry, "target_id"),
                field(entry, "actor_hint"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_travel_in_the_query() {
        let args = AuditArgs {
            from: None,
            to: None,
            target_kind: Some("trigger".into()),
            target_id: None,
            limit: Some(5),
            json: false,
        };
        assert_eq!(query(&args), "target_kind=trigger&limit=5");
    }

    #[test]
    fn an_entry_is_one_line() {
        let entries = serde_json::json!([{
            "ts": "t", "action": "trigger.deleted", "target_kind": "trigger",
            "target_id": "tr-1", "actor_hint": "cli"
        }]);
        assert_eq!(lines(&entries), "t\ttrigger.deleted\ttrigger\ttr-1\tcli\n");
    }
}
