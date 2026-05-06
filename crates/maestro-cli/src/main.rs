use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

const DEFAULT_DAEMON_URL: &str = "http://localhost:5172";

#[derive(Parser)]
#[command(
    name = "maestro",
    about = "Maestro — deterministic Claude Code pipeline orchestrator"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Maestro daemon (alias: run `maestro-daemon` directly)
    Daemon {
        #[arg(short, long, env = "MAESTRO_PORT", default_value_t = 5172)]
        port: u16,
    },
    /// Signal that the current NodeRun has completed successfully
    Complete,
    /// Signal that the current NodeRun has failed
    Fail {
        #[arg(long)]
        reason: String,
    },
}

fn daemon_url() -> String {
    std::env::var("MAESTRO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string())
}

fn run_id() -> Result<String> {
    std::env::var("MAESTRO_RUN_ID").context(
        "MAESTRO_RUN_ID not set — this command must be run inside a Maestro NodeRun session",
    )
}

fn node_id() -> Result<String> {
    std::env::var("MAESTRO_NODE_ID").context(
        "MAESTRO_NODE_ID not set — this command must be run inside a Maestro NodeRun session",
    )
}

fn node_iter() -> i64 {
    std::env::var("MAESTRO_NODE_ITER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon { port } => {
            eprintln!(
                "Use `maestro-daemon --port {port}` to start the daemon directly.\n\
                 The `maestro daemon` subcommand will proxy to it in a future slice."
            );
        }
        Commands::Complete => {
            let url = daemon_url();
            let rid = run_id()?;
            let nid = node_id()?;
            let iter = node_iter();

            let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/done");
            let client = reqwest::blocking::Client::new();
            let resp = client
                .post(&endpoint)
                .json(&serde_json::json!({ "iter": iter }))
                .send()
                .context("failed to reach daemon")?;

            if resp.status().is_success() {
                eprintln!("Node {nid} marked complete.");
            } else {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                anyhow::bail!("daemon returned {status}: {body}");
            }
        }
        Commands::Fail { reason } => {
            let url = daemon_url();
            let rid = run_id()?;
            let nid = node_id()?;
            let iter = node_iter();

            let endpoint = format!("{url}/runs/{rid}/nodes/{nid}/fail");
            let client = reqwest::blocking::Client::new();
            let resp = client
                .post(&endpoint)
                .json(&serde_json::json!({ "reason": reason, "iter": iter }))
                .send()
                .context("failed to reach daemon")?;

            if resp.status().is_success() {
                eprintln!("Node {nid} marked failed.");
            } else {
                let status = resp.status();
                let body = resp.text().unwrap_or_default();
                anyhow::bail!("daemon returned {status}: {body}");
            }
        }
    }
    Ok(())
}
