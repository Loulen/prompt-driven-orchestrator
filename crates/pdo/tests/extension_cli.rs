//! Stage 4 (ADR-0080) — `pdo ext <id> <args…>` (#33, ADR-0081): the real `pdo`
//! binary, in a subprocess as an agent runs it, against a daemon serving the
//! internal test extension.
//!
//! The contract: the arguments and the standard input reach the extension's
//! `run_cli` with the caller's Actor (the agent of the node session, the local
//! operator otherwise); its stdout and stderr come back byte for byte, its exit
//! code as is; `1` when the daemon cannot be reached or the extension fails;
//! `127` when the daemon has no such extension, or one without a command line.

use std::io::Write;
use std::process::{Command, Stdio};

use pdo::extension_api::{Actor, Extension, Manifest};
use serde_json::{json, Value};

use crate::common::TestDaemon;
use crate::test_extension::{self, Refusal, TestExtension};

/// An extension without a command line.
struct Mute;

impl Extension for Mute {
    fn manifest(&self) -> Manifest {
        Manifest::new("mute", "Mute", "1.0.0")
    }
}

async fn daemon() -> (TestDaemon, test_extension::Control) {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let extensions: Vec<Box<dyn Extension>> = vec![Box::new(ext), Box::new(Mute)];
    let daemon = TestDaemon::spawn_with_extensions(|_| Ok(()), extensions)
        .await
        .unwrap();
    (daemon, control)
}

struct Output {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

impl Output {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

impl std::fmt::Display for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit={:?}\n--- stdout\n{}\n--- stderr\n{}",
            self.code,
            self.stdout(),
            self.stderr
        )
    }
}

/// `pdo <args>` against `url`, from the node session `session` if any, fed
/// `stdin` (none: `/dev/null`), on a blocking task so the daemon keeps serving.
async fn pdo(
    url: &str,
    args: &[&str],
    session: Option<(&str, &str)>,
    stdin: Option<&[u8]>,
) -> Output {
    let url = url.to_string();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let session = session.map(|(r, n)| (r.to_string(), n.to_string()));
    let stdin = stdin.map(<[u8]>::to_vec);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        cmd.args(&args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("PDO_DAEMON_URL", &url);
        if let Some((run, node)) = &session {
            cmd.env("PDO_RUN_ID", run).env("PDO_NODE_ID", node);
        }
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn pdo");
        if let Some(bytes) = stdin {
            let mut pipe = child.stdin.take().unwrap();
            pipe.write_all(&bytes).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        Output {
            code: out.status.code(),
            stdout: out.stdout,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn from_a_node_session_the_extension_sees_the_agent_and_its_output_comes_back() {
    let (daemon, control) = daemon().await;
    let out = pdo(
        &daemon.url(),
        &["ext", "test", "echo", "salut"],
        Some(("run-1", "node-a")),
        None,
    )
    .await;
    assert_eq!(out.code, Some(0), "{out}");
    assert_eq!(out.stdout(), "salut\n", "{out}");
    assert_eq!(out.stderr, "actor: agent:run-1/node-a\n", "{out}");

    let calls = control.cli_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args, vec!["echo", "salut"]);
    assert_eq!(
        calls[0].actor,
        Actor::Agent {
            run_id: "run-1".into(),
            node_id: "node-a".into()
        }
    );
    assert!(calls[0].stdin.is_empty(), "/dev/null relays nothing");
    assert!(calls[0].cwd.is_some(), "the caller's working directory");
}

#[tokio::test(flavor = "multi_thread")]
async fn outside_a_session_the_actor_is_the_local_operator() {
    let (daemon, control) = daemon().await;
    let out = pdo(&daemon.url(), &["ext", "test", "echo", "salut"], None, None).await;
    assert_eq!(out.code, Some(0), "{out}");
    assert_eq!(out.stderr, "actor: local_operator\n", "{out}");
    assert_eq!(control.cli_calls()[0].actor, Actor::LocalOperator);
}

#[tokio::test(flavor = "multi_thread")]
async fn exit_codes_flags_and_bytes_are_relayed_as_they_are() {
    let (daemon, _) = daemon().await;
    let url = daemon.url();
    let out = pdo(&url, &["ext", "test", "exit", "42"], None, None).await;
    assert_eq!(out.code, Some(42), "{out}");
    let out = pdo(&url, &["ext", "test", "exit", "3"], None, None).await;
    assert_eq!(out.code, Some(3), "{out}");

    // Flags after the id belong to the extension, not to `pdo`.
    let out = pdo(&url, &["ext", "test", "echo", "--json", "-x"], None, None).await;
    assert_eq!(
        (out.code, out.stdout()),
        (Some(0), "--json -x\n".into()),
        "{out}"
    );

    // Standard input in, standard output out, byte for byte.
    let bytes = [0u8, 159, 146, 150, b'\n', 255];
    let out = pdo(&url, &["ext", "test", "cat"], None, Some(&bytes)).await;
    assert_eq!(out.code, Some(0), "{out}");
    assert_eq!(out.stdout, bytes, "{out}");

    let out = pdo(&url, &["ext", "test", "nope"], None, None).await;
    assert_eq!(out.code, Some(2), "{out}");
    assert_eq!(out.stderr, "unknown command\n", "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_absent_extension_or_one_without_a_command_line_exits_127() {
    let (daemon, _) = daemon().await;
    let out = pdo(&daemon.url(), &["ext", "inconnue"], None, None).await;
    assert_eq!(out.code, Some(127), "{out}");
    assert!(
        out.stderr
            .contains("no extension `inconnue` in this daemon"),
        "{out}"
    );
    assert!(out.stderr.contains("`pdo ext` lists"), "{out}");
    let out = pdo(&daemon.url(), &["ext", "mute", "anything"], None, None).await;
    assert_eq!(out.code, Some(127), "{out}");
    assert!(
        out.stderr.contains("extension `mute` has no command line"),
        "{out}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_extension_or_an_unreachable_daemon_exits_1() {
    let (daemon, _) = daemon().await;
    let out = pdo(&daemon.url(), &["ext", "test", "panic"], None, None).await;
    assert_eq!(out.code, Some(1), "{out}");
    assert!(
        out.stderr
            .contains("extension `test` failed to run its command line"),
        "{out}"
    );
    let out = pdo("http://127.0.0.1:1", &["ext", "test", "echo"], None, None).await;
    assert_eq!(out.code, Some(1), "{out}");
    assert!(out.stderr.contains("failed to reach daemon"), "{out}");
    let out = pdo("http://127.0.0.1:1", &["ext"], None, None).await;
    assert_eq!(out.code, Some(1), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn pdo_ext_alone_lists_the_extensions_that_have_a_command_line() {
    let (daemon, _) = daemon().await;
    let out = pdo(&daemon.url(), &["ext"], None, None).await;
    assert_eq!(out.code, Some(0), "{out}");
    assert_eq!(
        out.stdout(),
        "test\tEcho, cat, exit with a code: the test of `pdo ext`\n",
        "only the extension with a command line: {out}"
    );

    let index = daemon.get_json("/ext").await;
    assert_eq!(
        index[0]["cli"],
        json!({ "summary": "Echo, cat, exit with a code: the test of `pdo ext`" })
    );
    assert!(index[1].get("cli").is_none(), "{index}");
}

/// The daemon's side, on HTTP: base64 both ways, the named errors.
#[tokio::test(flavor = "multi_thread")]
async fn the_relay_speaks_base64_and_names_its_errors() {
    let (daemon, _) = daemon().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/ext-cli/test", daemon.url()))
        .json(&json!({ "args": ["cat"], "stdin": "c2FsdXQ=" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 0);
    assert_eq!(body["stdout"], "c2FsdXQ=", "{body}");

    let resp = client
        .post(format!("{}/ext-cli/test", daemon.url()))
        .json(&json!({ "args": ["cat"], "stdin": "not base64!" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "bad_request", "{body}");

    let resp = client
        .post(format!("{}/ext-cli/mute", daemon.url()))
        .json(&json!({ "args": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "cli_not_found", "{body}");
}
