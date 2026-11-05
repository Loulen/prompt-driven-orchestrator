//! `pdo session`: the sessions of the running daemon. `status` reads what the
//! status bar shows (live sessions, the cap, the service, the sweep); the
//! other commands are the gestures of a Session hébergée's detail (ADR-0092):
//! read it and its events, terminate, relaunch, configure.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};
use serde_json::{json, Map, Value};

#[derive(Subcommand, Debug)]
pub enum SessionAction {
    /// The live node sessions, the cap, the daemon's version, the service and
    /// the orphan sweep, in one line.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show a Session hébergée: its extension, kind, state, folder and
    /// configuration.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The events of a Session hébergée, oldest first, one tab-separated line
    /// each: when, what, who, message.
    Events {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Terminate a Session hébergée.
    Terminate {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Relaunch an ended Session hébergée, resuming its conversation when its
    /// harness can.
    Relaunch {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Change the configuration of a Session hébergée, applied at its next
    /// relaunch. Only the flags given change; an empty value clears the field.
    Configure {
        id: String,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Its initial prompt.
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run_session(action: SessionAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        SessionAction::Status { json } => {
            let status = daemon.get("/sessions")?.ok()?.json()?;
            if json {
                print_json(&status);
            } else {
                println!("{}", status_line(&status));
            }
        }
        SessionAction::Show { id, json } => {
            let session = read(&daemon, &id)?;
            if json {
                print_json(&session);
            } else {
                print!("{}", show_text(&session));
            }
        }
        SessionAction::Events { id, json } => {
            let events = daemon
                .get(&format!("/sessions/{}/events", segment(&id)))?
                .ok()?
                .json()?;
            if json {
                print_json(&events);
            } else {
                print!("{}", event_lines(&events));
            }
        }
        SessionAction::Terminate { id, json } => {
            let session = daemon
                .post(&format!("/sessions/{}/terminate", segment(&id)), None)?
                .ok()?
                .json()?;
            done(&id, &session, json, "terminated");
        }
        SessionAction::Relaunch { id, json } => {
            let session = daemon
                .post(&format!("/sessions/{}/relaunch", segment(&id)), None)?
                .ok()?
                .json()?;
            done(&id, &session, json, "relaunched");
        }
        SessionAction::Configure {
            id,
            harness,
            model,
            effort,
            prompt,
            json,
        } => {
            if [&harness, &model, &effort, &prompt]
                .iter()
                .all(|f| f.is_none())
            {
                anyhow::bail!("nothing to change: give at least one flag (see --help)");
            }
            // The route replaces the whole configuration, as the detail's form
            // sends it: the current one, with the fields changed.
            let current = read(&daemon, &id)?;
            let config = configured(
                &current["config"],
                [
                    ("harness", harness),
                    ("model", model),
                    ("effort", effort),
                    ("prompt", prompt),
                ],
            );
            let session = daemon
                .put(&format!("/sessions/{}/config", segment(&id)), &config)?
                .ok()?
                .json()?;
            done(&id, &session, json, "configured");
        }
    }
    Ok(())
}

fn read(daemon: &Daemon, id: &str) -> Result<Value> {
    daemon
        .get(&format!("/sessions/{}/hosted", segment(id)))?
        .ok()?
        .json()
}

/// `current` with each field given set, or removed when given empty.
fn configured<const N: usize>(current: &Value, fields: [(&str, Option<String>); N]) -> Value {
    let mut config = current.as_object().cloned().unwrap_or_else(Map::new);
    for (key, value) in fields {
        match value {
            Some(v) if v.trim().is_empty() => {
                config.remove(key);
            }
            Some(v) => {
                config.insert(key.to_string(), json!(v));
            }
            None => {}
        }
    }
    Value::Object(config)
}

fn done(id: &str, session: &Value, json: bool, verb: &str) {
    if json {
        print_json(session);
    } else {
        println!(
            "{id} {verb} ({})",
            session["state"].as_str().unwrap_or("unknown")
        );
    }
}

fn status_line(status: &Value) -> String {
    let cap = match status["cap"].as_u64() {
        Some(cap) => cap.to_string(),
        None => "no cap".to_string(),
    };
    format!(
        "pdo {} · {} live session(s) / {cap} · last sweep {}",
        status["version"].as_str().unwrap_or(""),
        status["live"],
        status["reaper"]["last_sweep_at"]
            .as_str()
            .unwrap_or("never"),
    )
}

fn show_text(session: &Value) -> String {
    let text = |key: &str| session[key].as_str().unwrap_or("").to_string();
    let mut out = format!(
        "{}  {} {} of {} ({})\n",
        text("id"),
        text("kind"),
        text("key"),
        text("extension"),
        text("state")
    );
    if let Some(reason) = session["end_reason"].as_str() {
        out.push_str(&format!("ended: {reason}\n"));
    }
    if let Some(message) = session["wait_message"].as_str() {
        out.push_str(&format!("waiting: {message}\n"));
    }
    out.push_str(&format!("folder: {}\n", text("cwd")));
    if let Some(project) = session["project"].as_str() {
        out.push_str(&format!("project: {project}\n"));
    }
    for key in ["harness", "model", "effort", "prompt"] {
        if let Some(value) = session["config"][key].as_str() {
            out.push_str(&format!("{key}: {value}\n"));
        }
    }
    out
}

fn event_lines(events: &Value) -> String {
    events
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|e| {
            let by = e["by"]["id"]
                .as_str()
                .or_else(|| e["by"]["kind"].as_str())
                .unwrap_or("");
            format!(
                "{}\t{}\t{by}\t{}\n",
                e["at"].as_str().unwrap_or(""),
                e["kind"].as_str().unwrap_or(""),
                e["message"].as_str().unwrap_or(""),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_changes_only_the_fields_given() {
        let current = json!({ "harness": "claude", "model": "opus", "prompt": "hi" });
        let config = configured(
            &current,
            [
                ("model", Some("sonnet".to_string())),
                ("prompt", Some(" ".to_string())),
                ("effort", None),
            ],
        );
        assert_eq!(config, json!({ "harness": "claude", "model": "sonnet" }));
    }

    #[test]
    fn a_session_shows_as_lines() {
        let session = json!({
            "id": "hs-1", "extension": "test", "key": "k", "kind": "agent",
            "state": "ended", "end_reason": "terminated", "cwd": "/srv/web",
            "config": { "harness": "claude" }
        });
        assert_eq!(
            show_text(&session),
            "hs-1  agent k of test (ended)\nended: terminated\nfolder: /srv/web\nharness: claude\n"
        );
    }

    #[test]
    fn the_status_is_one_line() {
        let status = json!({
            "live": 2, "cap": 8, "version": "2.71.0",
            "reaper": { "last_sweep_at": null }
        });
        assert_eq!(
            status_line(&status),
            "pdo 2.71.0 · 2 live session(s) / 8 · last sweep never"
        );
    }
}
