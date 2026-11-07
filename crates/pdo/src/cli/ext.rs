//! `pdo ext <id> <args…>` (#33, ADR-0081): an extension's command line,
//! relayed to the daemon that runs it. The CLI never builds the extensions
//! (amendment #23): it sends the arguments, its standard input when it is a
//! pipe or a file, its working directory, with the node session it runs in
//! (`PDO_RUN_ID`, `PDO_NODE_ID`), then writes the extension's stdout and stderr byte for byte and exits with
//! its code.
//!
//! Exit codes: the extension's, relayed as is; `1` when the daemon cannot be
//! reached or gives no verdict (the extension failed); `127`
//! ([`EXIT_NO_CLI`]) when the daemon has no extension `<id>`, or one without a
//! command line. `pdo ext` alone lists the extensions that have one.

#[cfg(not(unix))]
use std::io::IsTerminal;
use std::io::{Read, Write};
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use pdo_extension_api::EXIT_NO_CLI;
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_kernel::env::DEFAULT_DAEMON_URL;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;

use crate::extension_cli::{ExtensionCliRequest, ExtensionCliResponse};

/// `pdo ext [<id> <args…>]`.
pub fn run_ext(args: Vec<String>) -> ExitCode {
    let daemon = Daemon::from_env();
    match args.split_first() {
        None => list(&daemon),
        Some((id, args)) => relay(&daemon, id, args.to_vec()),
    }
}

/// The daemon the CLI talks to, and the node session it runs in.
struct Daemon {
    url: String,
    client: Client,
    session: Option<(String, String)>,
}

impl Daemon {
    fn from_env() -> Self {
        let clean = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let session = clean("PDO_RUN_ID").zip(clean("PDO_NODE_ID"));
        // No timeout: an extension's command may take its time (a backup).
        let client = Client::builder()
            .default_headers(pdo_kernel::env::cli_headers())
            .timeout(None)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            url: clean("PDO_DAEMON_URL").unwrap_or_else(|| DEFAULT_DAEMON_URL.to_string()),
            client,
            session,
        }
    }

    /// `request`, from the CLI of this node session, if any.
    fn identified(&self, request: RequestBuilder) -> RequestBuilder {
        let request = request.header("X-PDO-Actor", "cli");
        match &self.session {
            Some((run, node)) => request
                .header(SESSION_RUN_HEADER, run)
                .header(SESSION_NODE_HEADER, node),
            None => request,
        }
    }

    fn unreachable(&self, error: reqwest::Error) -> ExitCode {
        eprintln!("Error: failed to reach daemon at {}: {error}", self.url);
        ExitCode::FAILURE
    }
}

/// `pdo ext`: the extensions that have a command line, one per line.
fn list(daemon: &Daemon) -> ExitCode {
    let response = daemon
        .identified(daemon.client.get(format!("{}/ext", daemon.url)))
        .send();
    let entries: Vec<serde_json::Value> = match response.and_then(|r| r.error_for_status()) {
        Ok(response) => match response.json() {
            Ok(entries) => entries,
            Err(e) => return daemon.unreachable(e),
        },
        Err(e) => return daemon.unreachable(e),
    };
    let with_cli: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?;
            let summary = entry["cli"]["summary"].as_str()?;
            Some((id, summary))
        })
        .collect();
    if with_cli.is_empty() {
        eprintln!("No extension of this daemon has a command line.");
    }
    for (id, summary) in with_cli {
        println!("{id}\t{summary}");
    }
    ExitCode::SUCCESS
}

/// `pdo ext <id> <args…>`: the extension's answer, or why there is none.
fn relay(daemon: &Daemon, id: &str, args: Vec<String>) -> ExitCode {
    let stdin = match read_stdin() {
        Ok(stdin) => stdin,
        Err(e) => {
            eprintln!("Error: failed to read the standard input: {e}");
            return ExitCode::FAILURE;
        }
    };
    let request = ExtensionCliRequest {
        args,
        stdin: BASE64.encode(stdin),
        cwd: std::env::current_dir()
            .ok()
            .map(|dir| dir.display().to_string()),
    };
    let response = daemon
        .identified(daemon.client.post(format!("{}/ext-cli/{id}", daemon.url)))
        .json(&request)
        .send();
    let response = match response {
        Ok(response) => response,
        Err(e) => return daemon.unreachable(e),
    };
    let status = response.status();
    if status == StatusCode::OK {
        return match response.json::<ExtensionCliResponse>() {
            Ok(answer) => write_answer(&answer),
            Err(e) => daemon.unreachable(e),
        };
    }
    let body: serde_json::Value = response.json().unwrap_or_default();
    let message = body["message"].as_str().unwrap_or("no message");
    match body["error"].as_str() {
        Some("extension_not_found") | Some("cli_not_found") => {
            eprintln!("Error: {message}. `pdo ext` lists the extensions that have a command line.");
            ExitCode::from(EXIT_NO_CLI)
        }
        Some(pdo_extension_api::OPERATION_REFUSED) => {
            let denied = pdo_extension_api::Denied::from_body(&body)
                .map(|denied| denied.for_an_agent())
                .unwrap_or_else(|| message.to_string());
            eprintln!("Error: {denied}");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("Error: the daemon answered {status}: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Everything on the standard input when it is a pipe (`… | pdo ext …`) or a
/// file (`pdo ext … < file`), which both end; nothing otherwise. A terminal
/// would wait for the operator, and an agent's shell tool hands its commands a
/// standard input that never ends (a socket that stays open, in Claude Code's
/// Bash tool): reading it would block `pdo ext` forever.
fn read_stdin() -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if stdin_ends() {
        std::io::stdin().read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

#[cfg(unix)]
fn stdin_ends() -> bool {
    use std::os::fd::AsFd;
    use std::os::unix::fs::FileTypeExt;
    let Ok(fd) = std::io::stdin().as_fd().try_clone_to_owned() else {
        return false; // closed
    };
    match std::fs::File::from(fd).metadata() {
        Ok(meta) => meta.file_type().is_fifo() || meta.file_type().is_file(),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn stdin_ends() -> bool {
    !std::io::stdin().is_terminal()
}

fn write_answer(answer: &ExtensionCliResponse) -> ExitCode {
    let (Ok(stdout), Ok(stderr)) = (BASE64.decode(&answer.stdout), BASE64.decode(&answer.stderr))
    else {
        eprintln!("Error: the daemon answered an output that is not base64");
        return ExitCode::FAILURE;
    };
    let _ = std::io::stdout().write_all(&stdout);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(&stderr);
    ExitCode::from(answer.code)
}
