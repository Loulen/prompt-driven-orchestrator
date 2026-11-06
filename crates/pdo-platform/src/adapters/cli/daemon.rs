//! The way a one-shot command reaches the running daemon: one blocking request
//! per call, as the operator (`X-PDO-Actor: cli`) or as the node session the
//! command runs in, with `PDO_TOKEN` when set.
//!
//! A command of an operator is a thin client of a route (ADR draft « chaque
//! route a sa commande CLI ») : it sends what the UI sends and prints what the
//! daemon answers. The rules stay behind the route.
//!
//! Write a call so the route coverage of the CLI can tie it to its route:
//! `daemon.post(&format!("/projects/{id}/members"), Some(&body))`, the path a
//! literal argument of the verb.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::domain::service_unit::DEFAULT_DAEMON_URL;

/// Exit code of a command whose subject the daemon does not know (`404`), the
/// one `pdo run status` gives an unknown Run.
pub const EXIT_NOT_FOUND: u8 = 4;

/// The daemon answered `404`: the subject of the command does not exist.
#[derive(Debug)]
pub struct NotFound(pub String);

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotFound {}

/// `0` on success; `4` with the daemon's sentence when it knows no such
/// subject; `1` with the error otherwise (unreachable daemon, refusal, invalid
/// request).
pub fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => match e.downcast_ref::<NotFound>() {
            Some(not_found) => {
                eprintln!("Error: {not_found}");
                ExitCode::from(EXIT_NOT_FOUND)
            }
            None => {
                eprintln!("Error: {e:?}");
                ExitCode::FAILURE
            }
        },
    }
}

/// A client of the running daemon (`PDO_DAEMON_URL`, else the default port).
pub struct Daemon {
    url: String,
    client: Client,
    session: Option<(String, String)>,
}

/// The daemon's answer to one request: its status and its body, as bytes (an
/// archive) or as text ([`Answer::text`]).
#[derive(Debug)]
pub struct Answer {
    pub status: StatusCode,
    pub bytes: Vec<u8>,
}

impl Daemon {
    /// The daemon of this process's environment, called as the node session
    /// the command runs in when it does.
    pub fn from_env() -> Self {
        let url =
            std::env::var("PDO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string());
        // No overall timeout: a call can last as long as the daemon's own work
        // on it (`pdo trigger fire` waits for its guard, up to the instance's
        // guard timeout). An unreachable daemon still fails at once.
        let client = Client::builder()
            .default_headers(pdo_kernel::env::cli_headers())
            .connect_timeout(Duration::from_secs(10))
            .timeout(None::<Duration>)
            .build()
            .unwrap_or_default();
        Self {
            url: url.trim_end_matches('/').to_string(),
            client,
            session: session_env_claim(),
        }
    }

    pub fn get(&self, path: &str) -> Result<Answer> {
        self.send(Method::GET, path, None)
    }

    pub fn post(&self, path: &str, body: Option<&Value>) -> Result<Answer> {
        self.send(Method::POST, path, body)
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<Answer> {
        self.send(Method::PUT, path, Some(body))
    }

    pub fn patch(&self, path: &str, body: &Value) -> Result<Answer> {
        self.send(Method::PATCH, path, Some(body))
    }

    pub fn delete(&self, path: &str, body: Option<&Value>) -> Result<Answer> {
        self.send(Method::DELETE, path, body)
    }

    /// A `POST` whose body is not JSON: an archive, under its content type.
    pub fn post_bytes(&self, path: &str, content_type: &str, body: Vec<u8>) -> Result<Answer> {
        self.send_raw(Method::POST, path, content_type, body)
    }

    /// A `POST` of a multipart form: files, as a browser uploads them.
    pub fn post_multipart(
        &self,
        path: &str,
        form: reqwest::blocking::multipart::Form,
    ) -> Result<Answer> {
        answer(self.request(Method::POST, path).multipart(form))
    }

    /// A `PUT` whose body is plain text: a file's content, as the editor saves it.
    pub fn put_text(&self, path: &str, text: String) -> Result<Answer> {
        self.send_raw(
            Method::PUT,
            path,
            "text/plain; charset=utf-8",
            text.into_bytes(),
        )
    }

    fn send(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Answer> {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(body);
        }
        answer(request)
    }

    fn send_raw(
        &self,
        method: Method,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<Answer> {
        answer(
            self.request(method, path)
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(body),
        )
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.identified(self.client.request(method, format!("{}{path}", self.url)))
    }

    fn identified(&self, request: RequestBuilder) -> RequestBuilder {
        let mut request = request.header("X-PDO-Actor", "cli");
        if let Some((run_id, node_id)) = &self.session {
            request = request
                .header(SESSION_RUN_HEADER, run_id)
                .header(SESSION_NODE_HEADER, node_id);
        }
        request
    }
}

fn answer(request: RequestBuilder) -> Result<Answer> {
    let response = request.send().context("failed to reach daemon")?;
    let status = response.status();
    let bytes = response.bytes().map(|b| b.to_vec()).unwrap_or_default();
    Ok(Answer { status, bytes })
}

impl Answer {
    /// The body as text, any invalid UTF-8 replaced.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    /// The answer when it is a success; [`NotFound`] for a `404`; the daemon's
    /// sentence for any other refusal.
    pub fn ok(self) -> Result<Self> {
        if self.status.is_success() {
            return Ok(self);
        }
        let sentence = refusal_sentence(&self.text());
        if self.status == StatusCode::NOT_FOUND {
            return Err(NotFound(if sentence.is_empty() {
                "not found".to_string()
            } else {
                sentence
            })
            .into());
        }
        anyhow::bail!("daemon returned {}: {sentence}", self.status)
    }

    /// The body as JSON (`null` for an empty body, a `204`).
    pub fn json(&self) -> Result<Value> {
        let text = self.text();
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).context("the daemon's answer is not JSON")
    }
}

/// The daemon's sentence for a refusal: its `message`, else its `error`, else
/// the body as it is.
pub fn refusal_sentence(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.trim().to_string())
}

/// The node session this CLI runs in, when it does: both `PDO_RUN_ID` and
/// `PDO_NODE_ID`, non-blank.
pub fn session_env_claim() -> Option<(String, String)> {
    let clean = |v: Result<String, std::env::VarError>| {
        v.ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let run_id = clean(std::env::var("PDO_RUN_ID"))?;
    let node_id = clean(std::env::var("PDO_NODE_ID"))?;
    Some((run_id, node_id))
}

/// A path segment as it travels in a URL: every byte outside the unreserved
/// set percent-encoded.
pub fn segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A repository path as the UI sends it: absolute, without `.`, `..` or a
/// trailing `/`. A relative one is resolved from the working directory. The
/// normalisation is lexical, symlinks are not followed: the daemon compares
/// repository paths verbatim.
pub fn absolute_path(raw: &str) -> Result<String> {
    let path = Path::new(raw.trim());
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("cannot read the working directory")?
            .join(path)
    };
    let mut normal = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other.as_os_str()),
        }
    }
    Ok(normal.display().to_string())
}

/// The agentic-profile union a Projet or a Trigger carries, from the flags of
/// its command: a profile, or an inline harness with its model and effort.
/// `None` when neither is given.
pub fn agent_choice(
    profile: Option<String>,
    harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Option<Value> {
    if let Some(profile) = profile {
        return Some(serde_json::json!({ "mode": "profile", "profile_id": profile }));
    }
    let mut custom = serde_json::json!({ "mode": "custom", "harness": harness? });
    if let Some(model) = model {
        custom["model"] = model.into();
    }
    if let Some(effort) = effort {
        custom["effort"] = effort.into();
    }
    Some(custom)
}

/// `value` pretty-printed, as `--json` prints it.
pub fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

/// The bytes a `--file <path>` names, `-` for the standard input.
pub fn read_file_bytes(path: &str) -> Result<Vec<u8>> {
    if path == "-" {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)
            .context("cannot read the standard input")?;
        return Ok(bytes);
    }
    std::fs::read(path).with_context(|| format!("cannot read {path}"))
}

/// A `<key>=<value>` of a command line. The value is read as JSON when it is
/// JSON (`5`, `true`, `null`, `"quoted"`, `[…]`, `{…}`), as a string otherwise:
/// `public_url=https://pdo.example.net` sends a string, `session_cap=5` a
/// number, `session_cap='"5"'` the string `"5"`.
pub fn assignment(raw: &str) -> Result<(String, Value)> {
    let (key, value) = raw
        .split_once('=')
        .with_context(|| format!("expected <key>=<value>, got {raw:?}"))?;
    let key = key.trim();
    anyhow::ensure!(!key.is_empty(), "expected <key>=<value>, got {raw:?}");
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()));
    Ok((key.to_string(), value))
}

/// Who still names a subject (a skill, a profile), as a referents answer lists
/// them: one tab-separated line each, the tier then its id and name (`runs`
/// name a Run by `run_id`), `instance` when the instance's defaults name it.
/// `none` when nobody does.
pub fn referent_lines(referents: &Value) -> String {
    let mut out = String::new();
    let Some(tiers) = referents.as_object() else {
        return "none\n".to_string();
    };
    for (tier, value) in tiers {
        match value {
            Value::Bool(true) if matches!(tier.as_str(), "instance" | "instance_default") => {
                out.push_str("instance\n");
            }
            Value::Array(items) => {
                for item in items {
                    let id = item["id"]
                        .as_str()
                        .or_else(|| item["run_id"].as_str())
                        .unwrap_or("");
                    let mut line = format!("{tier}\t{id}\t{}", item["name"].as_str().unwrap_or(""));
                    if let Some(node) = item["node_id"].as_str() {
                        line.push_str(&format!("\tnode {node}"));
                    }
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        out.push_str("none\n");
    }
    out
}

/// The JSON a `--file <path>` names, `-` for the standard input.
pub fn read_json_file(path: &str) -> Result<Value> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("cannot read the standard input")?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("cannot read {path}"))?
    };
    serde_json::from_str(&text).with_context(|| format!("{path} is not JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, body: &str) -> Answer {
        Answer {
            status: StatusCode::from_u16(status).unwrap(),
            bytes: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_404_is_not_found_with_the_daemon_s_sentence() {
        let err = answer(404, r#"{"error":"trigger not found"}"#)
            .ok()
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<NotFound>().unwrap().0,
            "trigger not found"
        );
        let err = answer(404, "no such project").ok().unwrap_err();
        assert_eq!(err.downcast_ref::<NotFound>().unwrap().0, "no such project");
    }

    #[test]
    fn another_refusal_carries_its_status_and_sentence() {
        let err = answer(400, r#"{"error":"invalid cron expression: x"}"#)
            .ok()
            .unwrap_err();
        assert!(err.downcast_ref::<NotFound>().is_none());
        assert_eq!(
            err.to_string(),
            "daemon returned 400 Bad Request: invalid cron expression: x"
        );
        let err = answer(403, r#"{"error":"forbidden","message":"you may not"}"#)
            .ok()
            .unwrap_err();
        assert!(err.to_string().ends_with("you may not"), "{err}");
    }

    #[test]
    fn an_empty_body_reads_as_null() {
        assert_eq!(answer(204, "").json().unwrap(), Value::Null);
    }

    #[test]
    fn a_segment_is_percent_encoded() {
        assert_eq!(segment("prj-1"), "prj-1");
        assert_eq!(segment("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn a_path_is_made_absolute_and_normal() {
        assert_eq!(absolute_path("/srv/web/").unwrap(), "/srv/web");
        assert_eq!(absolute_path("/srv/./api/../web").unwrap(), "/srv/web");
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(absolute_path(".").unwrap(), cwd.display().to_string());
        assert_eq!(
            absolute_path("../web").unwrap(),
            cwd.parent().unwrap().join("web").display().to_string()
        );
    }

    #[test]
    fn an_agent_choice_is_a_profile_or_an_inline_combination() {
        assert_eq!(agent_choice(None, None, None, None), None);
        assert_eq!(
            agent_choice(Some("reviewer".into()), None, None, None),
            Some(serde_json::json!({ "mode": "profile", "profile_id": "reviewer" }))
        );
        assert_eq!(
            agent_choice(None, Some("claude".into()), None, Some("high".into())),
            Some(serde_json::json!({ "mode": "custom", "harness": "claude", "effort": "high" }))
        );
    }

    #[test]
    fn an_assignment_reads_its_value_as_json_or_as_a_string() {
        assert_eq!(
            assignment("session_cap=5").unwrap(),
            ("session_cap".into(), serde_json::json!(5))
        );
        assert_eq!(
            assignment("public_url=https://pdo.example.net").unwrap(),
            ("public_url".into(), serde_json::json!("https://pdo.example.net"))
        );
        assert_eq!(
            assignment(r#"session_cap="5""#).unwrap().1,
            serde_json::json!("5")
        );
        assert_eq!(assignment("default_sandbox=").unwrap().1, serde_json::json!(""));
        assert!(assignment("no-equal-sign").is_err());
        assert!(assignment("=5").is_err());
    }

    #[test]
    fn referents_are_one_line_per_tier_entry() {
        let referents = serde_json::json!({
            "skill_id": "s-1",
            "instance": true,
            "projects": [{ "id": "prj-1", "name": "Web" }],
            "pipelines": [{ "id": "p", "name": "Ship", "node_id": "n1", "scope": "instance" }],
            "runs": [{ "run_id": "r-1", "name": "nightly" }],
        });
        assert_eq!(
            referent_lines(&referents),
            "instance\npipelines\tp\tShip\tnode n1\nprojects\tprj-1\tWeb\nruns\tr-1\tnightly\n"
        );
        assert_eq!(
            referent_lines(&serde_json::json!({ "instance": false, "projects": [] })),
            "none\n"
        );
    }

    #[test]
    fn the_exit_code_of_a_not_found_is_4() {
        assert_eq!(
            exit_code(Err(NotFound("x".into()).into())),
            ExitCode::from(EXIT_NOT_FOUND)
        );
        assert_eq!(exit_code(Ok(())), ExitCode::SUCCESS);
        assert_eq!(exit_code(Err(anyhow::anyhow!("x"))), ExitCode::FAILURE);
    }
}
