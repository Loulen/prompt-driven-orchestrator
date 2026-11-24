//! `pdo bug-report`: the Destinations de bug report, over `/bug-report*`. A
//! thin client: which destinations the caller may send to, the checks of a
//! report (a description, the prompts only where they may go) and the
//! remembered choice stay behind the routes.

use std::path::Path;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use clap::Subcommand;
use serde_json::{json, Value};

use super::daemon::{print_json, read_file_bytes, read_text, Daemon};

#[derive(Subcommand, Debug)]
pub enum BugReportAction {
    /// The destinations the extensions offer you (`<extension>/<id>`), and
    /// the one you chose last. The core's own, `github`, opens in the
    /// browser from the Bug report window.
    Destinations {
        #[arg(long)]
        json: bool,
    },
    /// Send a bug report to an extension's destination; prints how the item
    /// it created is named and where it is. Exits 4 when no such destination
    /// is offered to you.
    Send {
        /// `<extension>/<id>`, as `pdo bug-report destinations` lists it.
        destination: String,
        /// The description of the bug; `-` reads it from the standard input.
        #[arg(long)]
        description: String,
        /// A screenshot to join (an image file).
        #[arg(long, value_name = "FILE")]
        screenshot: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Remember a destination as your last choice: « Send to » starts on it.
    Last { destination: String },
}

pub fn run_bug_report(action: BugReportAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        BugReportAction::Destinations { json } => {
            let answer = daemon.get("/bug-report/destinations")?.ok()?.json()?;
            if json {
                print_json(&answer);
            } else {
                print!("{}", destinations_text(&answer));
            }
        }
        BugReportAction::Send {
            destination,
            description,
            screenshot,
            json,
        } => {
            let description = if description == "-" {
                read_text("-")?
            } else {
                description
            };
            let screenshot = match screenshot {
                Some(path) => Some((media_type(&path), read_file_bytes(&path)?)),
                None => None,
            };
            let body = report_body(&destination, description.trim(), screenshot);
            let sent = daemon.post("/bug-report", Some(&body))?.ok()?.json()?;
            if json {
                print_json(&sent);
            } else {
                println!(
                    "Bug report sent: {} {}",
                    sent["reference"].as_str().unwrap_or("?"),
                    sent["link"].as_str().unwrap_or("")
                );
            }
        }
        BugReportAction::Last { destination } => {
            daemon
                .put("/bug-report/last", &json!({ "destination": destination }))?
                .ok()?;
        }
    }
    Ok(())
}

/// The media type of an image file, from its name.
fn media_type(path: &str) -> String {
    mime_guess::from_path(Path::new(path))
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

/// What `POST /bug-report` receives: the description as the report's only
/// section, and the screenshot in base64.
fn report_body(
    destination: &str,
    description: &str,
    screenshot: Option<(String, Vec<u8>)>,
) -> Value {
    let mut body = json!({
        "destination": destination,
        "text": format!("### Description\n{description}"),
        "fields": { "description": description },
    });
    if let Some((media_type, bytes)) = screenshot {
        body["screenshot"] = json!({ "media_type": media_type, "data": BASE64.encode(bytes) });
    }
    body
}

/// One line per destination, `id  label  [prompts]`, the last choice marked.
fn destinations_text(answer: &Value) -> String {
    let last = answer["last"].as_str();
    let mut out = String::new();
    for d in answer["destinations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let id = d["id"].as_str().unwrap_or("");
        let mut line = format!("{id}\t{}", d["label"].as_str().unwrap_or(""));
        if d["accepts_prompts"].as_bool() == Some(true) {
            line.push_str("\taccepts prompts");
        }
        if last == Some(id) {
            line.push_str("\t(last used)");
        }
        out.push_str(&line);
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str("No extension offers a bug report destination.\n");
    }
    if let Some(last) = last.filter(|l| !answer_lists(answer, l)) {
        out.push_str(&format!("Last used: {last}\n"));
    }
    out
}

fn answer_lists(answer: &Value, id: &str) -> bool {
    answer["destinations"]
        .as_array()
        .is_some_and(|ds| ds.iter().any(|d| d["id"] == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_destinations_read_one_per_line_the_last_one_marked() {
        let answer = json!({
            "destinations": [
                { "id": "desk/inbox", "label": "Inbox", "accepts_prompts": true },
                { "id": "desk/archive", "label": "Archive", "accepts_prompts": false }
            ],
            "last": "desk/archive"
        });
        assert_eq!(
            destinations_text(&answer),
            "desk/inbox\tInbox\taccepts prompts\ndesk/archive\tArchive\t(last used)\n"
        );
        let core = json!({ "destinations": [], "last": "github" });
        assert_eq!(
            destinations_text(&core),
            "No extension offers a bug report destination.\nLast used: github\n"
        );
    }

    #[test]
    fn a_report_carries_its_description_and_its_screenshot_in_base64() {
        let body = report_body(
            "desk/inbox",
            "It breaks",
            Some(("image/png".into(), b"png".to_vec())),
        );
        assert_eq!(body["text"], "### Description\nIt breaks");
        assert_eq!(body["fields"]["description"], "It breaks");
        assert_eq!(body["screenshot"]["media_type"], "image/png");
        assert_eq!(body["screenshot"]["data"], "cG5n");
        assert!(report_body("desk/inbox", "x", None)
            .get("screenshot")
            .is_none());
        assert_eq!(media_type("shot.png"), "image/png");
    }
}
