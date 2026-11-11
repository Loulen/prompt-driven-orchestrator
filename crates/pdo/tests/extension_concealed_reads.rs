//! Reads that do not leak out of a Projet, against the real daemon and the
//! test extension. Two Projets, P1 (the daemon's repository) and P2 (another),
//! a Run in each.
//!
//! - A concealed refusal (`?` in the test extension's rules) of a read of one
//!   entity answers exactly what an id nobody knows answers: the Run and its
//!   nodes (HTTP and `pdo run status`), a Trigger and its fires. Without the
//!   mark, the readable `403` stays.
//! - The Stats count only the Runs (and Triggers) their caller may read: a
//!   reader of no Run reads zeros and no Projet's name.
//! - Each read of the shared configuration is its Operation `read`, in no
//!   Projet; refused, it answers `403` with the sentence.

use std::process::Command;

use pdo::extension_api::{ResourceKind, Verb};
use serde_json::{json, Value};

use crate::common::flow::{git_init, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{Control, Refusal, TestExtension, ACTOR_HEADER};

/// A Run id no Run has.
const UNKNOWN_RUN: &str = "20990101-000000-0000000";
/// A Trigger id no Trigger has.
const UNKNOWN_TRIGGER: &str = "trg-20990101-000000-0000000";

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    seed_solo(repo, &[("solo", false)])
}

/// One answer, as bytes: status, content type and body.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    status: u16,
    content_type: String,
    body: String,
}

/// `GET path`, as `actor` when given.
async fn raw(daemon: &TestDaemon, path: &str, actor: Option<&str>) -> Answer {
    let mut request = reqwest::Client::new().get(format!("{}{path}", daemon.url()));
    if let Some(actor) = actor {
        request = request.header(ACTOR_HEADER, actor);
    }
    let resp = request.send().await.unwrap();
    Answer {
        status: resp.status().as_u16(),
        content_type: resp
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default(),
        body: resp.text().await.unwrap_or_default(),
    }
}

async fn get_json(daemon: &TestDaemon, path: &str, actor: Option<&str>) -> (u16, Value) {
    let answer = raw(daemon, path, actor).await;
    (
        answer.status,
        serde_json::from_str(&answer.body).unwrap_or(Value::Null),
    )
}

async fn post(daemon: &TestDaemon, path: &str, body: Value) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}{path}", daemon.url()))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// P1 owns the daemon's repository, P2 another; a Run and a Trigger in each.
struct World {
    daemon: TestDaemon,
    control: Control,
    p2: String,
    r1: String,
    r2: String,
    t2: String,
    _other: tempfile::TempDir,
}

async fn world() -> World {
    let (ext, control) = TestExtension::new(Refusal::Nothing);
    let daemon = TestDaemon::spawn_with_extensions(seed, ext.boxed())
        .await
        .unwrap();
    let other = tempfile::tempdir().unwrap();
    git_init(other.path()).unwrap();
    let other_repo = other.path().to_string_lossy().into_owned();
    let mut ids = Vec::new();
    for (name, repo) in [("P1", daemon.target_repo()), ("P2", other_repo.clone())] {
        let (status, project) = post(
            &daemon,
            "/projects",
            json!({ "name": name, "members": [repo] }),
        )
        .await;
        assert_eq!(status, 201, "{project}");
        let project = project["id"].as_str().unwrap().to_string();
        let (status, run) = post(
            &daemon,
            "/runs",
            json!({ "pipeline": "solo", "input": "go", "target_repo": repo, "project_id": project }),
        )
        .await;
        assert_eq!(status, 201, "{run}");
        let (status, trigger) = post(
            &daemon,
            "/triggers",
            json!({
                "name": format!("nightly {name}"),
                "pipeline_id": "solo",
                "cron": "0 3 * * *",
                "input_template": "go",
                "target_repo": repo,
                "project_id": project,
            }),
        )
        .await;
        assert_eq!(status, 201, "{trigger}");
        ids.push((
            project,
            run["run_id"].as_str().unwrap().to_string(),
            trigger["id"].as_str().unwrap().to_string(),
        ));
    }
    let (_, r1, _) = ids.remove(0);
    let (p2, r2, t2) = ids.remove(0);
    World {
        daemon,
        control,
        p2,
        r1,
        r2,
        t2,
        _other: other,
    }
}

/// The answers to `path` (a `{}` for the id) about `hidden` and about
/// `unknown`, as `actor`: the first is written as if it named `unknown`.
async fn both(
    w: &World,
    path: &str,
    hidden: &str,
    unknown: &str,
    actor: Option<&str>,
) -> (Answer, Answer) {
    let mut seen = raw(&w.daemon, &path.replace("{}", hidden), actor).await;
    seen.body = seen.body.replace(hidden, unknown);
    let never = raw(&w.daemon, &path.replace("{}", unknown), actor).await;
    (seen, never)
}

/// The reads of one Run the UI and the CLI make.
const RUN_READS: &[&str] = &[
    "/runs/{}",
    "/runs/{}/events",
    "/runs/{}/children",
    "/runs/{}/pipeline",
    "/runs/{}/artifact?node_id=worker&port=out",
    "/runs/{}/review/comments",
    "/runs/{}/diff",
    "/runs/{}/nodes/worker/pane",
    "/runs/{}/nodes/worker/prompt",
    "/runs/{}/nodes/worker/io",
    "/runs/{}/nodes/worker/transcript",
];

#[tokio::test]
async fn a_concealed_run_answers_byte_for_byte_like_an_unknown_one() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}~bob?", w.p2)).unwrap());

    for path in RUN_READS {
        let (seen, never) = both(&w, path, &w.r2, UNKNOWN_RUN, Some("bob")).await;
        assert_eq!(seen, never, "{path}");
        assert_ne!(seen.status, 403, "{path}: {seen:?}");
    }
    let (status, _) = get_json(&w.daemon, &format!("/runs/{}", w.r2), Some("alice")).await;
    assert_eq!(status, 200, "alice still reads it");
    let (status, _) = get_json(&w.daemon, &format!("/runs/{}", w.r1), Some("bob")).await;
    assert_eq!(status, 200, "bob still reads P1");
    let (_, listed) = get_json(&w.daemon, "/runs", Some("bob")).await;
    assert!(!listed.to_string().contains(&w.r2), "{listed}");
}

/// The Run `id` written as a path may write it: its first character
/// percent-encoded.
fn encoded(id: &str) -> String {
    let mut chars = id.chars();
    let first = chars.next().unwrap();
    format!("%{:02X}{}", first as u32, chars.as_str())
}

#[tokio::test]
async fn neither_an_encoded_id_nor_the_question_of_authorization_tells_a_concealed_run() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}~bob?", w.p2)).unwrap());
    let (seen, never) = both(&w, "/runs/{}", &encoded(&w.r2), UNKNOWN_RUN, Some("bob")).await;
    assert_eq!(seen.status, never.status, "{seen:?}");
    assert_eq!(seen, never);

    let ask = |target: String| {
        let url = format!("{}/authorize", w.daemon.url());
        async move {
            let resp = reqwest::Client::new()
                .post(url)
                .header(ACTOR_HEADER, "bob")
                .json(&json!({ "operations": [{ "resource": "run", "verb": "read", "target": target }] }))
                .send()
                .await
                .unwrap();
            resp.json::<Value>().await.unwrap()
        }
    };
    assert_eq!(ask(w.r2.clone()).await, ask(UNKNOWN_RUN.to_string()).await);
}

#[tokio::test]
async fn without_the_mark_the_refusal_stays_readable() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}~bob", w.p2)).unwrap());
    let (status, body) = get_json(&w.daemon, &format!("/runs/{}", w.r2), Some("bob")).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "operation_refused");
    assert_eq!(
        body["sentence"],
        "You can't read this run here. Ask an administrator of the test extension."
    );
}

/// The real binary, out of any node session: exit code, stdout, stderr.
async fn pdo(daemon: &TestDaemon, args: &[&str]) -> (Option<i32>, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let url = daemon.url();
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdo"));
        cmd.args(&args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("PDO_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("PDO_DAEMON_URL", &url).output().unwrap()
    })
    .await
    .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[tokio::test]
async fn the_cli_tells_a_concealed_run_from_an_unknown_one_no_better() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("run:read@{}?", w.p2)).unwrap());
    for command in [["run", "status"], ["run", "events"]] {
        let (code, stdout, stderr) = pdo(&w.daemon, &[command[0], command[1], &w.r2]).await;
        let seen = (
            code,
            stdout.replace(&w.r2, UNKNOWN_RUN),
            stderr.replace(&w.r2, UNKNOWN_RUN),
        );
        let never = pdo(&w.daemon, &[command[0], command[1], UNKNOWN_RUN]).await;
        assert_eq!(seen, never, "pdo {}", command.join(" "));
    }
}

#[tokio::test]
async fn a_concealed_trigger_answers_like_an_unknown_one() {
    let w = world().await;
    w.control
        .set(Refusal::parse(&format!("trigger:read@{}~bob?", w.p2)).unwrap());
    for path in ["/triggers/{}", "/triggers/{}/fires"] {
        let (seen, never) = both(&w, path, &w.t2, UNKNOWN_TRIGGER, Some("bob")).await;
        assert_eq!(seen, never, "{path}");
    }
    let (status, _) = get_json(&w.daemon, &format!("/triggers/{}", w.t2), Some("alice")).await;
    assert_eq!(status, 200);

    w.control
        .set(Refusal::parse(&format!("trigger:read@{}~bob", w.p2)).unwrap());
    let (status, body) = get_json(&w.daemon, &format!("/triggers/{}", w.t2), Some("bob")).await;
    assert_eq!(status, 403, "without the mark: {body}");
}

/// The number of Runs the overview counts, and the Projets the cost names.
async fn counted(w: &World, actor: &str) -> (i64, Vec<String>, i64) {
    let query = "from=2000-01-01T00:00:00Z&to=2100-01-01T00:00:00Z&bucket=month";
    let (status, overview) =
        get_json(&w.daemon, &format!("/stats/overview?{query}"), Some(actor)).await;
    assert_eq!(status, 200, "{overview}");
    let runs = overview["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["count"].as_i64().unwrap())
        .sum();
    let fires = overview["triggers_created_runs"]["enabled_triggers"]
        .as_i64()
        .unwrap();
    let (status, cost) = get_json(&w.daemon, &format!("/stats/cost?{query}"), Some(actor)).await;
    assert_eq!(status, 200, "{cost}");
    let mut projects: Vec<String> = cost["by_project"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    projects.sort();
    let (status, performance) = get_json(
        &w.daemon,
        "/stats/performance?from=2000-01-01T00:00:00Z&to=2100-01-01T00:00:00Z",
        Some(actor),
    )
    .await;
    assert_eq!(status, 200, "{performance}");
    (runs, projects, fires)
}

#[tokio::test]
async fn the_stats_count_only_what_their_reader_may_read() {
    let w = world().await;
    assert_eq!(
        counted(&w, "alice").await,
        (2, vec!["P1".to_string(), "P2".to_string()], 2)
    );

    // Bob reads P1 alone.
    w.control.set(
        Refusal::parse(&format!(
            "run:read@{p2}~bob?,trigger:read@{p2}~bob?",
            p2 = w.p2
        ))
        .unwrap(),
    );
    assert_eq!(counted(&w, "bob").await, (1, vec!["P1".to_string()], 1));
    assert_eq!(counted(&w, "alice").await.0, 2, "alice still reads both");

    // Carol reads nothing: zeros, and no Projet's name.
    w.control
        .set(Refusal::parse("run:read~carol,trigger:read~carol").unwrap());
    assert_eq!(counted(&w, "carol").await, (0, vec![], 0));

    // The cost of the Runs a list shows stays filtered by Run.
    let (_, costs) = get_json(
        &w.daemon,
        &format!("/runs/costs?ids={},{}", w.r1, w.r2),
        Some("carol"),
    )
    .await;
    assert_eq!(costs, json!({ "runs": [] }));
}

/// Each read of the shared configuration, and the Ressource it reads.
const SHARED_READS: &[(&str, &str)] = &[
    ("/pipelines", "pipeline"),
    ("/pipelines/solo", "pipeline"),
    ("/pipelines/solo/document", "pipeline"),
    ("/library", "library_entry"),
    ("/settings/agent-profiles", "agent_profile"),
    ("/settings/skills", "skill"),
    ("/settings/skill-folders", "skill_folder"),
    ("/settings", "instance_settings"),
    ("/audit", "audit"),
];

#[tokio::test]
async fn each_read_of_the_shared_configuration_is_its_operation() {
    let w = world().await;
    for (path, _) in SHARED_READS {
        let (status, body) = get_json(&w.daemon, path, Some("carol")).await;
        assert_eq!(status, 200, "{path}: {body}");
    }
    let rules: Vec<String> = SHARED_READS
        .iter()
        .map(|(_, resource)| format!("{resource}:read~carol"))
        .collect();
    w.control.set(Refusal::parse(&rules.join(",")).unwrap());
    for (path, resource) in SHARED_READS {
        let (status, body) = get_json(&w.daemon, path, Some("carol")).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert_eq!(body["error"], "operation_refused", "{path}");
        assert_eq!(
            body["sentence"],
            format!(
                "You can't read this {resource} here. Ask an administrator of the test extension."
            ),
            "{path}"
        );
        let (status, _) = get_json(&w.daemon, path, Some("alice")).await;
        assert_eq!(status, 200, "{path}: alice still reads it");
    }
    // Asked as reads, in no Projet.
    let shared: Vec<_> = w
        .control
        .asked()
        .into_iter()
        .map(|(_, op)| op)
        .filter(|op| op.verb == Verb::Read && op.resource == ResourceKind::Audit)
        .collect();
    assert!(!shared.is_empty());
    assert!(shared.iter().all(|op| op.is_outside_projects()));
}
