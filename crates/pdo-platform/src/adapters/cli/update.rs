//! `pdo update`: the version and update section of Settings › General, over
//! `/update*`. A thin client: the release source, whether an update may run
//! now (no Run in flight, a known install method) stay behind the route.

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::daemon::{print_json, segment, Daemon};

#[derive(Subcommand, Debug)]
pub enum UpdateAction {
    /// The installed version, the latest one known and when it was checked,
    /// whether an update may run now. Reads the last check, never the network.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Check the release source now, then print the status.
    Check {
        #[arg(long)]
        json: bool,
    },
    /// « What's new »: the changelog of the releases.
    Changelog {
        #[arg(long)]
        json: bool,
    },
    /// Update now: the daemon runs the install method's command detached,
    /// then restarts. Prints the attempt; `pdo update log <attempt>` follows it.
    Apply {
        #[arg(long)]
        json: bool,
    },
    /// The journal of an update attempt.
    Log { attempt: String },
}

pub fn run_update(action: UpdateAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        UpdateAction::Status { json } => {
            let status = daemon.get("/update")?.ok()?.json()?;
            status_out(&status, json);
        }
        UpdateAction::Check { json } => {
            let status = daemon.post("/update/check", None)?.ok()?.json()?;
            status_out(&status, json);
        }
        UpdateAction::Changelog { json } => {
            let changelog = daemon.get("/update/changelog")?.ok()?.json()?;
            if json {
                print_json(&changelog);
            } else {
                print!("{}", changelog_text(&changelog));
            }
        }
        UpdateAction::Apply { json } => {
            let attempt = daemon.post("/update/apply", None)?.ok()?.json()?;
            if json {
                print_json(&attempt);
            } else {
                println!(
                    "update {} started: {} (log: pdo update log {})",
                    attempt["attempt_id"].as_str().unwrap_or("?"),
                    attempt["command"].as_str().unwrap_or(""),
                    attempt["attempt_id"].as_str().unwrap_or("?"),
                );
            }
        }
        UpdateAction::Log { attempt } => {
            let log = daemon
                .get(&format!("/update/attempts/{}/log", segment(&attempt)))?
                .ok()?;
            print!("{}", log.text());
        }
    }
    Ok(())
}

fn status_out(status: &Value, json: bool) {
    if json {
        print_json(status);
    } else {
        print!("{}", status_text(status));
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("-")
}

/// The version section in a few lines.
pub(super) fn status_text(status: &Value) -> String {
    let mut out = format!(
        "installed {} · latest {}{}\n",
        text(&status["installed_version"]),
        text(&status["latest_version"]),
        if status["newer_available"] == true {
            " (newer available)"
        } else {
            ""
        }
    );
    out.push_str(&format!(
        "checked {} · check {}\n",
        text(&status["checked_at"]),
        if status["check_enabled"] == true {
            "on"
        } else {
            "off"
        }
    ));
    if status["can_apply"] == true {
        out.push_str("can update now: pdo update apply\n");
    } else if let Some(reason) = status["apply_blocked_reason"].as_str() {
        out.push_str(&format!("cannot update now: {reason}\n"));
    }
    if let Some(error) = status["last_error"].as_str() {
        out.push_str(&format!("last error: {error}\n"));
    }
    out
}

/// The changelog's markdown, after why it is the embedded one when the
/// releases could not be read.
fn changelog_text(changelog: &Value) -> String {
    let mut out = String::new();
    if let Some(reason) = changelog["fallback_reason"].as_str() {
        out.push_str(&format!("({reason})\n\n"));
    }
    out.push_str(changelog["markdown"].as_str().unwrap_or(""));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_status_says_the_versions_and_whether_an_update_may_run() {
        let status = json!({
            "installed_version": "2.71.0", "latest_version": "2.72.0", "newer_available": true,
            "checked_at": "2026-11-06T02:00:00Z", "check_enabled": true,
            "can_apply": false, "apply_blocked_reason": "2 Runs in flight", "last_error": null,
        });
        assert_eq!(
            status_text(&status),
            "installed 2.71.0 · latest 2.72.0 (newer available)\n\
             checked 2026-11-06T02:00:00Z · check on\n\
             cannot update now: 2 Runs in flight\n"
        );
    }
}
