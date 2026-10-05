//! `pdo run list`: the Runs the daemon knows, over `GET /runs`.

use anyhow::{Context, Result};
use serde_json::Value;

use super::cli_daemon_url;

pub fn run_run_list(json: bool) -> Result<()> {
    let url = cli_daemon_url();
    let resp = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default()
        .get(format!("{url}/runs"))
        .header("X-PDO-Actor", "cli")
        .send()
        .context("failed to reach daemon")?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("daemon returned {status}: {}", refusal_message(&body));
    }
    if json {
        println!("{body}");
        return Ok(());
    }
    let runs: Vec<Value> = serde_json::from_str(&body).context("the daemon's runs list")?;
    print!("{}", table(&runs));
    Ok(())
}

/// The daemon's sentence for a refusal (`message`, else `error`), else the
/// body as is.
fn refusal_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.to_string())
}

/// One line per Run: `run_id  status  pipeline  name`, tab-separated.
fn table(runs: &[Value]) -> String {
    let field = |run: &Value, key: &str| run[key].as_str().unwrap_or("").to_string();
    runs.iter()
        .map(|run| {
            format!(
                "{}\t{}\t{}\t{}\n",
                field(run, "run_id"),
                field(run, "status"),
                field(run, "pipeline_name"),
                field(run, "name"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_one_tab_separated_line() {
        let runs = vec![serde_json::json!({
            "run_id": "r1", "status": "running", "pipeline_name": "p", "name": "Nightly"
        })];
        assert_eq!(table(&runs), "r1\trunning\tp\tNightly\n");
        assert_eq!(table(&[]), "");
    }

    #[test]
    fn a_refusal_reads_the_daemon_s_message() {
        assert_eq!(
            refusal_message(
                r#"{"error":"unauthenticated","message":"authentication required: x"}"#
            ),
            "authentication required: x"
        );
        assert_eq!(refusal_message("oops"), "oops");
    }
}
