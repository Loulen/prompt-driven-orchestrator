//! `pdo support`: your Agent de support, the agent the core holds for you
//! outside any Run, with your rights. Open it on an agentic profile or a
//! harness, ask it, read it, close it; and the core's code it reads.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{agent_choice, print_json, Daemon};
use serde_json::{json, Value};

#[derive(Subcommand, Debug)]
pub enum SupportAction {
    /// Open your support agent, on an agentic profile (`--profile`) or a
    /// harness (`--harness`, with `--model` and `--effort`), the question as
    /// its first message. Refused while one is open, unless `--replace`,
    /// which closes it first; refused on a harness not installed here.
    Open {
        /// The question, as its first message; none opens it waiting for one.
        question: Option<String>,
        /// The id of an agentic profile (`pdo settings agent-profile list`).
        #[arg(long, conflicts_with = "harness")]
        profile: Option<String>,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long, requires = "harness")]
        model: Option<String>,
        #[arg(long, requires = "harness")]
        effort: Option<String>,
        /// Close the support agent open now, if any, and open this one.
        #[arg(long)]
        replace: bool,
        #[arg(long)]
        json: bool,
    },
    /// Ask your open support agent a question: typed into its terminal and
    /// submitted.
    Ask {
        question: String,
        #[arg(long)]
        json: bool,
    },
    /// Your support agent: whether it lives, its agent, your last choice.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Close your support agent.
    Close {
        #[arg(long)]
        json: bool,
    },
    /// The folder of the core's code at this instance's tag: a shallow clone
    /// of the public repository, made the first time, then reused. Refused,
    /// with nothing cloned, when this version has no public tag.
    Source {
        #[arg(long)]
        json: bool,
    },
}

pub fn run_support(action: SupportAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        SupportAction::Open {
            question,
            profile,
            harness,
            model,
            effort,
            replace,
            json,
        } => {
            let Some(choice) = agent_choice(profile, harness, model, effort) else {
                anyhow::bail!("choose an agent: --profile <id>, or --harness <name>");
            };
            let mut body = json!({ "choice": choice, "replace": replace });
            if let Some(question) = question {
                body["question"] = question.into();
            }
            let status = daemon
                .post("/sessions/support", Some(&body))?
                .ok()?
                .json()?;
            report(&status, json, "opened");
        }
        SupportAction::Ask { question, json } => {
            let status = daemon
                .post(
                    "/sessions/support/ask",
                    Some(&json!({ "question": question })),
                )?
                .ok()?
                .json()?;
            report(&status, json, "asked");
        }
        SupportAction::Status { json } => {
            let status = daemon.get("/sessions/support")?.ok()?.json()?;
            if json {
                print_json(&status);
            } else {
                println!("{}", status_line(&status));
            }
        }
        SupportAction::Close { json } => {
            let status = daemon.delete("/sessions/support", None)?.ok()?.json()?;
            report(&status, json, "closed");
        }
        SupportAction::Source { json } => {
            let source: Value = daemon
                .post("/sessions/support/source", None)?
                .ok()?
                .json()?;
            if json {
                print_json(&source);
            } else {
                println!("{}", source["path"].as_str().unwrap_or_default());
            }
        }
    }
    Ok(())
}

fn report(status: &Value, json: bool, done: &str) {
    if json {
        print_json(status);
    } else {
        println!("{done}: {}", status_line(status));
    }
}

/// One line: the session, whether it lives, and its agent.
fn status_line(status: &Value) -> String {
    let session = status["session"].as_str().unwrap_or("-");
    if status["alive"] != true {
        return format!("{session}\tclosed");
    }
    let agent = &status["agent"];
    let combination: Vec<&str> = ["harness", "model", "effort"]
        .iter()
        .map(|k| agent[*k].as_str().unwrap_or("—"))
        .collect();
    format!(
        "{session}\topen\t{}\t{}",
        agent["label"].as_str().unwrap_or("-"),
        combination.join(" · ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_says_the_session_and_its_agent() {
        let open = json!({
            "session": "pdo-support-operator",
            "alive": true,
            "agent": { "label": "Deep", "harness": "claude", "model": "opus", "effort": null },
        });
        assert_eq!(
            status_line(&open),
            "pdo-support-operator\topen\tDeep\tclaude · opus · —"
        );
        let closed = json!({ "session": "pdo-support-operator", "alive": false, "agent": null });
        assert_eq!(status_line(&closed), "pdo-support-operator\tclosed");
    }
}
